#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::Display,
    fs::{file_len, read_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::vec;
use core::panic::PanicInfo;
use embedded_graphics::{pixelcolor::Rgb565, prelude::Point};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print!("Starting Doom app");
    let mut display = Display;
}
