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
        iso_8859_1::FONT_10X20,
    },
    pixelcolor::Rgb565,
    prelude::{Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::{
        LinearLayout,
        spacing::{DistributeFill, FixedMargin},
    },
    object_chain::Chain,
    prelude::*,
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

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print("Starting Async Calculator app");
    let mut display = Display;

    let mut input = vec!['e', 'x', 'p', 'r', ':', ' '];
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

            let text = input.iter().cloned().collect::<String>();

            let expr = Text::new(
                &text,
                display.bounding_box().center(),
                MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
            );

            let layout = LinearLayout::vertical(
                Chain::new(Text::new(
                    "Calculator!",
                    Point::zero(),
                    MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE),
                ))
                .append(
                    LinearLayout::horizontal(Chain::new(expr).append(Text::new(
                        " = 901",
                        Point::zero(),
                        MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
                    )))
                    .with_spacing(DistributeFill(expr.size().width))
                    .arrange()
                    .align_to(
                        &display.bounding_box(),
                        horizontal::Center,
                        vertical::Center,
                    ),
                ),
            )
            .with_spacing(DistributeFill(50))
            .arrange()
            .align_to(
                &display.bounding_box(),
                horizontal::Center,
                vertical::Center,
            );

            last_area = Some(layout.bounds());
            layout.draw(&mut display).unwrap();

            dirty = false;
        }

        if let Some(event) = get_key() {
            match event.key {
                KeyCode::Char(ch) => {
                    input.push(ch);
                }
                KeyCode::Del => {
                    input.clear();
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Esc => return,
                _ => (),
            }
            dirty = true;
        }
    }
}
