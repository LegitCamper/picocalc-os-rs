use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Blocking, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable, Pixel,
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::{DrawTarget, OriginDimensions, Point, Primitive, RawData, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{Orientation, ST7365P};

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
    display.init(&mut Delay).unwrap();
    display.set_orientation(&Orientation::Landscape).unwrap();
    let mut virtual_display = VirtualDisplay::new(display, 320 / 2, 320 / 2);

    let thin_stroke = PrimitiveStyle::with_stroke(Rgb565::RED, 20);

    Rectangle::new(Point::new(10, 10), Size::new(100, 100))
        .into_styled(thin_stroke)
        .draw(&mut virtual_display)
        .unwrap();

    loop {
        Timer::after_millis(500).await;
    }
}

/// simple abstraction over real display & resolution to reduce frame buffer size
/// by cutting the resolution by 1/4
struct VirtualDisplay {
    display: ST7365P<
        ExclusiveDevice<Spi<'static, SPI1, Blocking>, Output<'static>, Delay>,
        Output<'static>,
        Output<'static>,
    >,
    width: u32,
    height: u32,
}

impl VirtualDisplay {
    pub fn new(
        display: ST7365P<
            ExclusiveDevice<Spi<'static, SPI1, Blocking>, Output<'static>, Delay>,
            Output<'static>,
            Output<'static>,
        >,
        new_width: u32,
        new_height: u32,
    ) -> Self {
        Self {
            display,
            width: new_width,
            height: new_height,
        }
    }
}

impl DrawTarget for VirtualDisplay {
    type Color = Rgb565;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            // Check bounds on the *virtual* (already reduced) resolution
            if coord.x >= 0
                && coord.y >= 0
                && coord.x < self.width as i32
                && coord.y < self.height as i32
            {
                let px = coord.x as u16 * 2;
                let py = coord.y as u16 * 2;
                let raw_color = RawU16::from(color).into_inner();

                // Draw the 2x2 block on the underlying hardware
                self.display.set_pixel(px, py, raw_color)?;
                self.display.set_pixel(px + 1, py, raw_color)?;
                self.display.set_pixel(px, py + 1, raw_color)?;
                self.display.set_pixel(px + 1, py + 1, raw_color)?;
            }
        }
        Ok(())
    }
}

impl OriginDimensions for VirtualDisplay {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}
