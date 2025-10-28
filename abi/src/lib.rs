#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

pub use abi_sys::{self, keyboard};
use abi_sys::{RngRequest, alloc, dealloc, keyboard::KeyEvent};
pub use alloc::format;
use core::alloc::{GlobalAlloc, Layout};
use rand_core::RngCore;

#[global_allocator]
static ALLOC: Alloc = Alloc;

struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        alloc(layout.into())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        dealloc(ptr, layout.into());
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let s = $crate::format!($($arg)*);
        $crate::abi_sys::print(s.as_ptr(), s.len());
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
    use abi_sys::CPixel;
    // use alloc::{vec, vec::Vec};
    use embedded_graphics::{
        Pixel,
        geometry::{Dimensions, Point},
        pixelcolor::Rgb565,
        prelude::{DrawTarget, Size},
        primitives::Rectangle,
    };
    // use once_cell::unsync::Lazy;

    pub const SCREEN_WIDTH: usize = 320;
    pub const SCREEN_HEIGHT: usize = 320;

    pub type Pixel565 = Pixel<Rgb565>;

    const BUF_SIZE: usize = 15 * 1024; // tune this for performance
    static mut BUF: [CPixel; BUF_SIZE] = [CPixel::new(); BUF_SIZE];
    // const BUF_SIZE: usize = 250 * 1024; // tune this for performance
    // static mut BUF: Lazy<Vec<CPixel>> = Lazy::new(|| vec![const { CPixel::new() }; BUF_SIZE]);

    pub struct Display;

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
    use embedded_sdmmc::DirEntry;

    pub fn read_file(file: &str, read_from: usize, buf: &mut [u8]) -> usize {
        abi_sys::read_file(
            file.as_ptr(),
            file.len(),
            read_from,
            buf.as_mut_ptr(),
            buf.len(),
        )
    }

    pub fn list_dir(path: &str, files: &mut [Option<DirEntry>]) -> usize {
        abi_sys::list_dir(path.as_ptr(), path.len(), files.as_mut_ptr(), files.len())
    }

    pub fn file_len(str: &str) -> usize {
        abi_sys::file_len(str.as_ptr(), str.len())
    }
}
