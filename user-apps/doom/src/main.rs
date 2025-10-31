#![no_std]
#![no_main]
#![allow(static_mut_refs)]
#![feature(c_variadic)]

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

mod doom;
use crate::doom::{DISPLAY, SCREEN_BUFFER, create, tick};
mod libc;

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
    let display = Display;
    unsafe { DISPLAY = Some(display) };

    unsafe { create(&SCREEN_BUFFER) };

    loop {
        tick();
    }
}
