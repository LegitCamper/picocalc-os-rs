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

// TODO: maybe return result
pub extern "Rust" fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    for _ in 0..10 {
        if let Some(mut framebuffer) = FRAMEBUFFER.try_lock().ok() {
            for _ in 0..10 {
                // kernel takes() framebuffer
                if let Some(framebuffer) = framebuffer.as_mut() {
                    framebuffer.draw_iter(pixels.iter().copied()).unwrap();
                }
                break;
            }
            break;
        }
        cortex_m::asm::nop();
    }
}

pub extern "Rust" fn get_key() -> Option<KeyEvent> {
    unsafe { KEY_CACHE.dequeue() }
}
