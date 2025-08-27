#![allow(static_mut_refs)]
use core::{alloc::Layout, ffi::c_void, ptr::NonNull, slice::from_raw_parts_mut};
use goblin::elf::{Elf, header::ET_DYN, program_header::PT_LOAD, sym};

// userland ram region defined in memory.x
unsafe extern "C" {
    static __userapp_start__: u8;
    static __userapp_end__: u8;
}

type EntryFn = extern "C" fn();

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
        .find(|s| elf.strtab.get_at(s.st_name).unwrap() == "call_abi_ptr")
        .expect("call_abi_ptr not found");

    // Virtual address inside user RAM
    let addr = call_abi_sym.st_value as *mut usize;

    // Patch it
    unsafe {
        core::ptr::write(addr, crate::abi::call_abi as usize);
    }

    Ok(unsafe { core::mem::transmute(elf.entry as u32) })
}
