#![no_std]
#![no_main]

use abi::{Pixel, Point, Rgb565, RgbColor, Syscall, call_abi};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    // Local pixel buffer
    let mut pixels = [Pixel(Point { x: 50, y: 50 }, Rgb565::RED); 300];
    for (i, p) in pixels.iter_mut().enumerate() {
        *p = Pixel(
            Point {
                x: i as i32,
                y: i as i32,
            },
            Rgb565::RED,
        )
    }

    // Construct syscall with raw pointer + length
    let call = Syscall::DrawIter {
        pixels: pixels.as_ptr(), // raw pointer
        len: pixels.len(),       // number of elements
    };

    unsafe { call_abi(&call) };
}
