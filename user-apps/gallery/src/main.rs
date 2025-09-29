#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    KeyCode, KeyState, Rng,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    file_len, get_key, list_dir, lock_display, print, read_file, sleep,
};
use alloc::{format, vec::Vec};
use core::panic::PanicInfo;
use embedded_graphics::{Drawable, image::Image, prelude::*};
use tinybmp::Bmp;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print(&format!(
        "user panic: {} @ {:?}",
        info.message(),
        info.location(),
    ));
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print("Starting Gallery app");
    let mut display = Display;

    let file = "/images/ferriseyes_tiny.bmp";

    let mut bmp_buf = [0_u8; 3_000];
    let read = read_file(file, 0, &mut bmp_buf);
    let bmp = Bmp::from_slice(&bmp_buf[..read]).unwrap();

    // ensure all draws show up at once
    lock_display(true);
    Image::new(&bmp, Point::new(10, 20))
        .draw(&mut display)
        .unwrap();
    lock_display(false);

    loop {
        let event = get_key();
        if event.state != KeyState::Idle {
            match event.key {
                KeyCode::Esc => return,
                _ => (),
            }
        };
    }
}
