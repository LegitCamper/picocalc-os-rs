use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Blocking, Spi},
};
use embassy_time::{Delay, Instant, Timer};
use embedded_graphics::{
    Drawable, Pixel,
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::{DrawTarget, OriginDimensions, Point, Primitive, RawData, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Text, TextStyle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, Orientation, ST7365P};

#[embassy_executor::task]
pub async fn display_task(spi: Spi<'static, SPI1, Async>, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let mut display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        320,
        320,
    );
    display.init(&mut Delay).await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap();

    let mut framebuffer: FrameBuffer<
        320,
        320,
        ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'_>, Delay>,
        Output<'_>,
        Output<'_>,
    > = FrameBuffer::new(display);

    Text::new(
        "PicoCalc Test\nLine 2\n Line 3 - and 1/2",
        Point { x: 100, y: 100 },
        MonoTextStyle::new(&FONT_6X10, Rgb565::BLUE),
    )
    .draw(&mut framebuffer)
    .unwrap();

    Rectangle::new(Point::new(0, 0), Size::new(50, 50))
        .draw_styled(
            &PrimitiveStyle::with_stroke(Rgb565::RED, 10),
            &mut framebuffer,
        )
        .unwrap();

    Rectangle::new(Point::new(0, 100), Size::new(50, 50))
        .draw_styled(
            &PrimitiveStyle::with_stroke(Rgb565::RED, 10),
            &mut framebuffer,
        )
        .unwrap();

    loop {
        Timer::after_millis(500).await;
        let start = Instant::now();
        framebuffer.draw().await.unwrap();
        info!(
            "Took {}ms to write framebuffer to screen",
            Instant::now().duration_since(start).as_millis()
        );
    }
}
