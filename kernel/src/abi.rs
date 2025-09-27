use abi_sys::{
    DrawIterAbi, GenRand, GetKeyAbi, LockDisplay, Modifiers, PrintAbi, RngRequest, SleepAbi,
};
use core::sync::atomic::Ordering;
use embassy_rp::clocks::{RoscRng, clk_sys_freq};
use embedded_graphics::{Pixel, draw_target::DrawTarget, pixelcolor::Rgb565};
use shared::keyboard::KeyEvent;

use crate::{
    KEY_CACHE,
    display::{FB_PAUSED, FRAMEBUFFER},
};

const _: PrintAbi = print;
pub extern "C" fn print(ptr: *const u8, len: usize) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };

    if let Ok(msg) = core::str::from_utf8(slice) {
        defmt::info!("print: {}", msg);
    } else {
        defmt::warn!("print: <invalid utf8>");
    }
}

const _: SleepAbi = sleep;
pub extern "C" fn sleep(ms: u64) {
    let cycles_per_ms = clk_sys_freq() / 1000;
    let total_cycles = ms * cycles_per_ms as u64;

    for _ in 0..total_cycles {
        cortex_m::asm::nop();
    }
}

const _: LockDisplay = lock_display;
pub extern "C" fn lock_display(lock: bool) {
    FB_PAUSED.store(lock, Ordering::Relaxed);
}

const _: DrawIterAbi = draw_iter;
// TODO: maybe return result
pub extern "C" fn draw_iter(pixels: *const Pixel<Rgb565>, len: usize) {
    let pixels = unsafe { core::slice::from_raw_parts(pixels, len) };
    unsafe { FRAMEBUFFER.draw_iter(pixels.iter().copied()).unwrap() }
}

const _: GetKeyAbi = get_key;
pub extern "C" fn get_key() -> KeyEvent {
    if let Some(event) = unsafe { KEY_CACHE.dequeue() } {
        event
    } else {
        KeyEvent {
            key: abi_sys::KeyCode::Unknown(0),
            state: abi_sys::KeyState::Idle,
            mods: Modifiers::empty(),
        }
    }
}

const _: GenRand = gen_rand;
pub extern "C" fn gen_rand(req: &mut RngRequest) {
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
