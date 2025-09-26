use abi_sys::{DrawIterAbi, GetKeyAbi, LockDisplay, PrintAbi, RngRequest, SleepAbi};
use core::sync::atomic::Ordering;
use embassy_rp::clocks::{RoscRng, clk_sys_freq};
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
    let total_cycles = ms * cycles_per_ms as u64;

    for _ in 0..total_cycles {
        cortex_m::asm::nop();
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

pub extern "Rust" fn gen_rand(req: &mut RngRequest) {
    let mut rng = RoscRng;

    match req {
        RngRequest::U32(i) => *i = rng.next_u32(),
        RngRequest::U64(i) => *i = rng.next_u64(),
        RngRequest::Bytes { ptr, len } => {
            let slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(*ptr, *len) };
            rng.fill_bytes(slice);
        }
    }
}
