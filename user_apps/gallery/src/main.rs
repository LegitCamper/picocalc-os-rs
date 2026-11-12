#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use alloc::{format, vec::Vec};
use core::panic::PanicInfo;
use embedded_graphics::{
    Drawable, image::Image, mono_font::MonoTextStyle, mono_font::ascii::FONT_6X10,
    pixelcolor::Rgb565, prelude::*, text::Text,
};
use tinybmp::Bmp;
use userlib::{
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    fs::{Entries, list_dir, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    println,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location());
    loop {
        userlib::sleep(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    println!("Starting Gallery app");
    let mut bmp_buf = Vec::new();
    if bmp_buf.try_reserve_exact(100_000).is_err() {
        println!("Not enough memory for gallery");
        return;
    }
    bmp_buf.resize(100_000, 0);
    let Some(mut display) = Display::take() else {
        return;
    };

    let grid_cols = 3;
    let grid_rows = 3;
    let cell_width = SCREEN_WIDTH as i32 / grid_cols;
    let cell_height = SCREEN_HEIGHT as i32 / grid_rows;

    let mut images_drawn = 0;

    let mut entries = Entries::new();
    if let Err(error) = list_dir("/images", &mut entries) {
        println!("Cannot list /images: {:?}", error);
        return;
    }

    for file in entries.entries() {
        if images_drawn >= grid_cols * grid_rows {
            break; // only draw 3x3
        }

        println!("file: {}", file);
        if file.extension().unwrap_or("") == "bmp" || file.extension().unwrap_or("") == "BMP" {
            let file_path = format!("/images/{file}");

            let Ok(read) = read_file(&file_path, 0, &mut bmp_buf[..]) else {
                println!("Cannot read {}", file_path);
                continue;
            };
            if read > 0 {
                let Ok(bmp) = Bmp::from_slice(&bmp_buf[..read]) else {
                    println!("Invalid or oversized BMP: {}", file_path);
                    continue;
                };

                let row = images_drawn / grid_cols;
                let col = images_drawn % grid_cols;
                let cell_x = col * cell_width;
                let cell_y = row * cell_height;

                // Center image inside cell
                let bmp_w = bmp.size().width as i32;
                let bmp_h = bmp.size().height as i32;
                let x = cell_x + (cell_width - bmp_w) / 2;
                let y = cell_y + 5; // 5px top margin

                if Image::new(&bmp, Point::new(x, y))
                    .draw(&mut display)
                    .is_err()
                {
                    return;
                }

                let text_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
                let text_y = y + bmp_h + 2; // 2px gap under image
                if Text::new(file.base(), Point::new(cell_x + 2, text_y), text_style)
                    .draw(&mut display)
                    .is_err()
                {
                    return;
                }

                images_drawn += 1;
            }
        }
    }

    loop {
        let event = get_key();
        if event.state != KeyState::Idle && event.key == KeyCode::Esc {
            return;
        }
    }
}
