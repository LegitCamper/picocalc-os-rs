use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, Orientation, ST7365P};

use arrform::{ArrForm, arrform};
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_9X15, FONT_10X20},
    },
    object_chain::Chain,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::Rectangle,
    text::Text,
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::LinearLayout,
    prelude::*,
};

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

pub const STATUS_BAR_WIDTH: usize = 320;
pub const STATUS_BAR_HEIGHT: usize = 40;

#[embassy_executor::task]
pub async fn display_task(spi: Spi<'static, SPI1, Async>, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        true,
        false,
        320,
        320,
    );
    display.init(&mut Delay).await.unwrap();
    display
        .set_orientation(&Orientation::Landscape)
        .await
        .unwrap();

    let mut framebuffer = FrameBuffer::new(display);

    let mut ui = UI::new();
    ui.draw_status_bar(&mut framebuffer);

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}

pub struct UI {
    pub status_bar: StatusBar,
}

impl UI {
    pub fn new() -> Self {
        Self {
            status_bar: StatusBar {
                battery: 100,
                backlight: 100,
                volume: 100,
            },
        }
    }

    pub fn draw_status_bar<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) {
        let text_style = MonoTextStyle::new(&FONT_9X15, Rgb565::WHITE);

        let status_bar = Rectangle::new(
            Point::new(0, 0),
            Size::new(STATUS_BAR_WIDTH as u32, STATUS_BAR_HEIGHT as u32),
        );
        let _ = LinearLayout::horizontal(
            Chain::new(Text::new(
                arrform!(20, "Bat: {}", self.status_bar.battery).as_str(),
                Point::zero(),
                text_style,
            ))
            .append(Text::new(
                arrform!(20, "Lght: {}", self.status_bar.backlight).as_str(),
                Point::zero(),
                text_style,
            ))
            .append(Text::new(
                arrform!(20, "Vol: {}", self.status_bar.volume).as_str(),
                Point::zero(),
                text_style,
            )),
        )
        .arrange()
        .align_to(&status_bar, horizontal::Center, vertical::Center)
        .draw(target);
    }
}

pub struct StatusBar {
    pub battery: u8,
    pub backlight: u8,
    pub volume: u8,
}
