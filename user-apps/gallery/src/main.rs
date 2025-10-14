#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use abi::{
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH, lock_display},
    fs::{list_dir, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    print,
};
use alloc::{format, string::ToString};
use core::panic::PanicInfo;
use embedded_graphics::{
    Drawable, image::Image, mono_font::MonoTextStyle, mono_font::ascii::FONT_6X10,
    pixelcolor::Rgb565, prelude::*, text::Text,
};
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
    static mut BMP_BUF: [u8; 100_000] = [0_u8; 100_000];
    let mut display = Display;

    // Grid parameters
    let grid_cols = 3;
    let grid_rows = 3;
    let cell_width = SCREEN_WIDTH as i32 / grid_cols;
    let cell_height = SCREEN_HEIGHT as i32 / grid_rows;

    let mut images_drawn = 0;

    let mut files = [const { None }; 18];
    let files_num = list_dir("/images", &mut files);

    for file in &files[2..files_num] {
        if images_drawn >= grid_cols * grid_rows {
            break; // only draw 3x3
        }

        if let Some(f) = file {
            print(&format!("file: {}", f.name));
            if f.name.extension() == b"bmp" || f.name.extension() == b"BMP" {
                let file = format!("/images/{}", f.name);

                let read = read_file(&file, 0, &mut unsafe { &mut BMP_BUF[..] });
                if read > 0 {
                    let bmp = Bmp::from_slice(unsafe { &BMP_BUF }).expect("failed to parse bmp");

                    let row = images_drawn / grid_cols;
                    let col = images_drawn % grid_cols;
                    let cell_x = col * cell_width;
                    let cell_y = row * cell_height;

                    // Center image inside cell
                    let bmp_w = bmp.size().width as i32;
                    let bmp_h = bmp.size().height as i32;
                    let x = cell_x + (cell_width - bmp_w) / 2;
                    let y = cell_y + 5; // 5px top margin

                    lock_display(true);
                    Image::new(&bmp, Point::new(x, y))
                        .draw(&mut display)
                        .unwrap();

                    let text_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
                    let text_y = y + bmp_h + 2; // 2px gap under image
                    Text::new(
                        f.name.to_string().as_str(),
                        Point::new(cell_x + 2, text_y),
                        text_style,
                    )
                    .draw(&mut display)
                    .unwrap();

                    lock_display(false);

                    images_drawn += 1;
                }
            }
        }
    }

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
