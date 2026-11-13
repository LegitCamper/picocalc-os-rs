#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use alloc::{vec, vec::Vec};
use core::{cell::LazyCell, mem::MaybeUninit, panic::PanicInfo};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::RgbColor,
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};
use userlib::{
    display::Display,
    format,
    fs::{Entries, file_len, list_dir, read_file, write_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    println, sleep,
};

mod peanut;
use peanut::gb_run_frame;

use crate::peanut::{
    JOYPAD_A, JOYPAD_B, JOYPAD_DOWN, JOYPAD_LEFT, JOYPAD_RIGHT, JOYPAD_SELECT, JOYPAD_START,
    JOYPAD_UP, ROM, gb_cart_ram_read, gb_cart_ram_write, gb_error, gb_get_rom_name,
    gb_get_save_size, gb_init, gb_init_lcd, gb_reset, gb_rom_read, gb_s, gb_set_frame_skip,
    gb_set_joypad, lcd_draw_line,
};

static mut DISPLAY: LazyCell<Display> = LazyCell::new(|| Display::take().unwrap());

const RAM_SIZE: usize = 32 * 1024; // largest ram size is 32k
static mut RAM: [u8; RAM_SIZE] = [0; RAM_SIZE];

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

const GAME_PATH: &'static str = "/games/gameboy";
// 4194304 Hz / 70224 cycles per frame = 59.73 Hz
const FRAME_MS: u64 = 1000 / 60;

static mut GAME_ROM: Option<Vec<u8>> = None;

pub fn main() {
    println!("Starting Gameboy app");

    let mut entries = Entries::new();
    if let Err(error) = list_dir(GAME_PATH, &mut entries) {
        println!("Unable to list {}: {:?}", GAME_PATH, error);
        return;
    }

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

    let Some(selection) = selection else {
        return;
    };

    let file_name = format!("{}/{}", GAME_PATH, roms[selection]);
    let size = match file_len(&file_name) {
        Ok(size) if size != 0 => size,
        other => {
            println!("Unable to size {}: {:?}", file_name, other);
            return;
        }
    };
    unsafe { GAME_ROM = Some(vec![0_u8; size]) };
    // SAFETY: GAME_ROM was just assigned and this app is single-threaded.
    let read = read_file(&file_name, 0, unsafe { GAME_ROM.as_mut().unwrap() });
    if read != Ok(size) {
        println!("Short read of {}: {:?} of {}", file_name, read, size);
        return;
    }
    println!("Rom size: {}", size);

    // hand the rom to the hot read callback as a plain slice
    unsafe { ROM = GAME_ROM.as_ref().unwrap().as_slice() };

    let mut gb_uninit = MaybeUninit::<gb_s>::uninit();

    let init_status = unsafe {
        gb_init(
            gb_uninit.as_mut_ptr(),
            Some(gb_rom_read),
            Some(gb_cart_ram_read),
            Some(gb_cart_ram_write),
            Some(gb_error),
            core::ptr::null_mut(),
        )
    };
    println!("gb init status: {}", init_status);
    assert!(init_status == 0, "gb_init failed");

    // `assume_init()` copies the whole ~26KB `gb_s` by value; every such copy
    // was both a huge memcpy and a write to a temporary that got discarded
    // (which is why joypad input never reached the emulator).
    let gb: &mut gb_s = unsafe { gb_uninit.assume_init_mut() };

    unsafe {
        load_save(gb);
        gb_init_lcd(gb, Some(lcd_draw_line));
    };

    // joypad bits are active low; all released
    let mut joypad: u8 = 0xFF;
    unsafe { gb_set_joypad(gb, joypad) };

    // skip rendering every other frame when we fall behind
    let mut frame_skip = false;
    let mut next_frame = get_ms();

    loop {
        let event = get_key();
        let button = match event.key {
            KeyCode::Esc => {
                unsafe { write_save(gb) };
                break;
            }
            KeyCode::Char('r') => {
                unsafe { gb_reset(gb) };
                continue;
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
            match event.state {
                KeyState::Pressed | KeyState::Hold => joypad &= !button,
                KeyState::Released => joypad |= button,
                KeyState::Idle => {}
            }
            unsafe { gb_set_joypad(gb, joypad) };
        }

        unsafe { gb_run_frame(gb) };

        // pace to the real gameboy's ~59.7Hz; only skip frames when behind
        next_frame += FRAME_MS;
        let now = get_ms();
        if now < next_frame {
            if frame_skip {
                frame_skip = false;
                unsafe { gb_set_frame_skip(gb, false) };
            }
            sleep(next_frame - now);
        } else {
            if !frame_skip {
                frame_skip = true;
                unsafe { gb_set_frame_skip(gb, true) };
            }
            // too far behind to catch up; resync instead of spiralling
            if now - next_frame > 10 * FRAME_MS {
                next_frame = now;
            }
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
