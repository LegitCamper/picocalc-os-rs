use crate::{
    storage::{File as SdFile, SDCARD},
    syscalls,
};
use bumpalo::Bump;
use core::alloc::Layout;
use embedded_sdmmc::ShortFileName;
use strum::IntoEnumIterator;
use userlib_sys::read_cache::{ReadCache, ReadError};
use userlib_sys::{EntryFn, SYS_CALL_TABLE_COUNT, SyscallTable};

#[path = "elf_format.rs"]
mod format;
pub use format::LoadError;
use format::{Header, Image, ProgramHeader, SectionHeader};

struct File<'a> {
    inner: SdFile<'a>,
    cache: ReadCache,
}

impl File<'_> {
    fn length(&self) -> u32 {
        self.inner.length()
    }
}

/// # Safety
/// The entry point contains untrusted native code, not an isolated process.
/// Keep the returned arena alive until execution has finished.
pub async unsafe fn load_binary(
    name: &ShortFileName,
) -> Result<(EntryFn, Bump, userlib_sys::memory::Region), LoadError> {
    let mut sd_lock = SDCARD.get().lock().await;
    let sd = sd_lock.as_mut().ok_or(LoadError::FailedToReadFile)?;
    sd.read_file(name, |file| {
        #[cfg(feature = "defmt")]
        let _started = embassy_time::Instant::now();
        let mut file = File {
            cache: ReadCache::new(file.length()),
            inner: file,
        };
        let mut bytes = [0; format::HEADER_SIZE];
        read_at(&mut file, 0, &mut bytes)?;
        let header = Header::parse(&bytes, file.length())?;
        let mut segments = [ProgramHeader::EMPTY; format::MAX_PROGRAM_HEADERS];
        let mut segment_count = 0;
        for index in 0..header.program_count {
            let mut bytes = [0; format::PROGRAM_HEADER_SIZE];
            read_at(&mut file, header.program_offset(index)?, &mut bytes)?;
            let segment = ProgramHeader::parse(&bytes, file.length())?;
            if segment.kind == format::PT_LOAD && segment.memory_size != 0 {
                segments[segment_count] = segment;
                segment_count += 1;
            }
        }
        let segments = &segments[..segment_count];
        let image = Image::new(segments, header.entry)?;
        let layout = Layout::from_size_align(image.size, image.alignment)
            .map_err(|_| LoadError::InvalidElf)?;
        let bump = Bump::try_with_capacity(0).map_err(|_| LoadError::OutOfMemory)?;
        bump.set_allocation_limit(Some(format::MAX_ALLOCATION));
        let allocation = bump
            .try_alloc_layout(layout)
            .map_err(|_| LoadError::OutOfMemory)?;
        // SAFETY: allocation owns image.size bytes until bump is dropped. Initialize
        // all bytes, including BSS and holes, before constructing the mutable slice.
        let base = unsafe {
            allocation.as_ptr().write_bytes(0, image.size);
            core::slice::from_raw_parts_mut(allocation.as_ptr(), image.size)
        };
        for segment in segments {
            let range = image.range(segment.address, segment.file_size, 0)?;
            read_at(&mut file, segment.offset, &mut base[range])?;
        }
        #[cfg(feature = "defmt")]
        let _image_done = embassy_time::Instant::now();
        let mut relocation_count = 0;
        for index in 1..header.section_count {
            let section = read_section(&mut file, &header, index)?;
            if section.flags & format::SHF_ALLOC != 0 && section.size != 0 {
                image.range(section.address, section.size, 0)?;
            }
            match section.kind {
                format::SHT_REL => {
                    relocation_count += section.size / format::REL_SIZE as u32;
                    if relocation_count > format::MAX_RELOCATIONS {
                        return Err(LoadError::TooLarge);
                    }
                    apply_relocations(&mut file, &header, &section, &image, base)?;
                }
                format::SHT_RELA => return Err(LoadError::UnknownRelocationType),
                _ => {}
            }
        }
        #[cfg(feature = "defmt")]
        let _relocations_done = embassy_time::Instant::now();
        patch_syscalls(&mut file, &header, &image, base)?;
        #[cfg(feature = "defmt")]
        {
            let _done = embassy_time::Instant::now();
            defmt::info!(
                "ELF load ms: image={} relocations={} symbols={} total={}",
                (_image_done - _started).as_millis(),
                (_relocations_done - _image_done).as_millis(),
                (_done - _relocations_done).as_millis(),
                (_done - _started).as_millis(),
            );
        }
        let entry_offset = image.range(header.entry & !1, 2, format::PF_X)?.start;
        let entry_address = (base.as_ptr() as usize + entry_offset) | 1;
        // SAFETY: entry is a Thumb address in loaded executable bytes. Executing
        // untrusted machine code is part of load_binary's caller contract.
        let entry = unsafe { core::mem::transmute::<usize, EntryFn>(entry_address) };
        let region = userlib_sys::memory::Region {
            start: base.as_ptr() as usize,
            len: base.len(),
        };
        Ok((entry, bump, region))
    })
    .await
    .map_err(|_| LoadError::FailedToReadFile)?
}

fn read_at(file: &mut File, offset: u32, bytes: &mut [u8]) -> Result<(), LoadError> {
    let size = u32::try_from(bytes.len()).map_err(|_| LoadError::TooLarge)?;
    format::checked_range(offset, size, file.length())?;
    file.cache
        .read_at(offset, bytes, |offset, bytes| {
            file.inner
                .seek_from_start(offset)
                .map_err(|_| LoadError::FailedToReadFile)?;
            format::read_exact_with(bytes, |remaining| {
                file.inner
                    .read(remaining)
                    .map_err(|_| LoadError::FailedToReadFile)
            })
        })
        .map_err(|error| match error {
            ReadError::OutOfBounds => LoadError::InvalidElf,
            ReadError::Source(error) => error,
        })
}

