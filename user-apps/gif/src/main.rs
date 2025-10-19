#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::{Display, lock_display},
    fs::read_file,
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::format;
use core::panic::PanicInfo;
use embedded_graphics::{image::ImageDrawable, pixelcolor::Rgb565};
use tinygif::{Gif, Header};

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
    print("Starting Gif app");
    let mut display = Display;

    static mut BUF: [u8; 256] = [0_u8; 256];

    read_file("/gif/bad_apple.gif", 0, unsafe { &mut BUF[0..6] });

    let gif_header = Header::parse(unsafe { &BUF[0..6] });

    let image = Gif::<Rgb565>::from_slice().unwrap();

    loop {
        for frame in image.frames() {
            let start = get_ms();

            frame.draw(&mut display).unwrap();

            sleep(((frame.delay_centis as u64) * 10).saturating_sub(start));

            let event = get_key();
            if event.state != KeyState::Idle {
                match event.key {
                    KeyCode::Esc => return,
                    _ => (),
                };
            };

            lock_display(true);
            lock_display(false);
        }
    }
}
