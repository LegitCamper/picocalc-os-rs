#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use core::panic::PanicInfo;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::{Dimensions, DrawTargetExt, Point, RgbColor},
    transform::Transform,
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};
use tinygif::Gif;
use userlib::{
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    fs::{file_len, list_media, read_file},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    println, sleep,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {
        userlib::sleep(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    println!("Starting Gif app");
    let Some(mut display) = Display::take() else {
        return;
    };

    let files = match list_media("/gifs", "gif") {
        Ok(files) => files,
        Err(error) => {
            println!("Cannot list GIFs: {:?}", error);
            return;
        }
    };
    let gifs = files.iter().map(|path| path.as_str()).collect::<Vec<_>>();

    let mut selection_ui = SelectionUi::new(&gifs);
    let selection = match selection_ui.run_selection_ui(&mut display) {
        Ok(maybe_sel) => maybe_sel,
        Err(e) => match e {
            SelectionUiError::SelectionListEmpty => {
                draw_text_center(
                    &mut display,
                    "No GIFs in / or /gifs",
                    MonoTextStyle::new(&FONT_6X10, Rgb565::RED),
                )
                .ok();
                None
            }
            SelectionUiError::DisplayError(_) => None,
        },
    };

    let Some(selection) = selection else {
        return;
    };

    let file_name = gifs[selection];
    let Ok(size) = file_len(file_name) else {
        return;
    };
    // ponytail: whole-file decoding is capped at 2 MiB; stream larger GIFs later.
    if size == 0 || size > 2_097_152 {
        println!("GIF is empty or exceeds 2 MiB");
        return;
    }
    let mut buf = Vec::new();
    if buf.try_reserve_exact(size).is_err() {
        println!("Not enough memory for GIF");
        return;
    }
    buf.resize(size, 0);
    let mut position = 0;
    while position < size {
        match read_file(file_name, position, &mut buf[position..]) {
            Ok(0) | Err(_) => return,
            Ok(read) => position += read,
        }
    }

    let Ok(gif) = Gif::<Rgb565>::from_slice(&buf) else {
        println!("Invalid GIF");
        return;
    };
    if u32::from(gif.width()) * u32::from(gif.height()) > 1_048_576 {
        println!("GIF exceeds decoded pixel limit");
        return;
    }

    let translation = Point::new(
        (SCREEN_WIDTH as i32 - gif.width() as i32) / 2,
        (SCREEN_HEIGHT as i32 - gif.height() as i32) / 2,
    );
    let bounds = display.bounding_box();
    let mut display = display.clipped(&bounds);

    loop {
        let mut has_frames = false;
        for mut frame in gif.frames() {
            has_frames = true;
            let start = get_ms();

            if frame
                .translate_mut(translation)
                .draw_checked(&mut display)
                .is_err()
            {
                return;
            }

            loop {
                let event = get_key();
                if event.state != KeyState::Idle && event.key == KeyCode::Esc {
                    return;
                }
                let elapsed = get_ms().saturating_sub(start);
                let remaining = (u64::from(frame.delay_centis) * 10).saturating_sub(elapsed);
                if remaining == 0 {
                    break;
                }
                sleep(remaining.min(10));
            }
        }
        if !has_frames {
            return;
        }
    }
}
