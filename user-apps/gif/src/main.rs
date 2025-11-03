#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::Display,
    fs::{file_len, read_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::vec;
use core::panic::PanicInfo;
use embedded_graphics::{
    image::ImageDrawable, pixelcolor::Rgb565, prelude::Point, transform::Transform,
};
use tinygif::Gif;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print!("Starting Gif app");
    let mut display = Display;

    let size = file_len("/gifs/bad_apple.gif");
    let mut buf = vec![0_u8; size];
    let read = read_file("/gifs/bad_apple.gif", 0, &mut buf);
    print!("read: {}, file size: {}", read, size);
    assert!(read == size);

    let gif = Gif::<Rgb565>::from_slice(&buf).unwrap();
    let height = gif.height();

    let mut frame_num = 0;
    loop {
        for mut frame in gif.frames() {
            let start = get_ms();

            frame
                .translate_mut(Point::new(0, (320 - height as i32) / 2))
                .draw(&mut display)
                .unwrap();
            frame_num += 1;

            if frame_num % 5 == 0 {
                let event = get_key();
                if event.state != KeyState::Idle {
                    match event.key {
                        KeyCode::Esc => {
                            drop(buf);
                            return;
                        }
                        _ => (),
                    };
                };
            }
            sleep(((frame.delay_centis as u64) * 10).saturating_sub(start));
        }
    }
}
