#![no_std]

use abi_sys::{Syscall, call_abi};
use shared::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

pub fn print(msg: &str) {
    let syscall = Syscall::Print {
        msg: msg.as_ptr(),
        len: msg.len(),
    };
    unsafe {
        call_abi(&syscall);
    }
}

pub mod display {
    use crate::{Syscall, call_abi};
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

    impl Display {
        fn syscall_draw(&self, pixels: &[Pixel565]) {
            let syscall = Syscall::DrawIter {
                pixels: pixels.as_ptr(),
                len: pixels.len(),
            };
            unsafe {
                call_abi(&syscall);
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
            const BUF_SIZE: usize = 1024; // tune this for performance
            let mut buf: [Pixel565; BUF_SIZE] = [Pixel(Point::new(0, 0), Rgb565::BLACK); BUF_SIZE];

            let mut count = 0;
            for p in pixels {
                buf[count] = p;
                count += 1;

                if count == BUF_SIZE {
                    self.syscall_draw(&buf[..count]);
                    count = 0;
                }
            }

            if count > 0 {
                self.syscall_draw(&buf[..count]);
            }

            Ok(())
        }
    }
}
