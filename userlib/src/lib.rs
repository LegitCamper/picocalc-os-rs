#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;
mod display_buffer;
#[cfg(test)]
extern crate std;

pub use alloc::format;
#[cfg(target_os = "none")]
use core::alloc::{GlobalAlloc, Layout};
use rand_core::RngCore;
pub use userlib_sys::animation::FramePacer;
use userlib_sys::{RngRequest, keyboard::KeyEvent};
pub use userlib_sys::{SysError, keyboard};

#[cfg(target_os = "none")]
#[global_allocator]
static ALLOC: Alloc = Alloc;

#[cfg(target_os = "none")]
struct Alloc;

#[cfg(target_os = "none")]
// SAFETY: allocation and deallocation preserve the layouts supplied by Rust.
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Layout is valid and the loader initialized the syscall table.
        unsafe { userlib_sys::alloc(layout.into()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: GlobalAlloc callers supply this allocation's original layout.
        unsafe { userlib_sys::dealloc(ptr, layout.into()) };
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        $crate::_print(core::format_args!($($arg)*));
    }};
}

pub fn print(text: &str) {
    // SAFETY: text remains a valid immutable UTF-8 slice for this synchronous call.
    unsafe { userlib_sys::print(text.as_ptr(), text.len()) };
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;

    struct Writer;
    impl Write for Writer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            print(text);
            Ok(())
        }
    }
    let _ = Writer.write_fmt(args);
}

pub fn sleep(ms: u64) {
    userlib_sys::sleep(ms);
}

pub fn get_ms() -> u64 {
    userlib_sys::get_ms()
}

pub fn get_key() -> KeyEvent {
    userlib_sys::keyboard::get_key().into()
}

pub mod display {
    pub use crate::display_buffer::FrameBuffer;
    use core::sync::atomic::{AtomicBool, Ordering};

    use embedded_graphics::{
        Pixel,
        geometry::{Dimensions, Point},
        pixelcolor::{
            Rgb565,
            raw::{RawData, RawU16},
        },
        prelude::{DrawTarget, Size},
        primitives::Rectangle,
    };
    use userlib_sys::CPixel;

    pub const SCREEN_WIDTH: usize = 320;
    pub const SCREEN_HEIGHT: usize = 320;

    pub type Pixel565 = Pixel<Rgb565>;

    const BUF_SIZE: usize = 1024;
    static mut BUF: [CPixel; BUF_SIZE] = [CPixel::new(); BUF_SIZE];

    static DISPLAY_TAKEN: AtomicBool = AtomicBool::new(false);

    pub struct Display {
        _private: (),
    }

