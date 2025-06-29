use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoFont, MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Point, WebColors},
    text::{Baseline, Text, TextStyle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};

use shared::{SCREEN_HEIGHT, SCREEN_WIDTH, TextBuffer};

type SPI = Spi<'static, SPI1, Async>;

type FRAMEBUFFER = FrameBuffer<
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
>;

#[embassy_executor::task]
pub async fn display_task(spi: SPI, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    );
    let mut framebuffer: FRAMEBUFFER = FrameBuffer::new(display);

    framebuffer.init(&mut Delay).await.unwrap();
    framebuffer.display.set_offset(0, 0);
    framebuffer
        .display
        .set_custom_orientation(0x60)
        .await
        .unwrap();

    let mut textbuffer = TextBuffer::new();
    textbuffer.fill('A');
    textbuffer.draw(&mut framebuffer);
    info!("finished rendering");

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}
