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
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use mousefood::prelude::*;
use ratatui::{Frame, Terminal, widgets::Paragraph};
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
        true,
        320,
        320,
    );
    display.set_offset(0, 0);
    display.init(&mut Delay).unwrap();
    display.set_orientation(&Orientation::Portrait).unwrap();
    display.set_address_window(0, 0, 319, 319).unwrap();

    // Rectangle::new(Point::new(0, 0), Size::new(1, 1))
    //     .draw_styled(&PrimitiveStyle::with_fill(Rgb565::GREEN), &mut display)
    //     .unwrap();

    // Rectangle::new(Point::new(319, 319), Size::new(1, 1))
    //     .draw_styled(&PrimitiveStyle::with_fill(Rgb565::GREEN), &mut display)
    //     .unwrap();

    let mut virtual_display = VirtualDisplay::new(display, 320, 320);

    let backend = EmbeddedBackend::new(&mut virtual_display, EmbeddedBackendConfig::default());
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        terminal.draw(draw).unwrap();
    }
    // loop {
    //     Timer::after_millis(100).await
    // }
}

fn draw(frame: &mut Frame) {
    let greeting = Paragraph::new("Hello World!");
    frame.render_widget(greeting, frame.area());
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
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            display,
            width: width / 2,
            height: height / 2,
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
            let px = coord.x as u16 * 2;
            let py = coord.y as u16 * 2;
            let raw_color = RawU16::from(color).into_inner();

            // Draw the 2x2 block on the underlying hardware
            self.display.set_pixel(px, py, raw_color)?;
            self.display.set_pixel(px + 1, py, raw_color)?;
            self.display.set_pixel(px, py + 1, raw_color)?;
            self.display.set_pixel(px + 1, py + 1, raw_color)?;
        }
        Ok(())
    }
}

impl OriginDimensions for VirtualDisplay {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}
