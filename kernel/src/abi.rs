use abi_sys::{
    DrawIterAbi, GenRand, GetKeyAbi, ListDir, LockDisplay, Modifiers, PrintAbi, ReadFile,
    RngRequest, SleepAbi,
};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use embassy_rp::clocks::{RoscRng, clk_sys_freq};
use embedded_graphics::{Pixel, draw_target::DrawTarget, pixelcolor::Rgb565};
use embedded_sdmmc::{DirEntry, ShortFileName};
use heapless::spsc::Queue;
use shared::keyboard::KeyEvent;

use crate::{
    display::{FB_PAUSED, FRAMEBUFFER},
    storage::{Dir, File, SDCARD},
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
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let pixels = unsafe { core::slice::from_raw_parts(pixels, len) };
    unsafe { FRAMEBUFFER.draw_iter(pixels.iter().copied()).unwrap() }
}

pub static mut KEY_CACHE: Queue<KeyEvent, 32> = Queue::new();

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
            // SAFETY: caller guarantees `ptr` is valid for `len` bytes
            let slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(*ptr, *len) };
            rng.fill_bytes(slice);
        }
    }
}

fn get_dir_entries(dir: &Dir, files: &mut [Option<DirEntry>]) {
    let mut files = files.iter_mut();

    dir.iterate_dir(|entry| {
        if let Some(f) = files.next() {
            *f = Some(entry.clone())
        }
    })
    .unwrap()
}

fn recurse_dir(dir: &Dir, dirs: &[&str], files: &mut [Option<DirEntry>]) {
    if dirs.is_empty() {
        get_dir_entries(dir, files);
        return;
    }

    let dir = dir.open_dir(dirs[0]).unwrap();
    recurse_dir(&dir, &dirs[1..], files);
}

const _: ListDir = list_dir;
pub extern "C" fn list_dir(
    dir: *const u8,
    len: usize,
    files: *mut Option<DirEntry>,
    files_len: usize,
) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let files = unsafe { core::slice::from_raw_parts_mut(files, files_len) };
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let dir = unsafe { core::str::from_raw_parts(dir, len) };
    let dirs: Vec<&str> = dir.split('/').collect();

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    sd.access_root_dir(|root| {
        if !dir.is_empty() {
            if dirs[0].is_empty() {
                get_dir_entries(&root, files);
            } else {
                recurse_dir(&root, &dirs[1..], files);
            }
        }
    });
}

fn recurse_file<T>(dir: &Dir, dirs: &[&str], mut access: impl FnMut(&mut File) -> T) -> T {
    if dirs.len() == 1 {
        let file_name = ShortFileName::create_from_str(dirs[0]).unwrap();

        let mut file = dir
            .open_file_in_dir(file_name, embedded_sdmmc::Mode::ReadWriteAppend)
            .unwrap();
        return access(&mut file);
    }

    let dir = dir.open_dir(dirs[0]).unwrap();
    recurse_file(&dir, &dirs[1..], access)
}

const _: ReadFile = read_file;
pub extern "C" fn read_file(
    str: *const u8,
    len: usize,
    start_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let mut buf = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let file = unsafe { core::str::from_raw_parts(str, len) };
    let file: Vec<&str> = file.split('/').collect();

    let mut res = 0;

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    if !file.is_empty() {
        if file[0].is_empty() {
        } else {
            sd.access_root_dir(|root| {
                res = recurse_file(&root, &file[1..], |file| {
                    file.seek_from_start(start_from as u32).unwrap();
                    file.read(&mut buf).unwrap()
                })
            });
        }
    }
    res
}
