#![no_std]

use core::ffi::c_void;
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

// Instead of extern, declare a static pointer in a dedicated section
#[unsafe(no_mangle)]
#[unsafe(link_section = ".user_reloc")]
pub static mut call_abi_ptr: usize = 0;

// Helper to call it
pub unsafe fn call_abi(call: *const Syscall) {
    let f: extern "C" fn(*const Syscall) = unsafe { core::mem::transmute(call_abi_ptr) };
    f(call);
}

#[repr(C)]
pub enum Syscall {
    DrawPixel { x: u32, y: u32, color: u16 },
}
