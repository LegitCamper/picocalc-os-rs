use abi_sys::{
    CPixel, DrawIterAbi, FileLen, GenRand, ListDir, LockDisplay, PrintAbi, ReadFile, RngRequest,
    SleepAbi, keyboard::*,
};
use alloc::{string::ToString, vec::Vec};
use core::sync::atomic::Ordering;
use embassy_rp::clocks::{RoscRng, clk_sys_freq};
use embedded_graphics::draw_target::DrawTarget;
use embedded_sdmmc::{DirEntry, LfnBuffer};
use heapless::spsc::Queue;

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
pub extern "C" fn draw_iter(cpixels: *const CPixel, len: usize) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let cpixels = unsafe { core::slice::from_raw_parts(cpixels, len) };

    let iter = cpixels.iter().copied().map(|c: CPixel| c.into());
    unsafe { FRAMEBUFFER.draw_iter(iter).unwrap() }
}

pub static mut KEY_CACHE: Queue<KeyEvent, 32> = Queue::new();

const _: GetKeyAbi = get_key;
pub extern "C" fn get_key() -> KeyEventC {
    if let Some(event) = unsafe { KEY_CACHE.dequeue() } {
        event.into()
    } else {
        KeyEvent {
            key: KeyCode::Unknown(0),
            state: KeyState::Idle,
            mods: Modifiers::empty(),
        }
        .into()
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

fn get_dir_entries(dir: &Dir, files: &mut [Option<DirEntry>]) -> usize {
    let mut i = 0;
    dir.iterate_dir(|entry| {
        if i < files.len() {
            files[i] = Some(entry.clone());
            i += 1;
        }
    })
    .unwrap();
    i
}

fn recurse_dir(dir: &Dir, dirs: &[&str], files: &mut [Option<DirEntry>]) -> usize {
    if dirs.is_empty() {
        return get_dir_entries(dir, files);
    }

    let dir = dir.open_dir(dirs[0]).unwrap();
    recurse_dir(&dir, &dirs[1..], files)
}

const _: ListDir = list_dir;
pub extern "C" fn list_dir(
    dir: *const u8,
    len: usize,
    files: *mut Option<DirEntry>,
    files_len: usize,
) -> usize {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let files = unsafe { core::slice::from_raw_parts_mut(files, files_len) };
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let dir = unsafe { core::str::from_raw_parts(dir, len) };
    let dirs: Vec<&str> = dir.split('/').collect();

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();

    let mut wrote = 0;
    sd.access_root_dir(|root| {
        if dirs[0] == "" && dirs.len() >= 2 {
            if dir == "/" {
                wrote = get_dir_entries(&root, files);
            } else {
                wrote = recurse_dir(&root, &dirs[1..], files);
            }
        }
    });
    wrote
}

fn recurse_file<T>(
    dir: &Dir,
    dirs: &[&str],
    mut access: impl FnMut(&mut File) -> T,
) -> Result<T, ()> {
    if dirs.len() == 1 {
        let mut b = [0_u8; 50];
        let mut buf = LfnBuffer::new(&mut b);
        let mut short_name = None;
        dir.iterate_dir_lfn(&mut buf, |entry, name| {
            if let Some(name) = name {
                if name == dirs[0] || entry.name.to_string().as_str() == dirs[0] {
                    short_name = Some(entry.name.clone());
                }
            }
        })
        .unwrap();
        if let Some(name) = short_name {
            let mut file = dir
                .open_file_in_dir(name, embedded_sdmmc::Mode::ReadWriteAppend)
                .map_err(|_| ())?;
            return Ok(access(&mut file));
        }
        return Err(());
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
    let file = unsafe { core::str::from_raw_parts(str, len) };
    let file: Vec<&str> = file.split('/').collect();
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let mut buf = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };

    let mut read = 0;

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    if !file.is_empty() {
        sd.access_root_dir(|root| {
            if let Ok(result) = recurse_file(&root, &file[1..], |file| {
                file.seek_from_start(start_from as u32).unwrap();
                file.read(&mut buf).unwrap()
            }) {
                read = result
            };
        });
    }
    read
}

const _: FileLen = file_len;
pub extern "C" fn file_len(str: *const u8, len: usize) -> usize {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let file = unsafe { core::str::from_raw_parts(str, len) };
    let file: Vec<&str> = file.split('/').collect();

    let mut len = 0;

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    if !file.is_empty() {
        sd.access_root_dir(|root| {
            if let Ok(result) = recurse_file(&root, &file[1..], |file| file.length()) {
                len = result
            }
        });
    }
    len as usize
}
