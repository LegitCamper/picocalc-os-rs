use core::sync::atomic::Ordering;

use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::{Delay, Instant, Timer};
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Dimensions, Point, RgbColor, Size},
    primitives::Rectangle,
    text::{Alignment, Text},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use portable_atomic::AtomicBool;
use st7365p_lcd::{FrameBuffer, ST7365P};

use crate::LAST_TEXT_RECT;

const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 320;

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
    let mut framebuffer: FrameBuffer<
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        { SCREEN_WIDTH * SCREEN_HEIGHT },
    > = FrameBuffer::new();
    display.init().await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap();
    framebuffer.draw(&mut display).await.unwrap();
    display.set_on().await.unwrap();

    DISPLAY_SIGNAL.signal(());

    loop {
        DISPLAY_SIGNAL.wait().await;

        let text_string = crate::STRING.lock().await.clone();

        let text = Text::with_alignment(
            &text_string,
            Point::new(160, 160),
            MonoTextStyle::new(&FONT_10X20, Rgb565::RED),
            Alignment::Center,
        );

        {
            let rect = LAST_TEXT_RECT.lock().await;
            if let Some(rect) = *rect.borrow() {
                framebuffer.fill_solid(&rect, Rgb565::BLACK).unwrap();
            }
            *rect.borrow_mut() = Some(text.bounding_box());
        }

        text.draw(&mut framebuffer).unwrap();

        let start = Instant::now();
        framebuffer
            .partial_draw_batched(&mut display)
            .await
            .unwrap();
        info!("Elapsed {}ms", start.elapsed().as_millis());
    }
}
