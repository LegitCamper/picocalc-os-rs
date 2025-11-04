#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::Display,
    fs::{Entries, file_len, list_dir, read_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::{format, vec, vec::Vec};
use core::panic::PanicInfo;
use embedded_graphics::{
    image::ImageDrawable, pixelcolor::Rgb565, prelude::Point, transform::Transform,
};
use selection_ui::SelectionUi;
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

    let mut gifs = Entries::new();
    list_dir("/gifs", &mut gifs);

    gifs.entries()
        .retain(|e| e.extension().unwrap_or("") == "gif");

    let mut selection_ui = SelectionUi::new(&gifs, "No Gif files found in /gifs");
    let selection = selection_ui
        .run_selection_ui(&mut display)
        .expect("failed to draw")
        .expect("Failed to get user selection");

    let size = file_len(&format!("/gifs/{}.gif", gifs.entries()[selection]));
    let mut buf = vec![0_u8; size];
    let read = read_file("/gifs/bad_apple.gif", 0, &mut buf);
    print!("read: {}, file size: {}", read, size);
    assert!(read == size);

    let gif = Gif::<Rgb565>::from_slice(&buf).expect("Failed to parse gif");
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
