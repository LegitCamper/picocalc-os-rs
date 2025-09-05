use crate::{
    TASK_STATE, TaskState,
    display::{FRAMEBUFFER, SCREEN_HEIGHT, SCREEN_WIDTH},
    format,
    peripherals::keyboard,
};
use alloc::{string::String, vec::Vec};
use core::fmt::Debug;
use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    mutex::Mutex,
    signal::Signal,
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
use shared::keyboard::KeyCode;

static SELECTIONS: Mutex<CriticalSectionRawMutex, SelectionList> =
    Mutex::new(SelectionList::new(Vec::new()));

pub async fn ui_handler() {
    loop {
        let state = TASK_STATE.lock().await;
        if let TaskState::Ui = *state {
            let mut selections = SELECTIONS.lock().await;
            if let Some(event) = keyboard::read_keyboard_fifo().await {
                match event.key {
                    KeyCode::JoyUp => selections.up(),
                    KeyCode::JoyDown => selections.down(),
                    KeyCode::Enter | KeyCode::JoyRight => (),
                    _ => (),
                }
            }

            draw_selection().await;
        }
    }
}

async fn draw_selection() {
    let mut fb_lock = FRAMEBUFFER.lock().await;
    if let Some(fb) = fb_lock.as_mut() {
        info!("UIINg");
        let text_style = MonoTextStyle::new(&FONT_9X15, Rgb565::WHITE);

        let guard = SELECTIONS.lock().await;
        let mut file_names = guard.selections.iter();

        let Some(first) = file_names.next() else {
            Text::new("No Programs found on SD Card\nEnsure programs end with '.bin',\nand are located in the root directory",
                Point::zero(), text_style).draw(*fb).unwrap();

            return;
        };

        let chain = Chain::new(Text::new(first, Point::zero(), text_style));

        for _ in 0..10 {
            LinearLayout::vertical(chain)
                .with_alignment(horizontal::Center)
                .arrange()
                .align_to(&fb.bounding_box(), horizontal::Center, vertical::Center)
                .draw(*fb)
                .unwrap();
            break;
        }
    }
}

pub struct SelectionList {
    current_selection: u16,
    selections: Vec<String>,
}

impl SelectionList {
    pub const fn new(selections: Vec<String>) -> Self {
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
