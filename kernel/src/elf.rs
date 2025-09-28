use crate::{
    abi,
    storage::{File, SDCARD},
};
use abi_sys::{CallAbiTable, EntryFn};
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

const ELF32_HDR_SIZE: usize = 52;

pub async unsafe fn load_binary(name: &ShortFileName) -> Option<(EntryFn, Bump)> {
    let mut sd_lock = SDCARD.get().lock().await;
    let sd = sd_lock.as_mut().unwrap();

    let mut header_buf = [0; ELF32_HDR_SIZE];

    let (entry, bump) = sd
        .read_file(name, |mut file| {
            file.read(&mut header_buf).unwrap();
            let elf_header = Header::from_bytes(&header_buf);

            // reject non-PIE
            if elf_header.e_type != header::ET_DYN {
                return None;
            }

            let mut ph_buf = vec![0_u8; elf_header.e_phentsize as usize];

            let (total_size, min_vaddr, _max_vaddr) =
                total_loadable_size(&mut file, &elf_header, &mut ph_buf);

            let bump = Bump::with_capacity(total_size);
            let base = bump.alloc_slice_fill_default::<u8>(total_size);

            // load each segment into bump, relative to base_ptr
            for i in 0..elf_header.e_phnum {
                file.seek_from_start(elf_header.e_phoff + (elf_header.e_phentsize * i) as u32)
                    .unwrap();
                file.read(&mut ph_buf).unwrap();
                let ph = cast_phdr(&ph_buf);

                let seg_offset = (ph.p_vaddr - min_vaddr) as usize;
                let mut segment = &mut base[seg_offset..seg_offset + ph.p_memsz as usize];

                if ph.p_type == PT_LOAD {
                    load_segment(&mut file, &ph, &mut segment).unwrap();
                }
            }

            for i in 0..elf_header.e_shnum {
                let sh = read_section(&mut file, elf_header, i.into());

                match sh.sh_type {
                    SHT_REL => {
                        apply_relocations(&sh, min_vaddr, base.as_mut_ptr(), &mut file).unwrap();
                    }
                    _ => {}
                }
            }

            patch_abi(&elf_header, base.as_mut_ptr(), min_vaddr, &mut file).unwrap();

            // entry pointer is base_ptr + (entry - min_vaddr)
            let entry_ptr: EntryFn = unsafe {
                core::mem::transmute(base.as_ptr().add((elf_header.e_entry - min_vaddr) as usize))
            };

            Some((entry_ptr, bump))
        })
        .await
        .expect("Failed to read file")?;

    Some((entry, bump))
}

fn load_segment(file: &mut File, ph: &ProgramHeader, segment: &mut [u8]) -> Result<(), ()> {
    let filesz = ph.p_filesz as usize;
    let memsz = ph.p_memsz as usize;

    // read file contents
    let mut remaining = filesz;
    let mut dst_offset = 0;
    let mut file_offset = ph.p_offset;
    let mut buf = [0u8; 512];

    while remaining > 0 {
        let to_read = core::cmp::min(remaining, buf.len());
        file.seek_from_start(file_offset).unwrap();
        file.read(&mut buf[..to_read]).unwrap();

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
) -> Result<(), ()> {
    let mut reloc = [0_u8; 8];

    let num_relocs = sh.sh_size as usize / sh.sh_entsize as usize;

    for i in 0..num_relocs {
        file.seek_from_start(sh.sh_offset + (i as u32 * 8)).unwrap();
        file.read(&mut reloc).unwrap();

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
                return Err(());
            }
        }
    }
    Ok(())
}

fn patch_abi(
    elf_header: &Header,
    base: *mut u8,
    min_vaddr: u32,
    file: &mut File,
) -> Result<(), ()> {
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
                    let table_base =
                        unsafe { base.add((sym.st_value as usize) - min_vaddr as usize) }
                            as *mut usize;

                    for (idx, call) in CallAbiTable::iter().enumerate() {
                        let ptr = match call {
                            CallAbiTable::PrintString => abi::print as usize,
                            CallAbiTable::SleepMs => abi::sleep as usize,
                            CallAbiTable::LockDisplay => abi::lock_display as usize,
                            CallAbiTable::DrawIter => abi::draw_iter as usize,
                            CallAbiTable::GetKey => abi::get_key as usize,
                            CallAbiTable::GenRand => abi::gen_rand as usize,
                            CallAbiTable::ListDir => abi::list_dir as usize,
                            CallAbiTable::ReadFile => abi::read_file as usize,
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

fn total_loadable_size(
    file: &mut File,
    elf_header: &Header,
    ph_buf: &mut [u8],
) -> (usize, u32, u32) {
    let mut min_vaddr = u32::MAX;
    let mut max_vaddr = 0u32;
    for i in 0..elf_header.e_phnum {
        file.seek_from_start(elf_header.e_phoff + (elf_header.e_phentsize * i) as u32)
            .unwrap();
        file.read(ph_buf).unwrap();
        let ph = cast_phdr(&ph_buf);

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
    (total_size, min_vaddr, max_vaddr)
}

fn read_section(file: &mut File, elf_header: &Header, section: u32) -> SectionHeader {
    let mut sh_buf = vec![0_u8; elf_header.e_shentsize as usize];

    file.seek_from_start(elf_header.e_shoff + (elf_header.e_shentsize as u32 * section))
        .unwrap();
    file.read(&mut sh_buf).unwrap();

    cast_shdr(&sh_buf)
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
