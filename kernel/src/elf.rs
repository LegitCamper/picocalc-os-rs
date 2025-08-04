#![allow(static_mut_refs)]
use abi::Syscall;
use bumpalo::Bump;
use core::{alloc::Layout, ffi::c_void, ptr::NonNull, slice::from_raw_parts_mut};
use goblin::{
    elf::{Elf, header::ET_DYN, program_header::PT_LOAD, sym},
    elf32,
};

use crate::abi::call_abi;

pub fn load_elf(elf_bytes: &[u8], bump: &mut Bump) -> Result<extern "C" fn() -> !, ()> {
    let elf = Elf::parse(elf_bytes).map_err(|_| ())?;

    if elf.is_64
        || elf.is_lib
        || elf.is_object_file()
        || !elf.little_endian
        || elf.header.e_type != ET_DYN
        || elf.interpreter.is_some()
    {
        return Err(());
    }

    // Find base address (lowest virtual address of PT_LOAD segments)
    let base_vaddr = elf
        .program_headers
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD)
        .map(|ph| ph.p_vaddr)
        .min()
        .ok_or(())?;

    // Determine total memory needed for all PT_LOAD segments
    let total_size = elf
        .program_headers
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD)
        .map(|ph| {
            let start = ph.p_vaddr;
            let end = ph.p_vaddr + ph.p_memsz;
            end - base_vaddr
        })
        .max()
        .unwrap_or(0) as usize;

    // Allocate one big block from the bump heap
    let layout = Layout::from_size_align(total_size, 0x1000).map_err(|_| ())?;
    let base_ptr = bump.alloc_layout(layout).as_ptr();

    for ph in &elf.program_headers {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let file_offset = ph.p_offset as usize;
        let file_size = ph.p_filesz as usize;
        let mem_size = ph.p_memsz as usize;
        let virt_offset = (ph.p_vaddr - base_vaddr) as usize;

        let src = &elf_bytes[file_offset..file_offset + file_size];
        let dst = unsafe { base_ptr.add(virt_offset) };

        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, file_size);
            if mem_size > file_size {
                core::ptr::write_bytes(dst.add(file_size), 0, mem_size - file_size);
            }
        }
    }

    // Patch `call_abi` symbol
    for sym in elf.syms.iter() {
        let name = elf.strtab.get_at(sym.st_name).ok_or(())?;
        if name == "call_abi" && sym.st_bind() == sym::STB_GLOBAL {
            let offset = (sym.st_value - base_vaddr) as usize;
            let ptr = unsafe { base_ptr.add(offset) as *mut usize };
            unsafe { *ptr = call_abi as usize };
        }
    }

    // Compute relocated entry point
    let relocated_entry = unsafe { base_ptr.add((elf.entry - base_vaddr) as usize) };
    Ok(unsafe { core::mem::transmute(relocated_entry) })
}
