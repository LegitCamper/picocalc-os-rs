use crate::{
    storage::{File, SDCARD},
    syscalls,
};
use alloc::{vec, vec::Vec};
use bumpalo::Bump;
use core::ptr;
use embedded_sdmmc::ShortFileName;
use goblin::{
    elf::{
        header::header32::Header,
        program_header::program_header32::{PT_LOAD, ProgramHeader},
        reloc::R_ARM_RELATIVE,
        section_header::{SHT_REL, SHT_SYMTAB},
    },
    elf32::{header, reloc::Rel, section_header::SectionHeader, sym::Sym},
};
use strum::IntoEnumIterator;
use userlib_sys::{EntryFn, SyscallTable};

const ELF32_HDR_SIZE: usize = 52;

#[derive(Debug)]
pub enum LoadError {
    FailedToReadFile,
    ElfIsNotPie,
    UnknownRelocationType,
    SyscallTableNotFound,
}

pub async unsafe fn load_binary(name: &ShortFileName) -> Result<(EntryFn, Bump), LoadError> {
    let mut sd_lock = SDCARD.get().lock().await;
    let sd = sd_lock.as_mut().expect("Sdcard locked");

    let mut header_buf = [0; ELF32_HDR_SIZE];

    sd.read_file(name, |mut file| {
        file.read(&mut header_buf)
            .map_err(|_| LoadError::FailedToReadFile)?;
        let elf_header = Header::from_bytes(&header_buf);

        // reject non-PIE
        if elf_header.e_type != header::ET_DYN {
            return Err(LoadError::ElfIsNotPie);
        }

        let mut ph_buf = vec![0_u8; elf_header.e_phentsize as usize];

        let (total_size, min_vaddr, _max_vaddr) =
            total_loadable_size(&mut file, elf_header, &mut ph_buf)?;

        let bump = Bump::with_capacity(total_size);
        let base = bump.alloc_slice_fill_default::<u8>(total_size);

        // load each segment into bump, relative to base_ptr
        for i in 0..elf_header.e_phnum {
            file.seek_from_start(elf_header.e_phoff + (elf_header.e_phentsize * i) as u32)
                .map_err(|_| LoadError::FailedToReadFile)?;
            file.read(&mut ph_buf)
                .map_err(|_| LoadError::FailedToReadFile)?;
            let ph = cast_phdr(&ph_buf);

            let seg_offset = (ph.p_vaddr - min_vaddr) as usize;
            let segment = &mut base[seg_offset..seg_offset + ph.p_memsz as usize];

            if ph.p_type == PT_LOAD {
                load_segment(&mut file, &ph, segment)?;
            }
        }

        for i in 0..elf_header.e_shnum {
            let sh = read_section(&mut file, elf_header, i.into())?;

            if sh.sh_type == SHT_REL {
                apply_relocations(&sh, min_vaddr, base.as_mut_ptr(), &mut file)?;
            }
        }

        patch_syscalls(elf_header, base.as_mut_ptr(), min_vaddr, &mut file)?;

        // entry pointer is base_ptr + (entry - min_vaddr)
        let entry_ptr: EntryFn = unsafe {
            core::mem::transmute(base.as_ptr().add((elf_header.e_entry - min_vaddr) as usize))
        };

        Ok((entry_ptr, bump))
    })
    .await
    .map_err(|_| LoadError::FailedToReadFile)?
}

fn load_segment(file: &mut File, ph: &ProgramHeader, segment: &mut [u8]) -> Result<(), LoadError> {
    let filesz = ph.p_filesz as usize;
    let memsz = ph.p_memsz as usize;

    // read file contents
    let mut remaining = filesz;
    let mut dst_offset = 0;
    let mut file_offset = ph.p_offset;
    let mut buf = [0u8; 512];

    while remaining > 0 {
        let to_read = core::cmp::min(remaining, buf.len());
        file.seek_from_start(file_offset)
            .map_err(|_| LoadError::FailedToReadFile)?;
        file.read(&mut buf[..to_read])
            .map_err(|_| LoadError::FailedToReadFile)?;

        segment[dst_offset..dst_offset + to_read].copy_from_slice(&buf[..to_read]);

        remaining -= to_read;
        dst_offset += to_read;
        file_offset += to_read as u32;
    }

    // zero BSS if needed
    if memsz > filesz {
        segment[filesz..].fill(0);
    }

    Ok(())
}

fn apply_relocations(
    sh: &SectionHeader,
    min_vaddr: u32,
    base: *mut u8,
    file: &mut File,
) -> Result<(), LoadError> {
    let mut reloc = [0_u8; 8];

    let num_relocs = sh.sh_size as usize / sh.sh_entsize as usize;

    for i in 0..num_relocs {
        file.seek_from_start(sh.sh_offset + (i as u32 * 8))
            .map_err(|_| LoadError::FailedToReadFile)?;
        file.read(&mut reloc)
            .map_err(|_| LoadError::FailedToReadFile)?;

        let rel = cast_rel(&reloc);

        let reloc_type = rel.r_info & 0xff;
        let reloc_addr = unsafe { base.add((rel.r_offset - min_vaddr) as usize) as *mut u32 };

        match reloc_type {
            R_ARM_RELATIVE => {
                // REL: add base to the word already stored there
                unsafe {
                    let val = ptr::read_unaligned(reloc_addr);
                    ptr::write_unaligned(reloc_addr, val.wrapping_add(base as u32));
                }
            }
            _ => {
                return Err(LoadError::UnknownRelocationType);
            }
        }
    }
    Ok(())
}

