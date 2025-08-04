#![no_std]

use core::ffi::c_void;
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

#[unsafe(no_mangle)]
pub unsafe fn call_abi(_call: *const Syscall) {}

#[repr(C)]
pub enum Syscall {
    DrawPixel { x: u32, y: u32, color: u16 },
}
