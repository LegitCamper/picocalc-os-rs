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
use core::{cell::LazyCell, ffi::c_void, mem::MaybeUninit, panic::PanicInfo};
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

const PEANUT_A: u8 = 0x01;
const PEANUT_B: u8 = 0x02;
const PEANUT_SELECT: u8 = 0x04;
const PEANUT_START: u8 = 0x08;
const PEANUT_RIGHT: u8 = 0x10;
const PEANUT_LEFT: u8 = 0x20;
const PEANUT_UP: u8 = 0x40;
const PEANUT_DOWN: u8 = 0x80;

const GAME: &'static str = "/games/gameboy/zelda.gb";

static mut GAME_ROM: Option<Vec<u8>> = None;

#[repr(C)]
struct Priv {}

pub fn main() {
    print!("Starting Gameboy app");

    let size = file_len(GAME);
    unsafe { GAME_ROM = Some(vec![0_u8; size]) };
    let read = read_file(GAME, 0, unsafe { GAME_ROM.as_mut().unwrap() });
    assert!(size == read);
    print!("Rom size: {}", read);

    let mut priv_ = MaybeUninit::<Priv>::uninit();
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
            match event.key {
                KeyCode::Esc => return,
                KeyCode::Tab => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_START;
                },
                KeyCode::Del => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_SELECT;
                },
                KeyCode::Enter => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_A;
                },
                KeyCode::Backspace => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_B;
                },
                KeyCode::JoyUp => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_UP;
                },
                KeyCode::JoyDown => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_DOWN;
                },
                KeyCode::JoyLeft => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_LEFT;
                },
                KeyCode::JoyRight => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !PEANUT_RIGHT;
                },
                _ => (),
            };
        };

        unsafe { gb_run_frame(gb.as_mut_ptr()) };
    }
}
