#![no_std]
#![no_main]

extern crate alloc;
use abi::{KeyCode, display::Display, get_key, print, sleep};
use alloc::{boxed::Box, format, string::String, vec};
use core::{panic::PanicInfo, pin::Pin};
use embedded_graphics::{
    Drawable,
    geometry::{Dimensions, Point},
    mono_font::{
        MonoTextStyle,
        ascii::{self, FONT_6X10},
    },
    pixelcolor::Rgb565,
    prelude::{Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print(&format!(
        "user panic: {} @ {:?}",
        info.message(),
        info.location(),
    ));
    loop {}
}

pub fn main() {
    print("Starting Async Calculator app");
    let mut display = Display;

    let character_style = MonoTextStyle::new(&FONT_6X10, Rgb565::RED);

    let mut text = vec!['T', 'y', 'p', 'e'];
    let mut dirty = true;
    let mut last_area: Option<Rectangle> = None;

    loop {
        if dirty {
            if let Some(area) = last_area {
                Rectangle::new(area.top_left, area.size)
                    .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                    .draw(&mut display)
                    .unwrap();
            }

            let text = text.iter().cloned().collect::<String>();
            let aligned_text = Text::with_alignment(
                &text,
                display.bounding_box().center() + Point::new(0, 15),
                character_style,
                Alignment::Center,
            );

            last_area = Some(aligned_text.bounding_box());

            aligned_text.draw(&mut display).unwrap();

            dirty = false;
        }

        if let Some(event) = get_key() {
            match event.key {
                KeyCode::Char(ch) => {
                    text.push(ch);
                }
                KeyCode::Del => {
                    text.clear();
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Esc => return,
                _ => (),
            }
            dirty = true;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}
