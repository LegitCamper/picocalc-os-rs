#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use abi::{
    display::Display,
    format,
    fs::{Entries, file_len, list_dir, read_file, write_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    print,
};
use alloc::{string::String, vec, vec::Vec};
use core::{cell::LazyCell, ffi::c_void, mem::MaybeUninit, panic::PanicInfo};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::RgbColor,
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};

mod peanut;
use peanut::gb_run_frame;

use crate::peanut::{
    JOYPAD_A, JOYPAD_B, JOYPAD_DOWN, JOYPAD_LEFT, JOYPAD_RIGHT, JOYPAD_SELECT, JOYPAD_START,
    JOYPAD_UP, gb_cart_ram_read, gb_cart_ram_write, gb_error, gb_get_rom_name, gb_get_save_size,
    gb_init, gb_init_lcd, gb_rom_read, gb_s, lcd_draw_line,
};

static mut DISPLAY: LazyCell<Display> = LazyCell::new(|| Display::take().unwrap());

const RAM_SIZE: usize = 32 * 1024; // largest ram size is 32k
static mut RAM: [u8; RAM_SIZE] = [0; RAM_SIZE];

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

const GAME_PATH: &'static str = "/games/gameboy";

static mut GAME_ROM: Option<Vec<u8>> = None;

#[repr(C)]
struct Priv {}

pub fn main() {
    print!("Starting Gameboy app");

    let mut entries = Entries::new();
    list_dir(GAME_PATH, &mut entries);

    let mut files = entries.entries();
    files.retain(|e| {
        let ext = e.extension().unwrap_or("");
        ext == "gb" || ext == "GB"
    });
    let mut roms = files.iter().map(|e| e.full_name()).collect::<Vec<&str>>();
    roms.sort();

    let selection = {
        let display = unsafe { &mut *DISPLAY };
        let mut selection_ui = SelectionUi::new(&roms);
        match selection_ui.run_selection_ui(display) {
            Ok(maybe_sel) => maybe_sel,
            Err(e) => match e {
                SelectionUiError::SelectionListEmpty => {
                    draw_text_center(
                        display,
                        &format!("No Roms were found in {}", GAME_PATH),
                        MonoTextStyle::new(&FONT_6X10, Rgb565::RED),
                    )
                    .expect("Display Error");
                    None
                }
                SelectionUiError::DisplayError(_) => panic!("Display Error"),
            },
        }
    };

    assert!(selection.is_some());

    let file_name = format!("{}/{}", GAME_PATH, roms[selection.unwrap()]);
    let size = file_len(&file_name);
    unsafe { GAME_ROM = Some(vec![0_u8; size]) };
    let read = read_file(&file_name, 0, unsafe { GAME_ROM.as_mut().unwrap() });
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
        load_save(&mut gb.assume_init());
    }

    unsafe {
        gb_init_lcd(gb.as_mut_ptr(), Some(lcd_draw_line));

        // enable frame skip
        gb.assume_init().direct.set_frame_skip(!true); // active low
    };

    loop {
        let event = get_key();
        let button = match event.key {
            KeyCode::Esc => {
                unsafe { write_save(&mut gb.assume_init()) };
                break;
            }
            KeyCode::Tab => JOYPAD_START as u8,
            KeyCode::Del => JOYPAD_SELECT as u8,
            KeyCode::Enter => JOYPAD_A as u8,
            KeyCode::Backspace => JOYPAD_B as u8,
            KeyCode::Up => JOYPAD_UP as u8,
            KeyCode::Down => JOYPAD_DOWN as u8,
            KeyCode::Left => JOYPAD_LEFT as u8,
            KeyCode::Right => JOYPAD_RIGHT as u8,
            _ => 0,
        };

        if button != 0 {
            let mut joypad = unsafe { (*gb.as_mut_ptr()).direct.__bindgen_anon_1.joypad };
            match event.state {
                KeyState::Pressed => joypad &= !button,
                KeyState::Released => joypad |= button,
                _ => {}
            }

            print!("joypad now: {:#010b}\n", joypad);
        }

        unsafe {
            gb_run_frame(gb.as_mut_ptr());
        }
    }
}

unsafe fn load_save(gb: &mut gb_s) {
    let mut buf = [0; 16];

    unsafe {
        gb_get_rom_name(gb, buf.as_mut_ptr());

        let save_size = gb_get_save_size(gb);

        if save_size > 0 {
            read_file(
                &format!(
                    "{}/saves/{}.sav",
                    GAME_PATH,
                    str::from_utf8(&buf).expect("bad rom name")
                ),
                0,
                &mut RAM,
            );
        }
    }
}

unsafe fn write_save(gb: &mut gb_s) {
    let mut buf = [0; 16];

    unsafe {
        gb_get_rom_name(gb, buf.as_mut_ptr());

        let save_size = gb_get_save_size(gb);

        if save_size > 0 {
            write_file(
                &format!(
                    "{}/saves/{}.sav",
                    GAME_PATH,
                    str::from_utf8(&buf).expect("bad rom name")
                ),
                0,
                &mut RAM,
            );
        }
    }
}
