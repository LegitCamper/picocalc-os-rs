#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unsafe_op_in_unsafe_fn)]

use crate::RAM;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use userlib::{
    display::{SCREEN_HEIGHT, SCREEN_WIDTH},
    println,
};

pub const GBOY_WIDTH: usize = 160;
pub const GBOY_HEIGHT: usize = 144;

// centered on the 320x320 panel
const X_OFF: usize = (SCREEN_WIDTH - GBOY_WIDTH) / 2;
const Y_OFF: usize = (SCREEN_HEIGHT - GBOY_HEIGHT) / 2;
const _: () = assert!(X_OFF + GBOY_WIDTH <= SCREEN_WIDTH);
const _: () = assert!(Y_OFF + GBOY_HEIGHT <= SCREEN_HEIGHT);

/// Rom bytes, cached as a slice so the per-byte read callback is a single
/// bounds compare instead of an `Option` unwrap + panic path.
pub static mut ROM: &[u8] = &[];

pub unsafe extern "C" fn gb_rom_read(_gb: *mut gb_s, addr: u32) -> u8 {
    unsafe { ROM.get(addr as usize).copied().unwrap_or(0xFF) }
}

pub unsafe extern "C" fn gb_cart_ram_read(_gb: *mut gb_s, addr: u32) -> u8 {
    unsafe { RAM.get(addr as usize).copied().unwrap_or(0xFF) }
}

pub unsafe extern "C" fn gb_cart_ram_write(_gb: *mut gb_s, addr: u32, val: u8) {
    if let Some(slot) = unsafe { RAM.get_mut(addr as usize) } {
        *slot = val;
    }
}

pub unsafe extern "C" fn gb_error(_gb: *mut gb_s, err: gb_error_e, addr: u16) {
    let e = match err {
        0 => "UNKNOWN ERROR",
        1 => "INVALID OPCODE",
        2 => "INVALID READ",
        3 => "INVALID WRITE",
        4 => "HALT FOREVER",
        5 => "INVALID MAX",
        _ => "BAD ERROR CODE",
    };

    println!("PeanutGB error: {}, addr: {}", e, addr);
}

/// Pack to the panel's native Rgb565 word once, at compile time.
/// `Rgb565::new` takes pre-scaled 5/6/5 channels and silently masks wider
/// values, which is why the old 8-bit palette rendered wrong.
const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0x1F) << 11) | ((g as u16 & 0x3F) << 5) | (b as u16 & 0x1F)
}

/// Classic DMG green shades, darkest first.
const PALETTE: [u16; 4] = [
    rgb565(0x08 >> 3, 0x18 >> 2, 0x20 >> 3),
    rgb565(0x34 >> 3, 0x68 >> 2, 0x56 >> 3),
    rgb565(0x88 >> 3, 0xC0 >> 2, 0x70 >> 3),
    rgb565(0xE0 >> 3, 0xF8 >> 2, 0xD0 >> 3),
];

pub unsafe extern "C" fn lcd_draw_line(_gb: *mut gb_s, pixels: *const u8, line: u8) {
    if line as usize >= GBOY_HEIGHT {
        return;
    }

    // SAFETY: Peanut-GB always hands us a full LCD_WIDTH scanline
    let src = unsafe { core::slice::from_raw_parts(pixels, GBOY_WIDTH) };

    let mut row = [0_u16; GBOY_WIDTH];
    for (dst, &p) in row.iter_mut().zip(src) {
        *dst = PALETTE[(p & 0x03) as usize];
    }

    // one syscall per scanline instead of one per pixel
    unsafe {
        (*crate::DISPLAY).blit(
            X_OFF as u16,
            (Y_OFF + line as usize) as u16,
            GBOY_WIDTH as u16,
            1,
            &row,
        )
    };
}
