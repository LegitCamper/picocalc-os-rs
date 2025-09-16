#![no_std]

use abi_sys::draw_iter;
pub use abi_sys::{get_key, print, sleep};
pub use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};
use talc::*;

static mut ARENA: [u8; 10000] = [0; 10000];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

pub mod display {
    use crate::draw_iter;
    use embedded_graphics::{
        Pixel,
        geometry::{Dimensions, Point},
        pixelcolor::{Rgb565, RgbColor},
        prelude::{DrawTarget, Size},
        primitives::Rectangle,
    };

    pub const SCREEN_WIDTH: usize = 320;
    pub const SCREEN_HEIGHT: usize = 320;

    pub type Pixel565 = Pixel<Rgb565>;

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
            const BUF_SIZE: usize = 1024; // tune this for performance
            let mut buf: [Pixel565; BUF_SIZE] = [Pixel(Point::new(0, 0), Rgb565::BLACK); BUF_SIZE];

            let mut count = 0;
            for p in pixels {
                buf[count] = p;
                count += 1;

                if count == BUF_SIZE {
                    draw_iter(&buf[..count]);
                    count = 0;
                }
            }

            if count > 0 {
                draw_iter(&buf[..count]);
            }

            Ok(())
        }
    }
}
