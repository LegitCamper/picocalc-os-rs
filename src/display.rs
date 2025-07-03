use arrform::{ArrForm, arrform};
use core::fmt::Debug;
use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_6X10, FONT_9X15, FONT_10X20},
    },
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
    text::Text,
};
use embedded_hal_1::digital::OutputPin;
use embedded_hal_async::spi::SpiDevice;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::LinearLayout,
    object_chain::Chain,
    prelude::*,
};
use heapless::{String, Vec};
use st7365p_lcd::{FrameBuffer, Orientation, ST7365P};

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

pub const STATUS_BAR_WIDTH: usize = 320;
pub const STATUS_BAR_HEIGHT: usize = 40;

pub type FRAMEBUFFER = FrameBuffer<
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
>;

pub struct UI<const MAX_SELECTIONS: usize, const MAX_STR_LEN: usize> {
    pub status_bar: StatusBar,
    pub selections_list: SelectionList<MAX_SELECTIONS, MAX_STR_LEN>,
}

impl<const MAX_SELECTIONS: usize, const MAX_STR_LEN: usize> UI<MAX_SELECTIONS, MAX_STR_LEN> {
    pub fn new() -> Self {
        Self {
            status_bar: StatusBar {
                battery: 100,
                backlight: 100,
                volume: 100,
            },
            selections_list: SelectionList::new(Vec::new()),
        }
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D)
    where
        <D as DrawTarget>::Error: Debug,
    {
        self.draw_status_bar(target);
        self.draw_selection(target);
    }

    fn draw_selection<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) {
        let text_style = MonoTextStyle::new(&FONT_9X15, Rgb565::WHITE);

        let selection = Rectangle::new(
            Point::new(0, STATUS_BAR_HEIGHT as i32 + 1),
            Size::new(
                SCREEN_WIDTH as u32,
                (SCREEN_HEIGHT - STATUS_BAR_HEIGHT) as u32 - 1,
            ),
        );

        let _ = if self.selections_list.selections.is_empty() {
            LinearLayout::horizontal(Chain::new(Text::new(
                "No Programs found on SD Card\nEnsure programs end with '.rhai',\nand are located in the root directory",
                Point::zero(),
                text_style,
            )))
            .arrange()
            .align_to(&selection, horizontal::Center, vertical::Center).draw(target)
        } else {
            LinearLayout::horizontal(
                Chain::new(Text::new(
                    arrform!(20, "Bat: {}", self.status_bar.battery).as_str(),
                    Point::zero(),
                    text_style,
                ))
                .append(Text::new(" ", Point::zero(), text_style))
                .append(Text::new(
                    arrform!(20, "Lght: {}", self.status_bar.backlight).as_str(),
                    Point::zero(),
                    text_style,
                ))
                .append(Text::new(" ", Point::zero(), text_style))
                .append(Text::new(
                    arrform!(20, "Vol: {}", self.status_bar.volume).as_str(),
                    Point::zero(),
                    text_style,
                )),
            )
            .arrange()
            .align_to(&selection, horizontal::Left, vertical::Center)
            .draw(target)
        };
    }

    fn draw_status_bar<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) {
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

pub struct SelectionList<const MAX_SELECTION: usize, const MAX_STR_LEN: usize> {
    current_selection: u16,
    selections: Vec<String<MAX_STR_LEN>, MAX_SELECTION>,
}

impl<const MAX_SELECTION: usize, const MAX_STR_LEN: usize>
    SelectionList<MAX_SELECTION, MAX_STR_LEN>
{
    pub fn new(selections: Vec<String<MAX_STR_LEN>, MAX_SELECTION>) -> Self {
        Self {
            selections,
            current_selection: 0,
        }
    }

    pub fn down(&mut self) {
        if self.current_selection + 1 < self.selections.len() as u16 {
            self.current_selection += 1
        }
    }

    pub fn up(&mut self) {
        if self.current_selection > self.selections.len() as u16 {
            self.current_selection -= 1
        }
    }
}
