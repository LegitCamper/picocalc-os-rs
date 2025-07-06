use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
    text::Text,
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};

const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 320;

#[embassy_executor::task]
pub async fn display_task(spi: Spi<'static, SPI1, Async>, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let mut display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
    );
    display.init(&mut Delay).await.unwrap();
    display.set_custom_orientation(0x60).await.unwrap();

    let mut framebuffer: FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT> = FrameBuffer::new();

    Text::with_alignment(
        "Hello!",
        Point::new(160, 160),
        MonoTextStyle::new(&FONT_10X20, Rgb565::RED),
        embedded_graphics::text::Alignment::Center,
    )
    .draw(&mut framebuffer)
    .unwrap();

    loop {
        framebuffer.draw(&mut display).await.unwrap();
        Timer::after_millis(500).await;
    }
}
