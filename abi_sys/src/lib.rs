#![no_std]

extern crate alloc;

#[allow(unused)]
use embedded_graphics::{
    Pixel,
    geometry::Point,
    pixelcolor::{Rgb565, RgbColor},
};
pub use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

pub type EntryFn = fn();

#[unsafe(no_mangle)]
#[unsafe(link_section = ".user_reloc")]
pub static mut CALL_ABI_TABLE: [usize; CallAbiTable::COUNT] = [0; CallAbiTable::COUNT];

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum CallAbiTable {
    Print = 0,
    Sleep = 1,
    DrawIter = 2,
    GetKey = 3,
}

impl CallAbiTable {
    pub const COUNT: usize = 4;
}

pub type PrintAbi = extern "Rust" fn(msg: &str);

#[allow(unused)]
pub fn print(msg: &str) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::Print as usize];
        let f: PrintAbi = core::mem::transmute(ptr);
        f(msg);
    }
}

pub type SleepAbi = extern "Rust" fn(ms: u64);

#[allow(unused)]
pub fn sleep(ms: u64) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::Sleep as usize];
        let f: SleepAbi = core::mem::transmute(ptr);
        f(ms);
    }
}

pub type DrawIterAbi = extern "Rust" fn(pixels: &[Pixel<Rgb565>]);

#[allow(unused)]
pub fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::DrawIter as usize];
        let f: DrawIterAbi = core::mem::transmute(ptr);
        f(pixels);
    }
}

pub type GetKeyAbi = extern "Rust" fn() -> Option<KeyEvent>;

#[allow(unused)]
pub fn get_key() -> Option<KeyEvent> {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::GetKey as usize];
        let f: GetKeyAbi = core::mem::transmute(ptr);
        f()
    }
}
