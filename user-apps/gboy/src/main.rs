#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use abi::{
    display::Display,
    fs::{file_len, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    print,
};
use alloc::{vec, vec::Vec};
use core::{ffi::c_void, mem::MaybeUninit, panic::PanicInfo};

mod peanut;
use peanut::gb_run_frame;

use crate::peanut::{
    JOYPAD_A, JOYPAD_B, JOYPAD_DOWN, JOYPAD_LEFT, JOYPAD_RIGHT, JOYPAD_SELECT, JOYPAD_START,
    JOYPAD_UP, gb_cart_ram_read, gb_cart_ram_write, gb_error, gb_init, gb_init_lcd, gb_rom_read,
    gb_s, lcd_draw_line,
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

    unsafe {
        gb_init_lcd(gb.as_mut_ptr(), Some(lcd_draw_line));

        // enable frame skip
        gb.assume_init().direct.set_frame_skip(true);
    };

    loop {
        let event = get_key();
        let keycode = match event.key {
            KeyCode::Esc => break,
            KeyCode::Tab => Some(JOYPAD_START),
            KeyCode::Del => Some(JOYPAD_SELECT),
            KeyCode::Enter => Some(JOYPAD_A),
            KeyCode::Backspace => Some(JOYPAD_B),
            KeyCode::JoyUp => Some(JOYPAD_UP),
            KeyCode::JoyDown => Some(JOYPAD_DOWN),
            KeyCode::JoyLeft => Some(JOYPAD_LEFT),
            KeyCode::JoyRight => Some(JOYPAD_RIGHT),
            _ => None,
        };

        if let Some(keycode) = keycode {
            match event.state {
                KeyState::Pressed => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad &= !keycode as u8
                },
                KeyState::Released => unsafe {
                    (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad |= keycode as u8
                },
                _ => (),
            }
        }

        unsafe { gb_run_frame(gb.as_mut_ptr()) };
    }
}