    impl Display {
        /// Only one instance of Display can be taken
        pub fn take() -> Option<Display> {
            if DISPLAY_TAKEN
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                Some(Self { _private: () })
            } else {
                None
            }
        }
    }

    impl Drop for Display {
        fn drop(&mut self) {
            DISPLAY_TAKEN.store(false, Ordering::Release);
        }
    }

    impl Display {
        /// Writes `width` x `height` row-major RGB565 pixels in one syscall.
        ///
        /// Cheaper than the `DrawTarget` path, which spends a syscall per
        /// `BUF_SIZE` pixels and ships coordinates for every pixel. Returns
        /// the number of pixels written, or `None` when `pixels` is shorter
        /// than `width * height`.
        pub fn blit(
            &mut self,
            x: u16,
            y: u16,
            width: u16,
            height: u16,
            pixels: &[u16],
        ) -> Option<usize> {
            let required = usize::from(width).checked_mul(usize::from(height))?;
            if required == 0 || pixels.len() < required {
                return None;
            }
            // SAFETY: pixels stays live and holds `required` colors for this call.
            unsafe { userlib_sys::blit(x, y, width, height, pixels.as_ptr(), required) };
            Some(required)
        }
    }

    impl Dimensions for Display {
        fn bounding_box(&self) -> Rectangle {
            Rectangle {
                top_left: Point { x: 0, y: 0 },
                size: Size {
                    width: SCREEN_WIDTH as u32,
                    height: SCREEN_HEIGHT as u32,
                },
            }
        }
    }

    impl DrawTarget for Display {
        type Color = Rgb565;
        type Error = ();

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            let mut count = 0;
            for p in pixels {
                // SAFETY: the unique Display owns BUF and count stays below BUF_SIZE.
                unsafe { BUF[count] = p.into() };
                count += 1;

                if count == BUF_SIZE {
                    // SAFETY: BUF contains count initialized pixels until the call returns.
                    unsafe { userlib_sys::draw_iter(BUF.as_ptr(), count) };
                    count = 0;
                }
            }

            if count > 0 {
                // SAFETY: BUF contains count initialized pixels until the call returns.
                unsafe { userlib_sys::draw_iter(BUF.as_ptr(), count) };
            }

            Ok(())
        }

        // One `fill_rect` syscall instead of decomposing the area into
        // individual pixels through `draw_iter` (the default impl this
        // overrides), e.g. for game backgrounds and UI panels.
        fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
            let drawable = area.intersection(&self.bounding_box());
            if drawable.size.width == 0 || drawable.size.height == 0 {
                return Ok(());
            }

            userlib_sys::fill_rect(
                drawable.top_left.x as u16,
                drawable.top_left.y as u16,
                drawable.size.width as u16,
                drawable.size.height as u16,
                RawU16::from(color).into_inner(),
            );

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use embedded_graphics::prelude::RgbColor;
        use std::sync::Mutex;

        static PIXELS: Mutex<alloc::vec::Vec<CPixel>> = Mutex::new(alloc::vec::Vec::new());
        static BLIT_CALLS: AtomicUsize = AtomicUsize::new(0);
        use core::sync::atomic::AtomicUsize;

        unsafe extern "C" fn capture_pixels(ptr: *const CPixel, length: usize) {
            // SAFETY: Display passes its initialized, exclusively owned pixel buffer.
            let pixels = unsafe { core::slice::from_raw_parts(ptr, length) };
            PIXELS.lock().unwrap().extend_from_slice(pixels);
        }

        unsafe extern "C" fn capture_blit(
            x: u16,
            y: u16,
            width: u16,
            height: u16,
            colors: *const u16,
            length: usize,
        ) {
            assert_eq!(length, usize::from(width) * usize::from(height));
            // SAFETY: Display passes a checked, live RGB565 slice for this synchronous call.
            let colors = unsafe { core::slice::from_raw_parts(colors, length) };
            PIXELS
                .lock()
                .unwrap()
                .extend(colors.iter().enumerate().map(|(index, &color)| CPixel {
                    x: i32::from(x) + (index % usize::from(width)) as i32,
                    y: i32::from(y) + (index / usize::from(width)) as i32,
                    color,
                }));
            BLIT_CALLS.fetch_add(1, Ordering::Relaxed);
        }

        #[test]
        fn short_contiguous_rows_are_drawn_and_display_can_be_retaken() {
            // SAFETY: this is the only test invoking draw syscalls in this test process.
            unsafe {
                userlib_sys::SYS_CALL_TABLE[userlib_sys::SyscallTable::DrawIter as usize] =
                    capture_pixels as *const () as usize;
                userlib_sys::SYS_CALL_TABLE[userlib_sys::SyscallTable::Blit as usize] =
                    capture_blit as *const () as usize;
            }
            let mut display = Display::take().unwrap();
            assert!(Display::take().is_none());
            let area = Rectangle::new(Point::new(0, 0), Size::new(3, 2));
            display.fill_contiguous(&area, [Rgb565::RED; 4]).unwrap();
            let pixels = PIXELS.lock().unwrap();
            assert_eq!(pixels.len(), 4);
            assert_eq!((pixels[3].x, pixels[3].y), (0, 1));
            drop(pixels);

            PIXELS.lock().unwrap().clear();
            let mut canvas = FrameBuffer::new(Size::new(4, 4)).unwrap();
            let snake = Rectangle::new(Point::new(1, 1), Size::new(2, 1));
            canvas.fill_solid(&snake, Rgb565::GREEN).unwrap();
            canvas.present(&mut display, Point::new(10, 20)).unwrap();
            PIXELS.lock().unwrap().clear();
            canvas.fill_solid(&snake, Rgb565::BLACK).unwrap();
            assert!(PIXELS.lock().unwrap().is_empty());
            canvas.fill_solid(&snake, Rgb565::GREEN).unwrap();
            assert!(PIXELS.lock().unwrap().is_empty());
            assert!(
                canvas
                    .present(&mut display, Point::new(i32::MAX, 0))
                    .is_err()
            );
            assert_eq!(BLIT_CALLS.load(Ordering::Relaxed), 1);
            canvas.present(&mut display, Point::new(10, 20)).unwrap();
            let pixels = PIXELS.lock().unwrap();
            assert_eq!(pixels.len(), 4);
            assert_eq!((pixels[1].x, pixels[1].y), (11, 21));
            assert_eq!(pixels[1].color, RawU16::from(Rgb565::GREEN).into_inner());
            assert_eq!(pixels[2].color, RawU16::from(Rgb565::GREEN).into_inner());
            drop(pixels);
            canvas.present(&mut display, Point::new(10, 20)).unwrap();
            assert_eq!(BLIT_CALLS.load(Ordering::Relaxed), 2);
            assert!(display.blit(0, 0, u16::MAX, u16::MAX, &[]).is_none());
            assert!(display.blit(0, 0, 3, 2, &[0; 1]).is_none());
            assert_eq!(BLIT_CALLS.load(Ordering::Relaxed), 2);
            drop(display);
            assert!(Display::take().is_some());
        }
    }
}

