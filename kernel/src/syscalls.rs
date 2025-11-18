use alloc::{string::ToString, vec::Vec};
use core::{ffi::c_char, ptr, sync::atomic::Ordering};
use embassy_rp::clocks::{RoscRng, clk_sys_freq};
use embassy_time::Instant;
use embedded_graphics::draw_target::DrawTarget;
use embedded_sdmmc::LfnBuffer;
use heapless::spsc::Queue;
use userlib_sys::{
    AUDIO_BUFFER_SAMPLES, Alloc, AudioBufferReady, CLayout, CPixel, Dealloc, DrawIter, FileLen,
    GenRand, GetMs, ListDir, Print, ReadFile, RngRequest, SendAudioBuffer, SleepMs, WriteFile,
    keyboard::*,
};

#[cfg(feature = "psram")]
use crate::heap::HEAP;

#[cfg(feature = "psram")]
use core::alloc::GlobalAlloc;

use crate::{
    audio::{AUDIO_BUFFER, AUDIO_BUFFER_READY},
    display::FRAMEBUFFER,
    framebuffer::FB_PAUSED,
    storage::{Dir, File, SDCARD},
};

const _: Alloc = alloc;
pub extern "C" fn alloc(layout: CLayout) -> *mut u8 {
    // SAFETY: caller guarantees layout is valid
    unsafe {
        #[cfg(feature = "psram")]
        {
            return HEAP.alloc(layout.into());
        }

        #[cfg(not(feature = "psram"))]
        {
            return alloc::alloc::alloc(layout.into());
        }
    }
}

const _: Dealloc = dealloc;
pub extern "C" fn dealloc(ptr: *mut u8, layout: CLayout) {
    // SAFETY: caller guarantees ptr and layout are valid
    #[cfg(feature = "psram")]
    {
        unsafe { HEAP.dealloc(ptr, layout.into()) }
    }

    #[cfg(not(feature = "psram"))]
    {
        unsafe { alloc::alloc::dealloc(ptr, layout.into()) }
    }
}

const _: Print = print;
pub extern "C" fn print(ptr: *const u8, len: usize) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };

    if let Ok(_msg) = core::str::from_utf8(slice) {
        #[cfg(feature = "defmt")]
        defmt::info!("print: {}", _msg);
    } else {
        #[cfg(feature = "defmt")]
        defmt::warn!("print: <invalid utf8>");
    }
}

const _: SleepMs = sleep;
pub extern "C" fn sleep(ms: u64) {
    let cycles_per_ms = clk_sys_freq() / 1000;
    let total_cycles = ms * cycles_per_ms as u64;

    for _ in 0..total_cycles {
        cortex_m::asm::nop();
    }
}

pub static mut MS_SINCE_LAUNCH: Option<Instant> = None;

const _: GetMs = get_ms;
pub extern "C" fn get_ms() -> u64 {
    Instant::now()
        .duration_since(unsafe { MS_SINCE_LAUNCH.unwrap() })
        .as_millis()
}

const _: DrawIter = draw_iter;
pub extern "C" fn draw_iter(cpixels: *const CPixel, len: usize) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let cpixels = unsafe { core::slice::from_raw_parts(cpixels, len) };

    let iter = cpixels.iter().copied().map(|c: CPixel| c.into());

    FB_PAUSED.store(true, Ordering::Release);
    unsafe { FRAMEBUFFER.as_mut().unwrap().draw_iter(iter).unwrap() }
    FB_PAUSED.store(false, Ordering::Release);
}

pub static mut KEY_CACHE: Queue<KeyEvent, 32> = Queue::new();

const _: GetKey = get_key;
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

unsafe fn copy_entry_to_user_buf(name: &[u8], dest: *mut c_char, max_str_len: usize) {
    if !dest.is_null() {
        let len = name.len().min(max_str_len - 1);
        unsafe {
            ptr::copy_nonoverlapping(name.as_ptr(), dest as *mut u8, len);
            *dest.add(len) = 0; // nul terminator
        }
    }
}

unsafe fn get_dir_entries(dir: &Dir, entries: &mut [*mut c_char], max_str_len: usize) -> usize {
    let mut b = [0; 25];
    let mut buf = LfnBuffer::new(&mut b);
    let mut i = 0;
    dir.iterate_dir_lfn(&mut buf, |entry, lfn_name| {
        if i < entries.len() {
            if let Some(name) = lfn_name {
                unsafe { copy_entry_to_user_buf(name.as_bytes(), entries[i], max_str_len) };
            } else {
                unsafe { copy_entry_to_user_buf(entry.name.base_name(), entries[i], max_str_len) };
            }
            i += 1;
        }
    })
    .unwrap();
    i
}

