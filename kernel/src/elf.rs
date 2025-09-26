#![allow(static_mut_refs)]

use crate::{
    abi,
    storage::{File, SDCARD},
};
use abi_sys::{CallAbiTable, EntryFn};
use alloc::{vec, vec::Vec};
use embedded_sdmmc::ShortFileName;
use goblin::{
    elf::{
        header::header32::Header,
        program_header::program_header32::{PT_LOAD, ProgramHeader},
        section_header::SHT_SYMTAB,
    },
    elf32::{section_header::SectionHeader, sym::Sym},
};
use strum::IntoEnumIterator;

const ELF32_HDR_SIZE: usize = 52;

// userland ram region defined in memory.x
unsafe extern "C" {
    static __userapp_start__: u8;
    static __userapp_end__: u8;
}

pub async unsafe fn load_binary(name: &ShortFileName) -> Result<EntryFn, &str> {
    let mut sd_lock = SDCARD.get().lock().await;
    let sd = sd_lock.as_mut().unwrap();

    let error = "";
    let mut entry = 0;

    let mut header_buf = [0; ELF32_HDR_SIZE];

    sd.read_file(name, |mut file| {
        file.read(&mut header_buf).unwrap();
        let elf_header = Header::from_bytes(&header_buf);

        let mut program_headers_buf = vec![0_u8; elf_header.e_phentsize as usize];
        for i in 1..=elf_header.e_phnum {
            file.seek_from_start(elf_header.e_phoff + (elf_header.e_phentsize * i) as u32)
                .unwrap();
            file.read(&mut program_headers_buf).unwrap();

            let ph = cast_phdr(&program_headers_buf);

            if ph.p_type == PT_LOAD {
                load_segment(&mut file, &ph).unwrap()
            }
        }

        patch_abi(&elf_header, &mut file).unwrap();

        // TODO: dynamically search for abi table

        entry = elf_header.e_entry as u32;
    })
    .await
    .unwrap();

    if entry != 0 {
        Ok(unsafe { core::mem::transmute(entry) })
    } else {
        Err(error)
    }
}

fn patch_abi(elf_header: &Header, file: &mut File) -> Result<(), ()> {
    for i in 1..=elf_header.e_shnum {
        let sh = read_section(file, &elf_header, i.into());

        // find the symbol table
        if sh.sh_type == SHT_SYMTAB {
            let mut symtab_buf = vec![0u8; sh.sh_size as usize];
            file.seek_from_start(sh.sh_offset).unwrap();
            file.read(&mut symtab_buf).unwrap();

            // Cast buffer into symbols
            let sym_count = sh.sh_size as usize / sh.sh_entsize as usize;
            for i in 0..sym_count {
                let sym_bytes =
                    &symtab_buf[i * sh.sh_entsize as usize..(i + 1) * sh.sh_entsize as usize];
                let sym = cast_sym(sym_bytes);

                let str_sh = read_section(file, &elf_header, sh.sh_link);

                let mut name = Vec::new();
                file.seek_from_start(str_sh.sh_offset + sym.st_name)
                    .unwrap();

                loop {
                    let mut byte = [0u8; 1];
                    file.read(&mut byte).unwrap();
                    if byte[0] == 0 {
                        break;
                    }
                    name.push(byte[0]);
                }

                let symbol_name = core::str::from_utf8(&name).unwrap();
                if symbol_name == "CALL_ABI_TABLE" {
                    let table_base = sym.st_value as *mut usize;

                    for (idx, call) in CallAbiTable::iter().enumerate() {
                        let ptr = match call {
                            CallAbiTable::Print => abi::print as usize,
                            CallAbiTable::Sleep => abi::sleep as usize,
                            CallAbiTable::LockDisplay => abi::lock_display as usize,
                            CallAbiTable::DrawIter => abi::draw_iter as usize,
                            CallAbiTable::GetKey => abi::get_key as usize,
                            CallAbiTable::GenRand => abi::gen_rand as usize,
                        };
                        unsafe {
                            table_base.add(idx as usize).write(ptr);
                        }
                    }
                    return Ok(());
                }
            }
        }
    }
    Err(())
}

fn read_section(file: &mut File, elf_header: &Header, section: u32) -> SectionHeader {
    let mut section_header_buf = vec![0_u8; elf_header.e_shentsize as usize];

    file.seek_from_start(elf_header.e_shoff + (elf_header.e_shentsize as u32 * section))
        .unwrap();
    file.read(&mut section_header_buf).unwrap();

    cast_shdr(&section_header_buf)
}

fn load_segment(file: &mut File, ph: &ProgramHeader) -> Result<(), ()> {
    let dst_start = ph.p_vaddr as *mut u8;
    let filesz = ph.p_filesz as usize;
    let memsz = ph.p_memsz as usize;
    let vaddr = ph.p_vaddr as usize;
    let mut remaining = filesz;
    let mut dst_ptr = dst_start;
    let mut file_offset = ph.p_offset;

    let seg_start = vaddr;
    let seg_end = vaddr + memsz;

    // Bounds check: make sure segment fits inside payload region
    let user_start = unsafe { &__userapp_start__ as *const u8 as usize };
    let user_end = unsafe { &__userapp_end__ as *const u8 as usize };
    if seg_start < user_start || seg_end > user_end {
        panic!(
            "Segment out of bounds: {:x}..{:x} not within {:x}..{:x}",
            seg_start, seg_end, user_start, user_end
        );
    }

    // Buffer for chunked reads (512 bytes is typical SD sector size)
    let mut buf = [0u8; 512];

    while remaining > 0 {
        let to_read = core::cmp::min(remaining, buf.len());
        // Read chunk from file
        file.seek_from_start(file_offset).unwrap();
        file.read(&mut buf[..to_read]).unwrap();

        unsafe {
            // Copy chunk directly into destination memory
            core::ptr::copy_nonoverlapping(buf.as_ptr(), dst_ptr, to_read);
            dst_ptr = dst_ptr.add(to_read);
        }

        remaining -= to_read;
        file_offset += to_read as u32;
    }

    // Zero BSS (memsz - filesz)
    if memsz > filesz {
        unsafe {
            core::ptr::write_bytes(dst_ptr, 0, memsz - filesz);
        }
    }

    Ok(())
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
