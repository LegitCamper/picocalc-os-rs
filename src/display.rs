use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Blocking, Spi},
};
use embassy_time::{Delay, Instant, Timer};
use embedded_graphics::{
    Drawable, Pixel,
    geometry::Dimensions,
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::{DrawTarget, OriginDimensions, Point, Primitive, RawData, RgbColor, Size, WebColors},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Text, TextStyle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, Orientation, ST7365P};

#[embassy_executor::task]
pub async fn display_task(spi: Spi<'static, SPI1, Async>, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        320,
        320,
    );
    let mut framebuffer: FrameBuffer<
        320,
        320,
        ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'_>, Delay>,
        Output<'_>,
        Output<'_>,
    > = FrameBuffer::new(display);

    framebuffer.init(&mut Delay).await.unwrap();
    framebuffer.display.set_offset(0, 0);
    framebuffer
        .display
        .set_custom_orientation(0x60)
        .await
        .unwrap();

    let t_style = MonoTextStyle::new(&FONT_6X10, Rgb565::BLUE);

    Text::with_alignment(
        "0, 0\n new line",
        Point::new(0, 0),
        t_style,
        Alignment::Right,
    )
    .draw(&mut framebuffer)
    .unwrap();
    Text::new("319, 319", Point { x: 320, y: 300 }, t_style)
        .draw(&mut framebuffer)
        .unwrap();

    Text::with_alignment(
        "160, 160",
        framebuffer.bounding_box().center(),
        t_style,
        Alignment::Center,
    )
    .draw(&mut framebuffer)
    .unwrap();

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}
