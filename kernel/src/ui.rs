use crate::{
    display::{FRAMEBUFFER, SCREEN_HEIGHT, SCREEN_WIDTH},
    format,
};
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
    prelude::{Dimensions, Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
    text::Text,
};
use embedded_hal_async::spi::SpiDevice;
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::LinearLayout,
    object_chain::Chain,
    prelude::*,
};
use heapless::{String, Vec};

pub struct UI<const MAX_SELECTIONS: usize, const MAX_STR_LEN: usize> {
    pub selections_list: SelectionList<MAX_SELECTIONS, MAX_STR_LEN>,
}

impl<const MAX_SELECTIONS: usize, const MAX_STR_LEN: usize> UI<MAX_SELECTIONS, MAX_STR_LEN> {
    pub fn new() -> Self {
        Self {
            selections_list: SelectionList::new(Vec::new()),
        }
    }

    pub async fn draw<D: DrawTarget<Color = Rgb565>>(&mut self)
    where
        <D as DrawTarget>::Error: Debug,
    {
        self.draw_selection().await;
    }

    async fn draw_selection(&mut self) {
        let mut fb_lock = FRAMEBUFFER.lock().await;
        let fb = fb_lock.as_mut().unwrap();

        let text_style = MonoTextStyle::new(&FONT_9X15, Rgb565::WHITE);

        let selection = Rectangle::new(
            Point::new(0, 0),
            Size::new(SCREEN_WIDTH as u32 - 1, SCREEN_HEIGHT as u32 - 1),
        );

        let mut file_names = self.selections_list.selections.iter();

        let Some(first) = file_names.next() else {
            Text::new("No Programs found on SD Card\nEnsure programs end with '.bin',\nand are located in the root directory",
                Point::zero(), text_style).draw(*fb).unwrap();

            return;
        };

        let chain = Chain::new(Text::new(first, Point::zero(), text_style));

        LinearLayout::vertical(chain)
            .with_alignment(horizontal::Center)
            .arrange()
            .align_to(&fb.bounding_box(), horizontal::Center, vertical::Center)
            .draw(*fb)
            .unwrap();
    }
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
