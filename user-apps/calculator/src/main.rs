#![no_std]
#![no_main]

extern crate alloc;
use abi::{KeyCode, display::Display, embassy_time, get_key, print};
use alloc::{boxed::Box, string::String, vec};
use core::{panic::PanicInfo, pin::Pin};
use embedded_graphics::{
    Drawable,
    geometry::{Dimensions, Point},
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::RgbColor,
    text::{Alignment, Text},
};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub async fn main() {
    print("Starting Async Calculator app");
    let mut display = Display;

    let character_style = MonoTextStyle::new(&FONT_6X10, Rgb565::RED);

    let mut text = vec!['H', 'E', 'L', 'L', 'O'];

    loop {
        Text::with_alignment(
            &text.iter().cloned().collect::<String>(),
            display.bounding_box().center() + Point::new(0, 15),
            character_style,
            Alignment::Center,
        )
        .draw(&mut display)
        .unwrap();

        if let Some(event) = get_key() {
            print("User got event");
            match event.key {
                KeyCode::Char(ch) => {
                    text.push(ch);
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                _ => (),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(async { main().await })
}
