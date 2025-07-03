use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoFont, MonoTextStyle,
        ascii::{FONT_6X9, FONT_10X20},
    },
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, WebColors},
    text::{Baseline, Text, TextStyle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};

type SPI = Spi<'static, SPI1, Async>;

type FRAMEBUFFER = FrameBuffer<
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
>;

const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 320;
const SCREEN_ROWS: usize = 15;
const SCREEN_COLS: usize = 31;
const FONT: MonoFont = FONT_10X20;
const COLOR: Rgb565 = Rgb565::CSS_LAWN_GREEN;

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

    Text::with_alignment(
        "Hello!",
        Point::new(160, 160),
        MonoTextStyle::new(&FONT_10X20, Rgb565::RED),
        embedded_graphics::text::Alignment::Center,
    )
    .draw(&mut framebuffer)
    .unwrap();

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}
