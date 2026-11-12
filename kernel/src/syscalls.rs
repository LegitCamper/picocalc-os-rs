use crate::{
    audio,
    display::FRAMEBUFFER,
    storage::{Dir, File, SDCARD},
    user_memory,
    utils::wait,
};
use core::{
    alloc::Layout,
    ffi::c_char,
    ptr, slice,
    sync::atomic::{AtomicI32, Ordering},
};
use embassy_rp::clocks::RoscRng;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::{Rgb565, raw::RawU16},
    primitives::Rectangle,
};
use embedded_sdmmc::{LfnBuffer, Mode};
use userlib_sys::{
    Alloc, AudioBufferReady, Blit, CLayout, CPixel, Dealloc, DrawIter, FileLen, FillRect, GenRand,
    GetLastError, GetMs, ListDir, Print, ReadFile, ReconfigureAudioSampleRate, RngRequest,
    SendAudioBuffer, SleepMs, SysError, WriteFile, keyboard::*, memory::Region, split_path,
};

static LAST_ERROR: AtomicI32 = AtomicI32::new(0);
fn report<T>(result: Result<T, SysError>, fallback: T) -> T {
    match result {
        Ok(value) => {
            LAST_ERROR.store(0, Ordering::Release);
            value
        }
        Err(error) => {
            #[cfg(feature = "defmt")]
            defmt::warn!("Syscall rejected: {}", error as i32);
            LAST_ERROR.store(error as i32, Ordering::Release);
            fallback
        }
    }
}
const _: GetLastError = get_last_error;
pub extern "C" fn get_last_error() -> i32 {
    LAST_ERROR.load(Ordering::Acquire)
}

fn valid_buffer<T>(pointer: *const T, len: usize) -> bool {
    user_memory::valid(pointer, len)
}
unsafe fn bytes<'a>(pointer: *const u8, len: usize) -> Result<&'a [u8], SysError> {
    if !valid_buffer(pointer, len) {
        return Err(SysError::InvalidArgument);
    }
    if len == 0 {
        return Ok(&[]);
    }
    // SAFETY: checked full range in live app storage; caller supplies initialized bytes.
    Ok(unsafe { slice::from_raw_parts(pointer, len) })
}
unsafe fn path<'a>(pointer: *const u8, len: usize) -> Result<&'a str, SysError> {
    if len == 0 || len > 2048 {
        return Err(SysError::InvalidArgument);
    }
    // SAFETY: forwarded readable buffer contract; UTF-8 checked before making str.
    core::str::from_utf8(unsafe { bytes(pointer, len)? }).map_err(|_| SysError::InvalidArgument)
}

