use core::pin::Pin;

use abi_sys::{DrawIterAbi, GetKeyAbi, Pixel, PrintAbi};
use alloc::boxed::Box;
use defmt::info;
use embassy_futures::block_on;
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};
use shared::keyboard::KeyEvent;

use crate::{KEY_CACHE, display::FRAMEBUFFER};

// ensure the abi and the kernel fn signatures are the same
const _: PrintAbi = print;
const _: DrawIterAbi = draw_iter;
const _: GetKeyAbi = get_key;

pub extern "Rust" fn print(msg: &str) {
    defmt::info!("{:?}", msg);
}

pub extern "Rust" fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    let framebuffer = block_on(FRAMEBUFFER.lock());
    framebuffer
        .borrow_mut()
        .as_mut()
        .unwrap()
        .draw_iter(pixels.iter().copied())
        .unwrap();
}

pub extern "Rust" fn get_key() -> Option<KeyEvent> {
    defmt::info!("get key called");
    unsafe { KEY_CACHE.dequeue() }
}
