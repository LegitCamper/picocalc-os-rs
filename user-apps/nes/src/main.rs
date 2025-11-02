#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use abi::{
    Rng,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    fs::{file_len, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::{vec, vec::Vec};
use core::{ffi::c_void, mem::MaybeUninit, panic::PanicInfo};
use embedded_graphics::{pixelcolor::Rgb565, prelude::RgbColor};

mod peanut;
use peanut::gb_run_frame;

use crate::peanut::{
    gb_cart_ram_read, gb_cart_ram_write, gb_error, gb_init, gb_init_lcd, gb_rom_read, gb_s,
    lcd_draw_line,
};

static mut DISPLAY: Display = Display;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

const GAME: &'static str = "/games/nes/game.nes";

#[repr(C)]
struct Priv {
    rom: Vec<u8>,
}

pub fn main() {
    print!("Starting Nes app");

    let size = file_len(GAME);
    let mut priv_ = MaybeUninit::<Priv>::uninit();
    // let read = unsafe {
    //     let priv_ptr = priv_.as_mut_ptr();
    //     (*priv_ptr).rom = Vec::with_capacity(size);

    //     read_file(GAME, 0, &mut (*priv_ptr).rom)
    // };

    // print!("read: {}, file size: {}", read, size);
    // assert!(read == size);

    let mut gb = MaybeUninit::<gb_s>::uninit();

    let init_status = unsafe {
        gb_init(
            gb.as_mut_ptr(),
            Some(gb_rom_read),
            Some(gb_cart_ram_read),
            Some(gb_cart_ram_write),
            Some(gb_error),
            priv_.as_mut_ptr() as *mut c_void,
        )
    };
    print!("gb init status: {}", init_status);

    unsafe { gb_init_lcd(gb.as_mut_ptr(), Some(lcd_draw_line)) };

    loop {
        let event = get_key();
        if event.state != KeyState::Idle {
            let key = match event.key {
                KeyCode::Esc => return,
                _ => (),
            };
        };

        unsafe { gb_run_frame(gb.as_mut_ptr()) };
    }
}
