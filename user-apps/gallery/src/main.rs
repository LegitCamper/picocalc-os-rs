#![no_std]
#![no_main]
#![allow(static_mut_refs)]

extern crate alloc;
use abi::{
    KeyCode, KeyState, Rng,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    file_len, get_key, list_dir, lock_display, print, read_file, sleep,
};
use alloc::{format, vec::Vec};
use core::{cell::RefCell, panic::PanicInfo};
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
    static mut BMP_BUF: [u8; 100_000] = [0_u8; 100_000];
    let mut display = Display;

    // Grid parameters
    let grid_cols = 3;
    let grid_rows = 3;
    let cell_width = 64;
    let cell_height = 64;

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
                    let x = (col * cell_width) as i32 + 10; // 10px margin
                    let y = (row * cell_height) as i32 + 10;

                    lock_display(true);
                    Image::new(&bmp, Point::new(x, y))
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
