#![allow(static_mut_refs)]
use crate::abi;
use abi_sys::{CallAbiTable, EntryFn};
use alloc::boxed::Box;
use core::{
    alloc::Layout,
    ffi::c_void,
    pin::Pin,
    ptr::NonNull,
    slice::from_raw_parts_mut,
    task::{Context, Poll},
};
use goblin::elf::{Elf, header::ET_DYN, program_header::PT_LOAD, sym};

// userland ram region defined in memory.x
unsafe extern "C" {
    static __userapp_start__: u8;
    static __userapp_end__: u8;
}

pub unsafe fn load_binary(bytes: &[u8]) -> Result<EntryFn, &str> {
    let elf = Elf::parse(&bytes).expect("Failed to parse ELF");

    if elf.is_64 || elf.is_lib || !elf.little_endian {
        return Err("Unsupported ELF type");
    }

    for ph in &elf.program_headers {
        if ph.p_type == PT_LOAD {
            let vaddr = ph.p_vaddr as usize;
            let memsz = ph.p_memsz as usize;
            let filesz = ph.p_filesz as usize;
            let offset = ph.p_offset as usize;

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

            unsafe {
                let dst = seg_start as *mut u8;
                let src = bytes.as_ptr().add(offset);

                // Copy initialized part
                core::ptr::copy_nonoverlapping(src, dst, filesz);

                // Zero BSS region (memsz - filesz)
                if memsz > filesz {
                    core::ptr::write_bytes(dst.add(filesz), 0, memsz - filesz);
                }
            }
        }
    }

    let call_abi_sym = elf
        .syms
        .iter()
        .find(|s| elf.strtab.get_at(s.st_name).unwrap() == "CALL_ABI_TABLE")
        .expect("syscall table not found");

    let table_base = call_abi_sym.st_value as *mut usize;

    let entries: &[(CallAbiTable, usize)] = &[
        (CallAbiTable::Print, abi::print as usize),
        (CallAbiTable::DrawIter, abi::draw_iter as usize),
        (CallAbiTable::GetKey, abi::get_key as usize),
    ];
    assert!(entries.len() == CallAbiTable::COUNT);

    for &(abi_idx, func_ptr) in entries {
        unsafe {
            table_base.add(abi_idx as usize).write(func_ptr);
        }
    }
    Ok(unsafe { core::mem::transmute(elf.entry as u32) })
}
