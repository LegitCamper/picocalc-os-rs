#![no_std]
#![no_main]

extern crate alloc;
use abi::{KeyCode, display::Display, get_key, print, sleep};
use alloc::{boxed::Box, string::String, vec};
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
use kolibri_embedded_gui::{label::Label, style::medsize_rgb565_style, ui::Ui};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub async fn main() {
    print("Starting Async Calculator app");
    let mut display = Display;

    let mut text = vec!['T', 'y', 'p', 'e'];
    let mut dirty = true;

    loop {
        if dirty {
            let mut ui = Ui::new_fullscreen(&mut display, medsize_rgb565_style());
            let text = text.iter().cloned().collect::<String>();

            // ui.clear_background();
            ui.add(Label::new(&text).with_font(ascii::FONT_10X20));
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
pub extern "Rust" fn _start() -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(async { main().await })
}
