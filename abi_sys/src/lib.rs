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
    LockDisplay = 2,
    DrawIter = 3,
    GetKey = 4,
    GenRand = 5,
}

impl CallAbiTable {
    pub const COUNT: usize = 6;
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

pub type LockDisplay = extern "Rust" fn(lock: bool);

#[allow(unused)]
pub fn lock_display(lock: bool) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::LockDisplay as usize];
        let f: LockDisplay = core::mem::transmute(ptr);
        f(lock);
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

pub type GenRand = extern "Rust" fn(req: &mut RngRequest);

#[repr(C)]
pub enum RngRequest {
    U32(u32),
    U64(u64),
    Bytes { ptr: *mut u8, len: usize },
}

#[allow(unused)]
pub fn gen_rand(req: &mut RngRequest) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::GenRand as usize];
        let f: GenRand = core::mem::transmute(ptr);
        f(req)
    }
}