fn patch_syscalls(
    elf_header: &Header,
    base: *mut u8,
    min_vaddr: u32,
    file: &mut File,
) -> Result<(), LoadError> {
    for i in 1..=elf_header.e_shnum {
        let sh = read_section(file, elf_header, i.into())?;

        // find the symbol table
        if sh.sh_type == SHT_SYMTAB {
            let mut symtab_buf = vec![0u8; sh.sh_size as usize];
            file.seek_from_start(sh.sh_offset)
                .map_err(|_| LoadError::FailedToReadFile)?;
            file.read(&mut symtab_buf)
                .map_err(|_| LoadError::FailedToReadFile)?;

            // Cast buffer into symbols
            let sym_count = sh.sh_size as usize / sh.sh_entsize as usize;
            for i in 0..sym_count {
                let sym_bytes =
                    &symtab_buf[i * sh.sh_entsize as usize..(i + 1) * sh.sh_entsize as usize];
                let sym = cast_sym(sym_bytes);

                let str_sh = read_section(file, elf_header, sh.sh_link)?;

                let mut name = Vec::new();
                file.seek_from_start(str_sh.sh_offset + sym.st_name)
                    .map_err(|_| LoadError::FailedToReadFile)?;

                loop {
                    let mut byte = [0u8; 1];
                    file.read(&mut byte)
                        .map_err(|_| LoadError::FailedToReadFile)?;
                    if byte[0] == 0 {
                        break;
                    }
                    name.push(byte[0]);
                }

                let symbol_name = core::str::from_utf8(&name).expect("symbol was not utf8");
                if symbol_name == stringify!(SYS_CALL_TABLE) {
                    let table_base =
                        unsafe { base.add((sym.st_value as usize) - min_vaddr as usize) }
                            as *mut usize;

                    for (idx, call) in SyscallTable::iter().enumerate() {
                        let ptr = match call {
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
                        };
                        unsafe {
                            table_base.add(idx).write(ptr);
                        }
                    }
                    return Ok(());
                }
            }
        }
    }
    Err(LoadError::SyscallTableNotFound)
}

fn total_loadable_size(
    file: &mut File,
    elf_header: &Header,
    ph_buf: &mut [u8],
) -> Result<(usize, u32, u32), LoadError> {
    let mut min_vaddr = u32::MAX;
    let mut max_vaddr = 0u32;
    for i in 0..elf_header.e_phnum {
        file.seek_from_start(elf_header.e_phoff + (elf_header.e_phentsize * i) as u32)
            .map_err(|_| LoadError::FailedToReadFile)?;
        file.read(ph_buf).map_err(|_| LoadError::FailedToReadFile)?;
        let ph = cast_phdr(ph_buf);

        if ph.p_type == PT_LOAD {
            if ph.p_vaddr < min_vaddr {
                min_vaddr = ph.p_vaddr;
            }
            if ph.p_vaddr + ph.p_memsz > max_vaddr {
                max_vaddr = ph.p_vaddr + ph.p_memsz;
            }
        }
    }

    let total_size = (max_vaddr - min_vaddr) as usize;
    Ok((total_size, min_vaddr, max_vaddr))
}

fn read_section(
    file: &mut File,
    elf_header: &Header,
    section: u32,
) -> Result<SectionHeader, LoadError> {
    let mut sh_buf = vec![0_u8; elf_header.e_shentsize as usize];

    file.seek_from_start(elf_header.e_shoff + (elf_header.e_shentsize as u32 * section))
        .map_err(|_| LoadError::FailedToReadFile)?;
    file.read(&mut sh_buf)
        .map_err(|_| LoadError::FailedToReadFile)?;

    Ok(cast_shdr(&sh_buf))
}

fn cast_phdr(buf: &[u8]) -> ProgramHeader {
    assert!(buf.len() >= core::mem::size_of::<ProgramHeader>());
    unsafe { core::ptr::read(buf.as_ptr() as *const ProgramHeader) }
}

fn cast_shdr(buf: &[u8]) -> SectionHeader {
    assert!(buf.len() >= core::mem::size_of::<SectionHeader>());
    unsafe { core::ptr::read(buf.as_ptr() as *const SectionHeader) }
}

fn cast_sym(buf: &[u8]) -> Sym {
    assert!(buf.len() >= core::mem::size_of::<Sym>());
    unsafe { core::ptr::read(buf.as_ptr() as *const Sym) }
}

fn cast_rel(buf: &[u8]) -> Rel {
    assert!(buf.len() >= core::mem::size_of::<Rel>());
    unsafe { core::ptr::read(buf.as_ptr() as *const Rel) }
}