fn gen_rand(req: &mut RngRequest) {
    // SAFETY: req and any byte slice in it remain exclusively borrowed during the call.
    unsafe { userlib_sys::gen_rand(req) };
}

pub struct Rng;

impl RngCore for Rng {
    fn next_u32(&mut self) -> u32 {
        let mut req = RngRequest::U32(0);
        gen_rand(&mut req);
        if let RngRequest::U32(i) = req {
            return i;
        };
        0
    }

    fn next_u64(&mut self) -> u64 {
        let mut req = RngRequest::U64(0);
        gen_rand(&mut req);
        if let RngRequest::U64(i) = req {
            return i;
        };
        0
    }
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        let mut req = RngRequest::Bytes {
            ptr: dst.as_mut_ptr(),
            len: dst.len(),
        };
        gen_rand(&mut req);
    }
}

pub mod fs {
    use crate::SysError;
    use alloc::{string::String, vec::Vec};
    use core::fmt::Display;

    pub fn read_file(file: &str, start_from: usize, buf: &mut [u8]) -> Result<usize, SysError> {
        // SAFETY: the path and exclusively borrowed output live through this call.
        let read = unsafe {
            userlib_sys::read_file(
                file.as_ptr(),
                file.len(),
                start_from,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        SysError::from_code(userlib_sys::get_last_error())?;
        Ok(read)
    }

    pub fn write_file(file: &str, start_from: usize, buf: &[u8]) -> Result<(), SysError> {
        // SAFETY: both immutable input slices live through this synchronous call.
        unsafe {
            userlib_sys::write_file(
                file.as_ptr(),
                file.len(),
                start_from,
                buf.as_ptr(),
                buf.len(),
            )
        };
        SysError::from_code(userlib_sys::get_last_error())
    }

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct FileName<'a> {
        full: &'a str,
        base: &'a str,
        ext: Option<&'a str>,
    }

    impl<'a> FileName<'a> {
        pub fn full_name(&self) -> &str {
            self.full
        }

        pub fn base(&self) -> &str {
            self.base
        }

        pub fn extension(&self) -> Option<&str> {
            self.ext
        }
    }

    impl<'a> Display for FileName<'a> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "{}", self.full_name())
        }
    }

    impl<'a> From<&'a str> for FileName<'a> {
        fn from(s: &'a str) -> FileName<'a> {
            let full = s;

            // Split on last dot for extension
            let (base, ext) = match s.rfind('.') {
                Some(idx) => (&s[..idx], Some(&s[idx + 1..])),
                None => (s, None),
            };

            FileName { full, base, ext }
        }
    }

    const MAX_ENTRY_NAME_LEN: usize = 25;
    const MAX_ENTRIES: usize = 25;

    #[derive(Clone, Copy, Debug)]
    pub struct Entries {
        names: [[u8; MAX_ENTRY_NAME_LEN]; MAX_ENTRIES],
        count: usize,
    }

    impl Default for Entries {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Entries {
        pub fn new() -> Self {
            Self {
                names: [[0; MAX_ENTRY_NAME_LEN]; MAX_ENTRIES],
                count: 0,
            }
        }

        /// Get list of file names after listing
        pub fn entries<'a>(&'a self) -> Vec<FileName<'a>> {
            self.names[..self.count]
                .iter()
                .filter_map(|buf| {
                    let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    if nul_pos == 0 {
                        return None;
                    }
                    Some(core::str::from_utf8(&buf[..nul_pos]).ok()?.into())
                })
                .collect()
        }

        fn as_ptrs(&mut self) -> [*mut core::ffi::c_char; MAX_ENTRIES] {
            let mut ptrs = [core::ptr::null_mut(); MAX_ENTRIES];
            for (i, buf) in self.names.iter_mut().enumerate() {
                ptrs[i] = buf.as_mut_ptr().cast();
            }
            ptrs
        }
    }

    pub fn list_dir(path: &str, entries: &mut Entries) -> Result<usize, SysError> {
        *entries = Entries::new();
        let mut pointers = entries.as_ptrs();
        // SAFETY: every pointer names a distinct writable fixed-capacity name buffer.
        let count = unsafe {
            userlib_sys::list_dir(
                path.as_ptr(),
                path.len(),
                pointers.as_mut_ptr(),
                MAX_ENTRIES,
                MAX_ENTRY_NAME_LEN,
            )
        };
        SysError::from_code(userlib_sys::get_last_error())?;
        if count > MAX_ENTRIES {
            return Err(SysError::Io);
        }
        entries.count = count;
        Ok(count)
    }

    /// Lists matching media in the SD root and its optional legacy media folder.
    pub fn list_media(directory: &str, extension: &str) -> Result<Vec<String>, SysError> {
        let mut paths = Vec::new();
        let mut entries = Entries::new();
        for parent in ["/", directory] {
            match list_dir(parent, &mut entries) {
                Ok(_) => (),
                Err(SysError::NotFound) if parent != "/" => continue,
                Err(error) => return Err(error),
            }
            for entry in entries.entries() {
                if !entry
                    .extension()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(extension)
                {
                    continue;
                }
                let prefix = parent.trim_end_matches('/');
                let mut path = String::new();
                path.try_reserve_exact(prefix.len() + 1 + entry.full_name().len())
                    .map_err(|_| SysError::NoMemory)?;
                path.push_str(prefix);
                path.push('/');
                path.push_str(entry.full_name());
                paths.try_reserve(1).map_err(|_| SysError::NoMemory)?;
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn file_len(path: &str) -> Result<usize, SysError> {
        // SAFETY: path remains a valid immutable UTF-8 slice through this call.
        let length = unsafe { userlib_sys::file_len(path.as_ptr(), path.len()) };
        SysError::from_code(userlib_sys::get_last_error())?;
        Ok(length)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use core::sync::atomic::{AtomicI32, Ordering};

        static LIST_ERROR: AtomicI32 = AtomicI32::new(0);

        unsafe extern "C" fn list_fixture(
            directory: *const u8,
            length: usize,
            entries: *mut *mut core::ffi::c_char,
            count: usize,
            capacity: usize,
        ) -> usize {
            // SAFETY: list_dir passes a live path slice for this synchronous call.
            let directory = unsafe { core::slice::from_raw_parts(directory, length) };
            let names: &[&[u8]] = match directory {
                b"/" => &[b"ROOT.BMP", b"APP.BIN", b"song.WaV"],
                b"/images" => &[b"long image.bmp", b"skip.gif"],
                b"/missing" => {
                    LIST_ERROR.store(SysError::NotFound as i32, Ordering::Relaxed);
                    return 0;
                }
                _ => {
                    LIST_ERROR.store(SysError::Io as i32, Ordering::Relaxed);
                    return 0;
                }
            };
            LIST_ERROR.store(0, Ordering::Relaxed);
            assert!(names.len() <= count);
            for (index, name) in names.iter().enumerate() {
                assert!(name.len() < capacity);
                // SAFETY: list_dir supplies count separate buffers of capacity bytes.
                unsafe {
                    let output = *entries.add(index);
                    core::ptr::copy_nonoverlapping(name.as_ptr(), output.cast(), name.len());
                    *output.add(name.len()) = 0;
                }
            }
            names.len()
        }

        extern "C" fn fixture_error() -> i32 {
            LIST_ERROR.load(Ordering::Relaxed)
        }

        #[test]
        fn media_scan_combines_root_and_legacy_folder_and_preserves_errors() {
            // SAFETY: no other test uses the filesystem syscall slots.
            unsafe {
                userlib_sys::SYS_CALL_TABLE[userlib_sys::SyscallTable::ListDir as usize] =
                    list_fixture as *const () as usize;
                userlib_sys::SYS_CALL_TABLE[userlib_sys::SyscallTable::GetLastError as usize] =
                    fixture_error as *const () as usize;
            }
            assert_eq!(
                list_media("/images", "bmp").unwrap(),
                ["/ROOT.BMP", "/images/long image.bmp"]
            );
            assert_eq!(list_media("/missing", "WAV").unwrap(), ["/song.WaV"]);
            assert_eq!(list_media("/", "bmp").unwrap(), ["/ROOT.BMP"]);
            assert_eq!(list_media("/broken", "bmp"), Err(SysError::Io));
        }

        #[test]
        fn entries_only_return_current_nonempty_valid_names() {
            let mut entries = Entries::new();
            assert!(entries.entries().is_empty());
            entries.names[0][..7].copy_from_slice(b"one.wav");
            entries.names[1][0] = 0xff;
            entries.names[3][..7].copy_from_slice(b"old.wav");
            entries.count = 3;
            assert_eq!(entries.entries(), alloc::vec![FileName::from("one.wav")]);
            entries = Entries::new();
            assert!(entries.entries().is_empty());
        }
    }
}

pub mod audio {
    use crate::SysError;
    pub use userlib_sys::{AUDIO_BUFFER_LEN, AUDIO_BUFFER_SAMPLES, audio_buffer_ready};

    pub fn send_audio_buffer(buf: &[u8]) -> Result<(), SysError> {
        // SAFETY: buf remains a valid immutable slice until the kernel copies it.
        unsafe { userlib_sys::send_audio_buffer(buf.as_ptr(), buf.len()) };
        SysError::from_code(userlib_sys::get_last_error())
    }

    pub fn reconfigure_sample_rate(sample_rate: u32) -> Result<(), SysError> {
        userlib_sys::reconfigure_audio_sample_rate(sample_rate);
        SysError::from_code(userlib_sys::get_last_error())
    }
}
