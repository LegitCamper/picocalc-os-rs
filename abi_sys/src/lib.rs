#![no_std]

pub use embedded_graphics::{
    Pixel,
    geometry::Point,
    pixelcolor::{Rgb565, RgbColor},
};
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

// Instead of extern, declare a static pointer in a dedicated section
#[unsafe(no_mangle)]
#[unsafe(link_section = ".user_reloc")]
#[allow(non_upper_case_globals)]
pub static mut call_abi_ptr: usize = 0;

// Helper to call it
pub unsafe fn call_abi(call: *const Syscall) {
    let f: extern "C" fn(*const Syscall) = unsafe { core::mem::transmute(call_abi_ptr) };
    f(call);
}

#[repr(C)]
pub enum Syscall {
    DrawIter {
        pixels: *const Pixel<Rgb565>,
        len: usize,
    },
    Print {
        msg: *const u8,
        len: usize,
    },
}
