use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Blocking, Spi},
};
use embassy_time::Delay;
use embedded_graphics::{
    Drawable,
    pixelcolor::{BinaryColor, Rgb555, Rgb565},
    prelude::{Point, Primitive, RgbColor},
    primitives::{PrimitiveStyle, Triangle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::ST7365P;

#[embassy_executor::task]
pub async fn display_task(
    spi: Spi<'static, SPI1, Blocking>,
    cs: PIN_13,
    data: PIN_14,
    reset: PIN_15,
) {
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

    let thin_stroke = PrimitiveStyle::with_stroke(Rgb565::RED, 1);

    let yoffset = 10;

    // Draw a triangle.
    Triangle::new(
        Point::new(16, 16 + yoffset),
        Point::new(16 + 16, 16 + yoffset),
        Point::new(16 + 8, yoffset),
    )
    .into_styled(thin_stroke)
    .draw(&mut display)
    .unwrap();
}
