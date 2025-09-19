use core::sync::atomic::Ordering;

use abi_sys::{DrawIterAbi, GetKeyAbi, LockDisplay, PrintAbi, SleepAbi};
use embassy_rp::clocks::clk_sys_freq;
use embedded_graphics::{Pixel, draw_target::DrawTarget, pixelcolor::Rgb565};
use shared::keyboard::KeyEvent;

use crate::{
    KEY_CACHE,
    display::{FB_PAUSED, FRAMEBUFFER},
};

// ensure the abi and the kernel fn signatures are the same
const _: PrintAbi = print;
const _: SleepAbi = sleep;
const _: LockDisplay = lock_display;
const _: DrawIterAbi = draw_iter;
const _: GetKeyAbi = get_key;

pub extern "Rust" fn print(msg: &str) {
    defmt::info!("{:?}", msg);
}

pub extern "Rust" fn sleep(ms: u64) {
    let cycles_per_ms = clk_sys_freq() / 1000;
    for _ in 0..ms {
        for _ in 0..cycles_per_ms {
            cortex_m::asm::nop();
        }
    }
}

pub extern "Rust" fn lock_display(lock: bool) {
    FB_PAUSED.store(lock, Ordering::Relaxed);
}

// TODO: maybe return result
pub extern "Rust" fn draw_iter(pixels: &[Pixel<Rgb565>]) {
    unsafe { FRAMEBUFFER.draw_iter(pixels.iter().copied()).unwrap() }
}

pub extern "Rust" fn get_key() -> Option<KeyEvent> {
    unsafe { KEY_CACHE.dequeue() }
}
