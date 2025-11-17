#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

use abi_sys::{RngRequest, keyboard::KeyEvent};
pub use abi_sys::{keyboard, print};
pub use alloc::format;
use core::alloc::{GlobalAlloc, Layout};
use rand_core::RngCore;

#[global_allocator]
static ALLOC: Alloc = Alloc;

struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        abi_sys::alloc(layout.into())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        abi_sys::dealloc(ptr, layout.into());
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        let s = $crate::format!($($arg)*);
        $crate::print(s.as_ptr(), s.len());
    }};
}

pub fn sleep(ms: u64) {
    abi_sys::sleep(ms);
}

pub fn get_ms() -> u64 {
    abi_sys::get_ms()
}

pub fn get_key() -> KeyEvent {
    abi_sys::keyboard::get_key().into()
}

pub mod display {
    use core::sync::atomic::{AtomicBool, Ordering};

    use abi_sys::CPixel;
    use embedded_graphics::{
        Pixel,
        geometry::{Dimensions, Point},
        pixelcolor::Rgb565,
        prelude::{DrawTarget, Size},
        primitives::Rectangle,
    };

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
                unsafe { BUF[count] = p.into() };
                count += 1;

                if count == BUF_SIZE - 1 {
                    abi_sys::draw_iter(unsafe { BUF.as_ptr() }, count);
                    count = 0;
                }
            }

            if count > 0 {
                abi_sys::draw_iter(unsafe { BUF.as_ptr() }, count);
            }

            Ok(())
        }
    }
}

fn gen_rand(req: &mut RngRequest) {
    abi_sys::gen_rand(req);
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
    use alloc::vec::Vec;
    use core::fmt::Display;

    pub fn read_file(file: &str, start_from: usize, buf: &mut [u8]) -> usize {
        abi_sys::read_file(
            file.as_ptr(),
            file.len(),
            start_from,
            buf.as_mut_ptr(),
            buf.len(),
        )
    }

    pub fn write_file(file: &str, start_from: usize, buf: &[u8]) {
        abi_sys::write_file(
            file.as_ptr(),
            file.len(),
            start_from,
            buf.as_ptr(),
            buf.len(),
        )
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
    pub struct Entries([[u8; MAX_ENTRY_NAME_LEN]; MAX_ENTRIES]);

    impl Entries {
        pub fn new() -> Self {
            Self([[0; MAX_ENTRY_NAME_LEN]; MAX_ENTRIES])
        }

        /// Get list of file names after listing
        pub fn entries<'a>(&'a self) -> Vec<FileName<'a>> {
            self.0
                .iter()
                .filter_map(|buf| {
                    let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    Some(core::str::from_utf8(&buf[..nul_pos]).ok()?.into())
                })
                .collect()
        }

        fn as_ptrs(&mut self) -> [*mut u8; MAX_ENTRIES] {
            let mut ptrs: [*mut u8; MAX_ENTRIES] = [core::ptr::null_mut(); MAX_ENTRIES];
            for (i, buf) in self.0.iter_mut().enumerate() {
                ptrs[i] = buf.as_mut_ptr();
            }
            ptrs
        }
    }

    pub fn list_dir(path: &str, entries: &mut Entries) -> usize {
        abi_sys::list_dir(
            path.as_ptr(),
            path.len(),
            entries.as_ptrs().as_mut_ptr(),
            MAX_ENTRIES,
            MAX_ENTRY_NAME_LEN,
        )
    }

    pub fn file_len(str: &str) -> usize {
        abi_sys::file_len(str.as_ptr(), str.len())
    }
}

pub mod audio {
    pub use abi_sys::{AUDIO_BUFFER_LEN, AUDIO_BUFFER_SAMPLES, audio_buffer_ready};

    pub fn send_audio_buffer(buf: &[u8]) {
        abi_sys::send_audio_buffer(buf.as_ptr(), buf.len())
    }
}
