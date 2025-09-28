#![no_std]

extern crate alloc;

#[allow(unused)]
use embedded_graphics::{
    Pixel,
    geometry::Point,
    pixelcolor::{Rgb565, RgbColor},
};
use embedded_sdmmc::DirEntry;
pub use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};
use strum::{EnumCount, EnumIter};

pub type EntryFn = fn();

#[unsafe(no_mangle)]
#[unsafe(link_section = ".syscall_table")]
pub static mut CALL_ABI_TABLE: [usize; CallAbiTable::COUNT] = [0; CallAbiTable::COUNT];

#[repr(usize)]
#[derive(Clone, Copy, EnumIter, EnumCount)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum CallAbiTable {
    PrintString = 0,
    SleepMs = 1,
    LockDisplay = 2,
    DrawIter = 3,
    GetKey = 4,
    GenRand = 5,
    ListDir = 6,
    ReadFile = 7,
}

pub type PrintAbi = extern "C" fn(ptr: *const u8, len: usize);

#[allow(unused)]
pub fn print(msg: &str) {
    let f: PrintAbi =
        unsafe { core::mem::transmute(CALL_ABI_TABLE[CallAbiTable::PrintString as usize]) };
    f(msg.as_ptr(), msg.len());
}

pub type SleepAbi = extern "C" fn(ms: u64);

#[allow(unused)]
pub fn sleep(ms: u64) {
    let f: SleepAbi =
        unsafe { core::mem::transmute(CALL_ABI_TABLE[CallAbiTable::SleepMs as usize]) };
    f(ms);
}

pub type LockDisplay = extern "C" fn(lock: bool);

#[allow(unused)]
pub fn lock_display(lock: bool) {
    let f: LockDisplay =
        unsafe { core::mem::transmute(CALL_ABI_TABLE[CallAbiTable::LockDisplay as usize]) };
    f(lock);
}

pub type DrawIterAbi = extern "C" fn(ptr: *const Pixel<Rgb565>, len: usize);

#[allow(unused)]
pub fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    let f: DrawIterAbi =
        unsafe { core::mem::transmute(CALL_ABI_TABLE[CallAbiTable::DrawIter as usize]) };
    f(pixels.as_ptr(), pixels.len());
}

pub type GetKeyAbi = extern "C" fn() -> KeyEvent;

#[allow(unused)]
pub fn get_key() -> KeyEvent {
    let f: GetKeyAbi =
        unsafe { core::mem::transmute(CALL_ABI_TABLE[CallAbiTable::GetKey as usize]) };
    f()
}

#[repr(C)]
pub enum RngRequest {
    U32(u32),
    U64(u64),
    Bytes { ptr: *mut u8, len: usize },
}

pub type GenRand = extern "C" fn(req: &mut RngRequest);

#[allow(unused)]
pub fn gen_rand(req: &mut RngRequest) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::GenRand as usize];
        let f: GenRand = core::mem::transmute(ptr);
        f(req)
    }
}

pub type ListDir =
    extern "C" fn(str: *const u8, len: usize, files: *mut Option<DirEntry>, file_len: usize);

#[allow(unused)]
pub fn list_dir(path: &str, files: &mut [Option<DirEntry>]) {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::ListDir as usize];
        let f: ListDir = core::mem::transmute(ptr);
        f(path.as_ptr(), path.len(), files.as_mut_ptr(), files.len())
    }
}

pub type ReadFile = extern "C" fn(
    str: *const u8,
    len: usize,
    read_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize;

#[allow(unused)]
pub fn read_file(file: &str, read_from: usize, buf: &mut [u8]) -> usize {
    unsafe {
        let ptr = CALL_ABI_TABLE[CallAbiTable::ReadFile as usize];
        let f: ReadFile = core::mem::transmute(ptr);
        f(
            file.as_ptr(),
            file.len(),
            read_from,
            buf.as_mut_ptr(),
            buf.len(),
        )
    }
}