unsafe fn recurse_dir(
    dir: &Dir,
    dirs: &[&str],
    entries: &mut [*mut c_char],
    max_str_len: usize,
) -> usize {
    if dirs.is_empty() {
        return unsafe { get_dir_entries(dir, entries, max_str_len) };
    }

    let dir = dir.open_dir(dirs[0]).unwrap();
    unsafe { recurse_dir(&dir, &dirs[1..], entries, max_str_len) }
}

const _: ListDir = list_dir;
pub extern "C" fn list_dir(
    dir: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    files_len: usize,
    max_entry_str_len: usize,
) -> usize {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let files = unsafe { core::slice::from_raw_parts_mut(entries, files_len) };
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let dir = unsafe { core::str::from_raw_parts(dir, len) };
    let dirs: Vec<&str> = dir.split('/').collect();

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();

    let mut wrote = 0;
    sd.access_root_dir(|root| {
        if dirs[0] == "" && dirs.len() >= 2 {
            unsafe {
                if dir == "/" {
                    wrote = get_dir_entries(&root, files, max_entry_str_len);
                } else {
                    wrote = recurse_dir(&root, &dirs[1..], files, max_entry_str_len);
                }
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
        .expect("Failed to iterate dir");

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

    let mut components: [&str; 8] = [""; 8];
    let mut count = 0;
    for part in file.split('/') {
        if count >= components.len() {
            break;
        }
        components[count] = part;
        count += 1;
    }

    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let mut buf = unsafe { core::slice::from_raw_parts_mut(buf, buf_len) };

    let mut read = 0;

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    if !file.is_empty() {
        sd.access_root_dir(|root| {
            if let Ok(result) = recurse_file(&root, &components[1..count], |file| {
                file.seek_from_start(start_from as u32).unwrap_or(());
                file.read(&mut buf).unwrap()
            }) {
                read = result
            };
        });
    }
    read
}

const _: WriteFile = write_file;
pub extern "C" fn write_file(
    str: *const u8,
    len: usize,
    start_from: usize,
    buf: *const u8,
    buf_len: usize,
) {
    // SAFETY: caller guarantees str ptr is valid for `len` bytes
    let file = unsafe { core::str::from_raw_parts(str, len) };

    let mut components: [&str; 8] = [""; 8];
    let mut count = 0;
    for part in file.split('/') {
        if count >= components.len() {
            break;
        }
        components[count] = part;
        count += 1;
    }

    // SAFETY: caller guarantees buf ptr is valid for `buf_len` bytes
    let buf = unsafe { core::slice::from_raw_parts(buf, buf_len) };

    let mut guard = SDCARD.get().try_lock().expect("Failed to get sdcard");
    let sd = guard.as_mut().unwrap();
    if !file.is_empty() {
        sd.access_root_dir(|root| {
            recurse_file(&root, &components[1..count], |file| {
                file.seek_from_start(start_from as u32).unwrap();
                file.write(&buf).unwrap()
            })
            .unwrap_or(())
        });
    };
}

const _: FileLen = file_len;
pub extern "C" fn file_len(str: *const u8, len: usize) -> usize {
    // SAFETY: caller guarantees str ptr is valid for `len` bytes
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

const _: AudioBufferReady = audio_buffer_ready;
pub extern "C" fn audio_buffer_ready() -> bool {
    AUDIO_BUFFER_READY.load(Ordering::Acquire)
}

const _: SendAudioBuffer = send_audio_buffer;
pub extern "C" fn send_audio_buffer(ptr: *const u8, len: usize) {
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes
    let buf = unsafe { core::slice::from_raw_parts(ptr, len) };

    while !AUDIO_BUFFER_READY.load(Ordering::Acquire) {}

    if buf.len() == AUDIO_BUFFER_SAMPLES * 2 {
        AUDIO_BUFFER_READY.store(false, Ordering::Release);
        unsafe { AUDIO_BUFFER.copy_from_slice(buf) };
    } else {
        #[cfg(feature = "defmt")]
        defmt::warn!(
            "user audio stream was wrong size: {} should be {}",
            buf.len(),
            AUDIO_BUFFER_SAMPLES * 2
        )
    }
}
