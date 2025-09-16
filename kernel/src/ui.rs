use core::sync::atomic::Ordering;

use crate::{
    BINARY_CH, display::FRAMEBUFFER, elf::load_binary, peripherals::keyboard, storage::FileName,
    usb::USB_ACTIVE,
};
use alloc::vec::Vec;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::mono_font::ascii;
use kolibri_embedded_gui::{label::Label, style::medsize_rgb565_style, ui::Ui};
use shared::keyboard::{KeyCode, KeyState};

pub static SELECTIONS: Mutex<CriticalSectionRawMutex, SelectionList> =
    Mutex::new(SelectionList::new());

pub async fn ui_handler() {
    loop {
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

                        let entry = unsafe { load_binary(&selection.short_name).await.unwrap() };
                        BINARY_CH.send(entry).await;
                    }
                    _ => (),
                }
            }
        }

        if SELECTIONS.lock().await.changed {
            draw_selection().await;
        }
    }
}

async fn draw_selection() {
    const NO_BINS: &str = "No Programs found on SD Card. Ensure programs end with '.bin', and are located in the root directory";
    let file_names: Vec<FileName> = {
        let guard = SELECTIONS.lock().await;
        guard.selections.clone()
    };

    let mut ui = Ui::new_fullscreen(unsafe { &mut FRAMEBUFFER }, medsize_rgb565_style());

    if file_names.is_empty() {
        ui.add(Label::new(NO_BINS).with_font(ascii::FONT_10X20));
    } else {
        for file in file_names {
            ui.add(Label::new(&file.long_name).with_font(ascii::FONT_10X20));
        }
    }

    let mut sel = SELECTIONS.lock().await;
    sel.changed = false;
}

#[derive(Clone)]
pub struct SelectionList {
    current_selection: u16,
    selections: Vec<FileName>,
    changed: bool,
}

impl SelectionList {
    pub const fn new() -> Self {
        Self {
            selections: Vec::new(),
            current_selection: 0,
            changed: false,
        }
    }

    pub fn update_selections(&mut self, selections: Vec<FileName>) {
        self.selections = selections;
        self.changed = true;
    }

    pub fn selections(&self) -> &Vec<FileName> {
        &self.selections
    }

    pub fn reset(&mut self) {
        self.current_selection = 1;
        self.changed = true;
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
