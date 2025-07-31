#![no_std]

use core::ffi::c_void;
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

#[repr(C)]
pub enum Syscall {
    DrawPixel { x: u32, y: u32, color: u16 },
}
