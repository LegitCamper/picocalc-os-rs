#![no_std]
#![no_main]

use abi::display::Display;
use core::panic::PanicInfo;
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

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    let mut display = Display;

    let character_style = MonoTextStyle::new(&FONT_6X10, Rgb565::RED);

    // Draw centered text.
    let text = "embedded-graphics";
    Text::with_alignment(
        text,
        display.bounding_box().center() + Point::new(0, 15),
        character_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .unwrap();
}