fn read_section(file: &mut File, header: &Header, index: u16) -> Result<SectionHeader, LoadError> {
    let mut bytes = [0; format::SECTION_HEADER_SIZE];
    read_at(file, header.section_offset(index)?, &mut bytes)?;
    SectionHeader::parse(&bytes, file.length(), header.section_count)
}

fn apply_relocations(
    file: &mut File,
    header: &Header,
    section: &SectionHeader,
    image: &Image<'_>,
    base: &mut [u8],
) -> Result<(), LoadError> {
    let symbols = read_section(file, header, section.link as u16)?;
    if !matches!(symbols.kind, format::SHT_SYMTAB | format::SHT_DYNSYM) {
        return Err(LoadError::InvalidElf);
    }
    let target = if section.info != 0 {
        let target = read_section(file, header, section.info as u16)?;
        if target.flags & format::SHF_ALLOC == 0 {
            return Err(LoadError::InvalidElf);
        }
        Some(target)
    } else {
        None
    };
    for index in 0..section.size / format::REL_SIZE as u32 {
        let mut bytes = [0; format::REL_SIZE];
        read_at(
            file,
            section.offset + index * format::REL_SIZE as u32,
            &mut bytes,
        )?;
        let Some(range) = image.relocation_range(&bytes, target.as_ref())? else {
            continue;
        };
        let addend = u32::from_le_bytes([
            base[range.start],
            base[range.start + 1],
            base[range.start + 2],
            base[range.start + 3],
        ]);
        let value = format::relative_value(base.as_ptr() as u32, image.origin, addend);
        base[range].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_syscalls(
    file: &mut File,
    header: &Header,
    image: &Image<'_>,
    base: &mut [u8],
) -> Result<(), LoadError> {
    const NAME: &[u8] = b"SYS_CALL_TABLE\0";
    let mut table_address = None;
    let mut symbol_count = 0;
    for section_index in 1..header.section_count {
        let symbols = read_section(file, header, section_index)?;
        if !matches!(symbols.kind, format::SHT_SYMTAB | format::SHT_DYNSYM) {
            continue;
        }
        let strings = read_section(file, header, symbols.link as u16)?;
        if strings.kind != format::SHT_STRTAB {
            return Err(LoadError::InvalidElf);
        }
        symbol_count += symbols.size / format::SYMBOL_SIZE as u32;
        if symbol_count > format::MAX_SYMBOLS {
            return Err(LoadError::TooLarge);
        }
        for index in 0..symbols.size / format::SYMBOL_SIZE as u32 {
            let mut bytes = [0; format::SYMBOL_SIZE];
            read_at(
                file,
                symbols.offset + index * format::SYMBOL_SIZE as u32,
                &mut bytes,
            )?;
            let symbol = format::Symbol::parse(&bytes);
            if symbol.name >= strings.size {
                return Err(LoadError::InvalidElf);
            }
            if strings.size - symbol.name < NAME.len() as u32 {
                continue;
            }
            let mut name = [0; NAME.len()];
            read_at(file, strings.offset + symbol.name, &mut name)?;
            if name != NAME {
                continue;
            }
            symbol.validate_section(header.section_count)?;
            let section = read_section(file, header, symbol.section)?;
            image.syscall_range(&symbol, &section, SYS_CALL_TABLE_COUNT)?;
            if table_address.is_some_and(|previous| previous != symbol.address) {
                return Err(LoadError::InvalidElf);
            }
            table_address = Some(symbol.address);
        }
    }
    let address = table_address.ok_or(LoadError::SyscallTableNotFound)?;
    let range = image.range(address, (SYS_CALL_TABLE_COUNT * 4) as u32, format::PF_W)?;
    for (slot, call) in base[range].chunks_exact_mut(4).zip(SyscallTable::iter()) {
        let pointer = match call {
            SyscallTable::Alloc => syscalls::alloc as usize,
            SyscallTable::Dealloc => syscalls::dealloc as usize,
            SyscallTable::PrintString => syscalls::print as usize,
            SyscallTable::SleepMs => syscalls::sleep as usize,
            SyscallTable::GetMs => syscalls::get_ms as usize,
            SyscallTable::DrawIter => syscalls::draw_iter as usize,
            SyscallTable::GetKey => syscalls::get_key as usize,
            SyscallTable::GenRand => syscalls::gen_rand as usize,
            SyscallTable::ListDir => syscalls::list_dir as usize,
            SyscallTable::ReadFile => syscalls::read_file as usize,
            SyscallTable::WriteFile => syscalls::write_file as usize,
            SyscallTable::FileLen => syscalls::file_len as usize,
            SyscallTable::ReconfigureAudioSampleRate => {
                syscalls::reconfigure_audio_sample_rate as usize
            }
            SyscallTable::AudioBufferReady => syscalls::audio_buffer_ready as usize,
            SyscallTable::SendAudioBuffer => syscalls::send_audio_buffer as usize,
            SyscallTable::FillRect => syscalls::fill_rect as usize,
            SyscallTable::Blit => syscalls::blit as usize,
            SyscallTable::GetLastError => syscalls::get_last_error as usize,
        };
        slot.copy_from_slice(&(pointer as u32).to_le_bytes());
    }
    Ok(())
}
