use core::sync::atomic::Ordering;

use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::Rectangle,
    text::{Alignment, Text},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use portable_atomic::AtomicBool;
use st7365p_lcd::{FrameBuffer, ST7365P};

const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 320;
const REFRESH_INTERVAL_MS: u64 = 20;

pub static DISPLAY_SIGNAL: Signal<ThreadModeRawMutex, ()> = Signal::new();

pub async fn display_handler(
    spi: Spi<'static, SPI1, Async>,
    cs: PIN_13,
    data: PIN_14,
    reset: PIN_15,
) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let mut display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        Delay,
    );
    let mut framebuffer: FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT> = FrameBuffer::new();
    display.init().await.unwrap();
    display.set_custom_orientation(0x60).await.unwrap();
    framebuffer.draw(&mut display).await.unwrap();
    display.set_on().await.unwrap();

    loop {
        DISPLAY_SIGNAL.wait().await;

        framebuffer
            .fill_solid(
                &Rectangle::new(
                    Point::new(0, 0),
                    Size::new(SCREEN_HEIGHT as u32 - 1, SCREEN_WIDTH as u32 - 1),
                ),
                Rgb565::BLACK,
            )
            .unwrap();
        let text = crate::STRING.lock().await.clone();

        Text::with_alignment(
            &text,
            Point::new(160, 160),
            MonoTextStyle::new(&FONT_10X20, Rgb565::RED),
            Alignment::Center,
        )
        .draw(&mut framebuffer)
        .unwrap();

        framebuffer.draw(&mut display).await.unwrap();
    }
}
