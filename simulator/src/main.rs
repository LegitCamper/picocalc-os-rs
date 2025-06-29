#![feature(ascii_char)]

use embedded_graphics::{
    geometry::Size,
    mono_font::{
        MonoFont, MonoTextStyle,
        ascii::{FONT_6X9, FONT_10X20},
    },
    pixelcolor::{BinaryColor, Rgb565},
    prelude::{Point, WebColors, *},
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyle},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, Window,
};

use shared::{SCREEN_HEIGHT, SCREEN_WIDTH, TextBuffer};

fn main() -> Result<(), core::convert::Infallible> {
    let mut display =
        SimulatorDisplay::<Rgb565>::new(Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));

    let mut textbuffer = TextBuffer::new();
    textbuffer.fill('A');
    textbuffer.draw(&mut display);

    let output_settings = OutputSettingsBuilder::new().build();
    Window::new("Hello World", &output_settings).show_static(&display);

    Ok(())
}
