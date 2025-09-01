#![no_std]

extern crate alloc;
use alloc::boxed::Box;

use core::pin::Pin;
pub use embedded_graphics::{
    Pixel,
    geometry::Point,
    pixelcolor::{Rgb565, RgbColor},
};
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

pub type EntryFn = fn() -> Pin<Box<dyn Future<Output = ()>>>;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".userapp")]
pub static mut CALL_ABI_TABLE: [usize; CallAbiTable::COUNT] = [0; CallAbiTable::COUNT];

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum CallAbiTable {
    Print = 0,
    DrawIter = 1,
    GetKey = 2,
}

impl CallAbiTable {
    pub const COUNT: usize = 3;
}

pub type PrintAbi = extern "Rust" fn(msg: &str);

pub fn print(msg: &str) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::Print as usize];
        let f: PrintAbi = core::mem::transmute(ptr);
        f(msg);
    }
}

pub type DrawIterAbi = extern "Rust" fn(pixels: &[Pixel<Rgb565>]);

pub fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::DrawIter as usize];
        let f: DrawIterAbi = core::mem::transmute(ptr);
        f(pixels);
    }
}

pub type GetKeyAbi = extern "Rust" fn() -> Option<KeyEvent>;

pub fn get_key() -> Option<KeyEvent> {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::GetKey as usize];
        let f: GetKeyAbi = core::mem::transmute(ptr);
        f()
    }
}
