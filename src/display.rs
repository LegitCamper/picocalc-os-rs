use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Blocking, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable, Pixel,
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::{DrawTarget, OriginDimensions, Point, Primitive, RawData, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
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
        true,
        false,
        320,
        320,
    );
    display.init(&mut Delay).await.unwrap();
    display.init(&mut Delay).await.unwrap();
    display.set_address_window(0, 0, 319, 319).await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap(); // inverts X axis (reverts the natural mirroring)
    let mut framebuffer: FrameBuffer<
        320,
        320,
        ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'_>, Delay>,
        Output<'_>,
        Output<'_>,
    > = FrameBuffer::new(display);

    let thin_stroke = PrimitiveStyle::with_stroke(Rgb565::RED, 20);

    Rectangle::new(Point::new(10, 10), Size::new(100, 100))
        .into_styled(thin_stroke)
        .draw(&mut framebuffer)
        .unwrap();

    loop {
        Timer::after_millis(500).await;
        info!("drawing");
        framebuffer.draw().await.unwrap();
    }
}
