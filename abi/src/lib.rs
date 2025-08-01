#![no_std]

use core::ffi::c_void;
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

unsafe extern "C" {
    fn call_abi(call: *const Syscall);
}

#[repr(C)]
pub enum Syscall {
    DrawPixel { x: u32, y: u32, color: u16 },
}
