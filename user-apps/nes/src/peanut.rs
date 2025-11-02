#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::DISPLAY;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use abi::{display::Pixel565, fs::read_file};
use embedded_graphics::{Drawable, pixelcolor::Rgb565, prelude::Point};

pub const NES_WIDTH: usize = 256;
pub const NES_HEIGHT: usize = 240;

pub unsafe extern "C" fn gb_rom_read(gb: *mut gb_s, addr: u32) -> u8 {
    let mut buf = [0_u8; 1];
    read_file("/games/nes/rom.nes", addr as usize, &mut buf);
    buf[0]
}

pub unsafe extern "C" fn gb_cart_ram_read(gb: *mut gb_s, addr: u32) -> u8 {
    0
}

pub unsafe extern "C" fn gb_cart_ram_write(gb: *mut gb_s, addr: u32, val: u8) {}

pub unsafe extern "C" fn gb_error(gb: *mut gb_s, err: gb_error_e, addr: u16) {}

pub unsafe extern "C" fn lcd_draw_line(gb: *mut gb_s, pixels: *const u8, line: u8) {
    unsafe {
        if line < NES_HEIGHT as u8 {
            let pixels = core::slice::from_raw_parts(pixels, NES_WIDTH);
            let mut x = 0;
            for p in pixels {
                let mut pixel = Pixel565::default();
                pixel.0 = Point::new(x, line.into());
                pixel.1 = Rgb565::new(*p, 10, 10);

                pixel.draw(&mut DISPLAY).unwrap();

                x += 1;
            }
        }
    }
}
