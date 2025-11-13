#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    fs::{Entries, file_len, list_dir, read_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use alloc::{format, vec, vec::Vec};
use core::panic::PanicInfo;
use embedded_graphics::{
    image::ImageDrawable,
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
    transform::Transform,
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};
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
    let mut display = Display::take().unwrap();

    let mut entries = Entries::new();
    list_dir("/gifs", &mut entries);

    let mut files = entries.entries();
    files.retain(|e| e.extension().unwrap_or("") == "gif");
    let mut gifs = files.iter().map(|e| e.full_name()).collect::<Vec<&str>>();
    gifs.sort();

    let mut selection_ui = SelectionUi::new(&mut gifs);
    let selection = match selection_ui.run_selection_ui(&mut display) {
        Ok(maybe_sel) => maybe_sel,
        Err(e) => match e {
            SelectionUiError::SelectionListEmpty => {
                draw_text_center(
                    &mut display,
                    "No Gifs were found in /gifs",
                    MonoTextStyle::new(&FONT_6X10, Rgb565::RED),
                )
                .expect("Display Error");
                None
            }
            SelectionUiError::DisplayError(_) => panic!("Display Error"),
        },
    };

    assert!(selection.is_some());

    let file_name = format!("/gifs/{}", gifs[selection.unwrap()]);
    let size = file_len(&file_name);
    let mut buf = vec![0_u8; size];
    let read = read_file(&file_name, 0, &mut buf);
    print!("read: {}, file size: {}", read, size);
    assert!(read == size);

    let gif = Gif::<Rgb565>::from_slice(&buf).expect("Failed to parse gif");

    let translation = Point::new(
        (SCREEN_WIDTH as i32 - gif.width() as i32) / 2,
        (SCREEN_HEIGHT as i32 - gif.height() as i32) / 2,
    );

    let mut frame_num = 0;
    loop {
        for mut frame in gif.frames() {
            let start = get_ms();

            frame.translate_mut(translation).draw(&mut display).unwrap();
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
