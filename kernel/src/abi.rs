use core::{pin::Pin, time::Duration};

use abi_sys::{DrawIterAbi, GetKeyAbi, Pixel, PrintAbi, SleepAbi};
use alloc::boxed::Box;
use defmt::info;
use embassy_futures::block_on;
use embassy_time::Timer;
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
const _: SleepAbi = sleep;
const _: DrawIterAbi = draw_iter;
const _: GetKeyAbi = get_key;

pub extern "Rust" fn print(msg: &str) {
    defmt::info!("{:?}", msg);
}

pub extern "Rust" fn sleep(ticks: u64) {
    for _ in 0..ticks {
        for _ in 0..100 {
            cortex_m::asm::nop();
        }
    }
}

// TODO: maybe return result
pub extern "Rust" fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    loop {
        let fb = FRAMEBUFFER.get().try_lock();
        if let Ok(mut fb) = fb {
            fb.draw_iter(pixels.iter().copied()).unwrap();
            return;
        }
        sleep(1)
    }
}

pub extern "Rust" fn get_key() -> Option<KeyEvent> {
    unsafe { KEY_CACHE.dequeue() }
}
