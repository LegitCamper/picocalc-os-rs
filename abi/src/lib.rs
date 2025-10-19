#![no_std]

pub use abi_sys::keyboard;
use abi_sys::{RngRequest, keyboard::KeyEvent};
use rand_core::RngCore;
use talc::*;

extern crate alloc;

static mut ARENA: [u8; 10000] = [0; 10000];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

pub fn print(msg: &str) {
    abi_sys::print(msg.as_ptr(), msg.len());
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

    pub fn lock_display(lock: bool) {
        abi_sys::lock_display(lock);
    }

    const BUF_SIZE: usize = 1024; // tune this for performance

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
            let mut buf: [CPixel; BUF_SIZE] = [CPixel::new(); BUF_SIZE];

            let mut count = 0;
            for p in pixels {
                buf[count] = p.into();
                count += 1;

                if count == BUF_SIZE {
                    abi_sys::draw_iter(buf.as_ptr(), count);
                    count = 0;
                }
            }

            if count > 0 {
                abi_sys::draw_iter(buf.as_ptr(), count);
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
