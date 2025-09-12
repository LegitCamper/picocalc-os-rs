use core::pin::Pin;

use abi_sys::{DrawIterAbi, GetKeyAbi, Pixel, PrintAbi};
use alloc::boxed::Box;
use defmt::info;
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};
use shared::keyboard::KeyEvent;

use crate::{KEY_CACHE, display::framebuffer_mut};

// ensure the abi and the kernel fn signatures are the same
const _: PrintAbi = print;
const _: DrawIterAbi = draw_iter;
const _: GetKeyAbi = get_key;

pub extern "Rust" fn print(msg: &str) {
    defmt::info!("{:?}", msg);
}

// TODO: maybe return result
pub extern "Rust" fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    let fb = framebuffer_mut();
    fb.draw_iter(pixels.iter().copied()).unwrap();
}

pub extern "Rust" fn get_key() -> Option<KeyEvent> {
    unsafe { KEY_CACHE.dequeue() }
}
