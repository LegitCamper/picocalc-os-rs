use crate::{
    BINARY_CH, TASK_STATE, TaskState,
    display::{FRAMEBUFFER, SCREEN_HEIGHT, SCREEN_WIDTH},
    elf::load_binary,
    format,
    peripherals::keyboard,
    storage::FileName,
};
use alloc::{string::String, vec::Vec};
use core::{fmt::Debug, str::FromStr, sync::atomic::Ordering};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    mutex::Mutex,
};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_9X15},
    pixelcolor::Rgb565,
    prelude::{Dimensions, Point, RgbColor, Size},
    primitives::Rectangle,
    text::Text,
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::LinearLayout,
    object_chain::Chain,
    prelude::*,
};
use embedded_text::TextBox;
use shared::keyboard::{KeyCode, KeyState};

pub static SELECTIONS: Mutex<CriticalSectionRawMutex, SelectionList> =
    Mutex::new(SelectionList::new());

pub async fn ui_handler() {
    loop {
        if let TaskState::Ui = *TASK_STATE.lock().await {
            if let Some(event) = keyboard::read_keyboard_fifo().await {
                if let KeyState::Pressed = event.state {
                    match event.key {
                        KeyCode::JoyUp => {
                            let mut selections = SELECTIONS.lock().await;
                            selections.up();
                        }
                        KeyCode::JoyDown => {
                            let mut selections = SELECTIONS.lock().await;
                            selections.down();
                        }
                        KeyCode::Enter | KeyCode::JoyRight => {
                            let selections = SELECTIONS.lock().await;
                            let selection = selections.selections
                                [selections.current_selection as usize - 1]
                                .clone();

                            let entry =
                                unsafe { load_binary(&selection.short_name).await.unwrap() };
                            BINARY_CH.send(entry).await;
                        }
                        _ => (),
                    }
                }
            }

            draw_selection().await;
        } else {
            embassy_time::Timer::after_millis(50).await;
        }
    }
}

async fn draw_selection() {
    let file_names: Vec<FileName> = {
        let guard = SELECTIONS.lock().await;
        guard.selections.clone()
    };

    let mut fb = FRAMEBUFFER.get().lock().await;
    let text_style = MonoTextStyle::new(&FONT_9X15, Rgb565::WHITE);
    let display_area = fb.bounding_box();

    const NO_BINS: &str = "No Programs found on SD Card. Ensure programs end with '.bin', and are located in the root directory";
    let no_bins = String::from_str(NO_BINS).unwrap();

    if file_names.is_empty() {
        TextBox::new(
            &no_bins,
            Rectangle::new(
                Point::new(25, 25),
                Size::new(display_area.size.width - 50, display_area.size.width - 50),
            ),
            text_style,
        )
        .draw(&mut *fb)
        .unwrap();
    } else {
        let mut file_names = file_names.iter();
        let Some(first) = file_names.next() else {
            Text::new(NO_BINS, Point::zero(), text_style)
                .draw(&mut *fb)
                .unwrap();

            return;
        };

        let chain = Chain::new(Text::new(&first.long_name, Point::zero(), text_style));

        // for _ in 0..file_names.len() {
        //     let chain = chain.append(Text::new(
        //         file_names.next().unwrap(),
        //         Point::zero(),
        //         text_style,
        //     ));
        // }

        LinearLayout::vertical(chain)
            .with_alignment(horizontal::Center)
            .arrange()
            .align_to(&display_area, horizontal::Center, vertical::Center)
            .draw(&mut *fb)
            .unwrap();
    }
}

#[derive(Clone)]
pub struct SelectionList {
    current_selection: u16,
    pub selections: Vec<FileName>,
}

impl SelectionList {
    pub const fn new() -> Self {
        Self {
            selections: Vec::new(),
            current_selection: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_selection = 1
    }

    fn down(&mut self) {
        if self.current_selection + 1 < self.selections.len() as u16 {
            self.current_selection += 1
        }
    }

    fn up(&mut self) {
        if self.current_selection > self.selections.len() as u16 {
            self.current_selection -= 1
        }
    }
}
