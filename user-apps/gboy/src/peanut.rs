#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::{DISPLAY, GAME_ROM, RAM};

#[allow(unused)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use abi::{display::Pixel565, println};
use embedded_graphics::{Drawable, pixelcolor::Rgb565, prelude::Point};

pub const GBOY_WIDTH: usize = 160;
pub const GBOY_HEIGHT: usize = 144;

pub unsafe extern "C" fn gb_rom_read(_gb: *mut gb_s, addr: u32) -> u8 {
    unsafe { GAME_ROM.as_ref().unwrap()[addr as usize] }
}

pub unsafe extern "C" fn gb_cart_ram_read(_gb: *mut gb_s, addr: u32) -> u8 {
    unsafe { RAM[addr as usize] }
}

pub unsafe extern "C" fn gb_cart_ram_write(_gb: *mut gb_s, addr: u32, val: u8) {
    unsafe { RAM[addr as usize] = val }
}

pub unsafe extern "C" fn gb_error(_gb: *mut gb_s, err: gb_error_e, addr: u16) {
    let e = match err {
        0 => "UNKNOWN ERROR",
        1 => "INVALID OPCODE",
        2 => "INVALID READ",
        3 => "INVALID WRITE",
        4 => "HALT FOREVER",
        5 => "INVALID MAX",
        _ => unreachable!(),
    };

    println!("PeanutGB error: {}, addr: {}", e, addr);
}

const NUM_PALETTES: usize = 3;
const SHADES_PER_PALETTE: usize = 4;

const PALETTES: [[Rgb565; SHADES_PER_PALETTE]; NUM_PALETTES] = [
    [
        Rgb565::new(8, 24, 32),
        Rgb565::new(52, 104, 86),
        Rgb565::new(136, 192, 112),
        Rgb565::new(224, 248, 208),
    ], // BG
    [
        Rgb565::new(8, 24, 32),
        Rgb565::new(52, 104, 86),
        Rgb565::new(136, 192, 112),
        Rgb565::new(224, 248, 208),
    ], // OBJ0
    [
        Rgb565::new(8, 24, 32),
        Rgb565::new(52, 104, 86),
        Rgb565::new(136, 192, 112),
        Rgb565::new(224, 248, 208),
    ], // OBJ1
];

pub unsafe extern "C" fn lcd_draw_line(_gb: *mut gb_s, pixels: *const u8, line: u8) {
    if line < GBOY_HEIGHT as u8 {
        let pixels = unsafe { core::slice::from_raw_parts(pixels, GBOY_WIDTH) };
        let y = line as u16;

        for (x, &p) in pixels.iter().enumerate() {
            let palette_idx = ((p & 0xF0) >> 4) as usize;
            let shade_idx = (p & 0x03) as usize;

            let color = PALETTES
                .get(palette_idx)
                .and_then(|pal| pal.get(shade_idx))
                .copied()
                .unwrap_or(Rgb565::new(0, 0, 0));

            // let sx = (x as u16) * 2;
            // let sy = y * 2;

            // draw_color(color, sx, sy);
            // draw_color(color, sx + 1, sy);
            // draw_color(color, sx, sy + 1);
            // draw_color(color, sx + 1, sy + 1);
            //
            draw_color(color, x as u16, y as u16);
        }
    }
}

fn draw_color(color: Rgb565, x: u16, y: u16) {
    let mut pixel = Pixel565::default();
    pixel.0 = Point::new(x.into(), y.into());
    pixel.1 = color;

    unsafe {
        pixel.draw(&mut *DISPLAY).unwrap();
    }
}