const _: Alloc = alloc;
pub unsafe extern "C" fn alloc(layout: CLayout) -> *mut u8 {
    report(
        Layout::try_from(layout)
            .map_err(|_| SysError::InvalidArgument)
            .and_then(user_memory::allocate),
        ptr::null_mut(),
    )
}
const _: Dealloc = dealloc;
pub unsafe extern "C" fn dealloc(pointer: *mut u8, layout: CLayout) {
    report(
        Layout::try_from(layout)
            .map_err(|_| SysError::InvalidArgument)
            .and_then(|layout| user_memory::deallocate(pointer, layout)),
        (),
    )
}
const _: Print = print;
pub unsafe extern "C" fn print(pointer: *const u8, len: usize) {
    // SAFETY: forwarded live-buffer contract.
    let result = unsafe { bytes(pointer, len) }.and_then(|buffer| {
        let _message = core::str::from_utf8(buffer).map_err(|_| SysError::InvalidArgument)?;
        #[cfg(feature = "defmt")]
        defmt::info!("print: {}", _message);
        Ok(())
    });
    report(result, ())
}
const _: SleepMs = sleep;
pub extern "C" fn sleep(ms: u64) {
    if let Some(duration) = Duration::try_from_millis(ms)
        && let Some(deadline) = Instant::now().checked_add(duration)
    {
        wait(Timer::at(deadline));
    } else {
        report(Err(SysError::InvalidArgument), ());
    }
}
pub static mut MS_SINCE_LAUNCH: Option<Instant> = None;
const _: GetMs = get_ms;
pub extern "C" fn get_ms() -> u64 {
    // SAFETY: only core1 writes before app entry and reads via this syscall.
    unsafe { MS_SINCE_LAUNCH }.map_or(0, |start| start.elapsed().as_millis())
}
const _: DrawIter = draw_iter;
pub unsafe extern "C" fn draw_iter(cpixels: *const CPixel, len: usize) {
    if !valid_buffer(cpixels, len) {
        report(Err(SysError::InvalidArgument), ());
        return;
    }
    if len == 0 {
        return;
    }
    // SAFETY: checked shape and caller's readable CPixel buffer contract.
    let pixels = unsafe { slice::from_raw_parts(cpixels, len) };
    if let Some(framebuffer) = wait(FRAMEBUFFER.lock()).as_mut() {
        let _ = framebuffer.draw_iter(pixels.iter().copied().map(Into::into));
    }
}
const _: FillRect = fill_rect;
pub extern "C" fn fill_rect(x: u16, y: u16, width: u16, height: u16, color: u16) {
    let area = Rectangle::new(
        Point::new(x as i32, y as i32),
        Size::new(width as u32, height as u32),
    );
    if let Some(framebuffer) = wait(FRAMEBUFFER.lock()).as_mut() {
        let _ = framebuffer.fill_solid(&area, Rgb565::from(RawU16::new(color)));
    }
}
const _: Blit = blit;
pub unsafe extern "C" fn blit(
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    colors: *const u16,
    len: usize,
) {
    if !valid_buffer(colors, len) || len > width as usize * height as usize {
        report(Err(SysError::InvalidArgument), ());
        return;
    }
    if len == 0 {
        return;
    }
    let area = Rectangle::new(
        Point::new(x as i32, y as i32),
        Size::new(width as u32, height as u32),
    );
    // SAFETY: checked shape and caller's readable storage. Short iterators are valid.
    let raw = unsafe { slice::from_raw_parts(colors, len) };
    if let Some(framebuffer) = wait(FRAMEBUFFER.lock()).as_mut() {
        let _ =
            framebuffer.fill_contiguous(&area, raw.iter().map(|&color| RawU16::new(color).into()));
    }
}
pub static KEY_CACHE: Channel<CriticalSectionRawMutex, KeyEvent, 32> = Channel::new();
const _: GetKey = get_key;
pub unsafe extern "C" fn get_key(output: *mut KeyEventC) {
    if !valid_buffer(output, 1) {
        report(Err(SysError::InvalidArgument), ());
        return;
    }
    let event = KEY_CACHE
        .try_receive()
        .unwrap_or(KeyEvent {
            key: KeyCode::Unknown(0),
            state: KeyState::Idle,
            mods: Modifiers::empty(),
        })
        .into();
    // SAFETY: output is a checked app-owned range; no references to it are retained.
    unsafe { output.write(event) };
    report(Ok(()), ())
}
const _: GenRand = gen_rand;
pub unsafe extern "C" fn gen_rand(request: *mut RngRequest) {
    if !valid_buffer(request, 1) {
        report(Err(SysError::InvalidArgument), ());
        return;
    }
    // SAFETY: repr(C) starts with a C enum tag; range and alignment checked first.
    let tag = unsafe { request.cast::<u32>().read() };
    if tag > 2 {
        report(Err(SysError::InvalidArgument), ());
        return;
    }
    let mut rng = RoscRng;
    // SAFETY: valid tag and mapped storage; raw pointers in Bytes admit any bit pattern.
    match unsafe { request.read() } {
        RngRequest::U32(_) => {
            // SAFETY: validated request storage; no borrowed references into it survive.
            unsafe { request.write(RngRequest::U32(rng.next_u32())) };
        }
        RngRequest::U64(_) => {
            // SAFETY: validated request storage; no borrowed references into it survive.
            unsafe { request.write(RngRequest::U64(rng.next_u64())) };
        }
        RngRequest::Bytes { ptr, len } => {
            if !valid_buffer(ptr, len)
                || (Region {
                    start: request as usize,
                    len: size_of::<RngRequest>(),
                })
                .overlaps(Region {
                    start: ptr as usize,
                    len,
                })
            {
                report(Err(SysError::InvalidArgument), ());
                return;
            }
            if len > 0 {
                // SAFETY: validated app storage disjoint from request; single app thread.
                rng.fill_bytes(unsafe { slice::from_raw_parts_mut(ptr, len) });
            }
        }
    }
    report(Ok(()), ())
}
fn fs_error<Error: core::fmt::Debug>(error: embedded_sdmmc::Error<Error>) -> SysError {
    use embedded_sdmmc::Error::*;
    match error {
        NotFound => SysError::NotFound,
        FilenameError(_) | InvalidOffset | OpenedDirAsFile | OpenedFileAsDir => {
            SysError::InvalidArgument
        }
        TooManyOpenDirs | TooManyOpenFiles | TooManyOpenVolumes => SysError::NoMemory,
        FileAlreadyOpen | DirAlreadyOpen | LockError => SysError::Busy,
        Unsupported => SysError::Unsupported,
        _ => SysError::Io,
    }
}
unsafe fn get_dir_entries(
    dir: &Dir,
    entries: &[*mut c_char],
    capacity: usize,
) -> Result<usize, SysError> {
    let mut storage = [0; 256];
    let mut lfn = LfnBuffer::new(&mut storage);
    let mut count = 0;
    let mut error = None;
    dir.iterate_dir_lfn(&mut lfn, |entry, long_name| {
        let short_name = crate::format!(13, "{}", entry.name);
        let name = long_name.unwrap_or(&short_name);
        if name == "." || name == ".." || entry.attributes.is_volume() {
            return;
        }
        if count == entries.len() || name.len() >= capacity {
            error = Some(SysError::BufferTooSmall);
            return;
        }
        // SAFETY: caller supplies each writable buffer; list_dir checked shapes.
        unsafe {
            ptr::copy_nonoverlapping(name.as_ptr(), entries[count].cast(), name.len());
            *entries[count].add(name.len()) = 0;
        }
        count += 1;
    })
    .map_err(fs_error)?;
    error.map_or(Ok(count), Err)
}
unsafe fn recurse_dir(
    dir: &Dir,
    components: &[&str],
    entries: &[*mut c_char],
    capacity: usize,
) -> Result<usize, SysError> {
    let Some((component, rest)) = components.split_first() else {
        // SAFETY: forwarded validated entry buffers.
        return unsafe { get_dir_entries(dir, entries, capacity) };
    };
    let child = dir.open_dir(*component).map_err(fs_error)?;
    // SAFETY: forwarded entry contracts; component count bounds recursion.
    unsafe { recurse_dir(&child, rest, entries, capacity) }
}
const _: ListDir = list_dir;
pub unsafe extern "C" fn list_dir(
    directory: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    count: usize,
    capacity: usize,
) -> usize {
    let result = (|| {
        // SAFETY: forwarded readable path contract; parser checks UTF-8.
        let directory = unsafe { path(directory, len)? };
        let (components, used) = split_path(directory).ok_or(SysError::InvalidArgument)?;
        if capacity == 0 || count > 256 || !valid_buffer(entries, count) {
            return Err(SysError::InvalidArgument);
        }
        let entries = if count == 0 {
            &[]
        } else {
            // SAFETY: full pointer array is in live app storage; no mutable borrow is needed.
            unsafe { slice::from_raw_parts(entries, count) }
        };
        let pointer_array = Region {
            start: entries.as_ptr() as usize,
            len: size_of_val(entries),
        };
        let path_region = Region {
            start: directory.as_ptr() as usize,
            len: directory.len(),
        };
        for (index, &entry) in entries.iter().enumerate() {
            let output = Region {
                start: entry as usize,
                len: capacity,
            };
            if !valid_buffer(entry, capacity)
                || output.overlaps(pointer_array)
                || output.overlaps(path_region)
                || entries[..index].iter().any(|&previous| {
                    output.overlaps(Region {
                        start: previous as usize,
                        len: capacity,
                    })
                })
            {
                return Err(SysError::InvalidArgument);
            }
        }
        let mut guard = SDCARD.get().try_lock().map_err(|_| SysError::Busy)?;
        let sd = guard.as_mut().ok_or(SysError::Busy)?;
        sd.access_root_dir(|root| {
            // SAFETY: shapes checked above; entry storage supplied by caller.
            unsafe { recurse_dir(&root, &components[..used], entries, capacity) }
        })
        .map_err(|_| SysError::Io)?
    })();
    report(result, 0)
}
fn recurse_file<T>(
    dir: &Dir,
    components: &[&str],
    mode: Mode,
    access: &mut impl FnMut(&mut File) -> Result<T, SysError>,
) -> Result<T, SysError> {
    let Some((component, rest)) = components.split_first() else {
        return Err(SysError::InvalidArgument);
    };
    if !rest.is_empty() {
        let child = dir.open_dir(*component).map_err(fs_error)?;
        return recurse_file(&child, rest, mode, access);
    }
    let mut storage = [0; 256];
    let mut lfn = LfnBuffer::new(&mut storage);
    let mut found = None;
    dir.iterate_dir_lfn(&mut lfn, |entry, long_name| {
        if long_name.is_some_and(|name| name == *component)
            || crate::format!(13, "{}", entry.name).eq_ignore_ascii_case(component)
        {
            found = Some(entry.name.clone());
        }
    })
    .map_err(fs_error)?;
    let mut file = dir
        .open_file_in_dir(found.ok_or(SysError::NotFound)?, mode)
        .map_err(fs_error)?;
    let result = access(&mut file);
    let close = file.close().map_err(fs_error);
    let value = result?;
    close?;
    Ok(value)
}
fn access_file<T>(
    file: &str,
    mode: Mode,
    mut access: impl FnMut(&mut File) -> Result<T, SysError>,
) -> Result<T, SysError> {
    let (components, count) = split_path(file).ok_or(SysError::InvalidArgument)?;
    if count == 0 {
        return Err(SysError::InvalidArgument);
    }
    let mut guard = SDCARD.get().try_lock().map_err(|_| SysError::Busy)?;
    let sd = guard.as_mut().ok_or(SysError::Busy)?;
    sd.access_root_dir(|root| recurse_file(&root, &components[..count], mode, &mut access))
        .map_err(|_| SysError::Io)?
}
const _: ReadFile = read_file;
pub unsafe extern "C" fn read_file(
    file: *const u8,
    len: usize,
    offset: usize,
    buffer: *mut u8,
    buffer_len: usize,
) -> usize {
    let result = (|| {
        // SAFETY: forwarded readable path contract.
        let file = unsafe { path(file, len)? };
        if !valid_buffer(buffer, buffer_len)
            || (Region {
                start: file.as_ptr() as usize,
                len: file.len(),
            })
            .overlaps(Region {
                start: buffer as usize,
                len: buffer_len,
            })
        {
            return Err(SysError::InvalidArgument);
        }
        let buffer = if buffer_len == 0 {
            &mut []
        } else {
            // SAFETY: caller supplies exclusive writable storage; checked shape.
            unsafe { slice::from_raw_parts_mut(buffer, buffer_len) }
        };
        let offset = u32::try_from(offset).map_err(|_| SysError::InvalidArgument)?;
        access_file(file, Mode::ReadOnly, |file| {
            file.seek_from_start(offset).map_err(fs_error)?;
            file.read(buffer).map_err(fs_error)
        })
    })();
    report(result, 0)
}
const _: WriteFile = write_file;
pub unsafe extern "C" fn write_file(
    file: *const u8,
    len: usize,
    offset: usize,
    buffer: *const u8,
    buffer_len: usize,
) {
    let result = (|| {
        // SAFETY: caller supplies both readable path and payload storage.
        let file = unsafe { path(file, len)? };
        let buffer = unsafe { bytes(buffer, buffer_len)? };
        let offset = u32::try_from(offset).map_err(|_| SysError::InvalidArgument)?;
        access_file(file, Mode::ReadWriteAppend, |file| {
            file.seek_from_start(offset).map_err(fs_error)?;
            file.write(buffer).map_err(fs_error)?;
            file.flush().map_err(fs_error)
        })
    })();
    report(result, ())
}
const _: FileLen = file_len;
pub unsafe extern "C" fn file_len(file: *const u8, len: usize) -> usize {
    // SAFETY: caller supplies readable path storage; errors are returned.
    let result = unsafe { path(file, len) }
        .and_then(|file| access_file(file, Mode::ReadOnly, |file| Ok(file.length() as usize)));
    report(result, 0)
}
const _: ReconfigureAudioSampleRate = reconfigure_audio_sample_rate;
pub extern "C" fn reconfigure_audio_sample_rate(sample_rate: u32) {
    report(audio::configure(sample_rate), ())
}
const _: AudioBufferReady = audio_buffer_ready;
pub extern "C" fn audio_buffer_ready() -> bool {
    audio::ready()
}
const _: SendAudioBuffer = send_audio_buffer;
pub unsafe extern "C" fn send_audio_buffer(pointer: *const u8, len: usize) {
    // SAFETY: caller's readable buffer contract; submit copies before return.
    report(unsafe { bytes(pointer, len) }.and_then(audio::submit), ())
}
