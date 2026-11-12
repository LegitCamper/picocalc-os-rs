use crate::{
    BINARY_CH,
    display::FRAMEBUFFER,
    elf::{LoadError, load_binary},
    peripherals::keyboard,
    storage::{FileName, SDCARD},
};
use alloc::vec::Vec;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Dimensions, Point, Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use embedded_text::TextBox;
use userlib_sys::keyboard::{KeyCode, KeyState};

pub static SELECTIONS: Mutex<CriticalSectionRawMutex, SelectionList> =
    Mutex::new(SelectionList::new());

pub async fn ui_handler() {
    loop {
        if let Some(event) = keyboard::read_keyboard_fifo().await
            && let KeyState::Pressed = event.state
        {
            let pending_delete = SELECTIONS.lock().await.pending_delete.clone();
            if let Some(selection) = pending_delete {
                match event.key {
                    KeyCode::Enter => delete_program(&selection).await,
                    KeyCode::Esc | KeyCode::Up | KeyCode::Down => {
                        let mut selections = SELECTIONS.lock().await;
                        selections.pending_delete = None;
                        selections.changed = true;
                    }
                    _ => (),
                }
            } else {
                match event.key {
                    KeyCode::Del => {
                        let mut selections = SELECTIONS.lock().await;
                        if selections.error.is_none() {
                            selections.pending_delete = selections
                                .selections
                                .get(selections.current_selection)
                                .cloned();
                            selections.changed = true;
                        }
                    }
                    KeyCode::Up => {
                        let mut selections = SELECTIONS.lock().await;
                        selections.up();
                    }
                    KeyCode::Down => {
                        let mut selections = SELECTIONS.lock().await;
                        selections.down();
                    }
                    KeyCode::Enter | KeyCode::Right => {
                        let selection = {
                            let selections = SELECTIONS.lock().await;
                            selections
                                .selections
                                .get(selections.current_selection)
                                .cloned()
                        };
                        if let Some(selection) = selection {
                            // SAFETY: the loader validates the ELF before returning its entry point.
                            match unsafe { load_binary(&selection.short_name).await } {
                                Ok(entry) => BINARY_CH.send(entry).await,
                                Err(error) => {
                                    let message = match error {
                                        LoadError::SyscallTableSizeMismatch => {
                                            "ABI mismatch. Rebuild apps. Up/Down to return."
                                        }
                                        LoadError::OutOfMemory => {
                                            "Not enough memory. Up/Down to return."
                                        }
                                        LoadError::FailedToReadFile => {
                                            "Cannot read program. Check SD card. Up/Down to return."
                                        }
                                        _ => "Invalid program. Up/Down to return.",
                                    };
                                    #[cfg(feature = "defmt")]
                                    defmt::warn!("{}", message);
                                    SELECTIONS.lock().await.set_error(message);
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }

        let changed = SELECTIONS.lock().await.changed;
        if changed {
            clear_selection().await;
            draw_selection().await;
        }
        embassy_time::Timer::after_millis(10).await;
    }
}

async fn delete_program(selection: &FileName) {
    let mut sd_guard = SDCARD.get().lock().await;
    let deleted = sd_guard.as_mut().is_some_and(|sd| {
        sd.is_attached()
            && matches!(
                sd.access_root_dir(|dir| dir.delete_file_in_dir(&selection.short_name)),
                Ok(Ok(()))
            )
    });
    drop(sd_guard);
    let mut selections = SELECTIONS.lock().await;
    selections.pending_delete = None;
    if deleted {
        selections
            .selections
            .retain(|file| file.short_name != selection.short_name);
        selections.current_selection = selections
            .current_selection
            .min(selections.selections.len().saturating_sub(1));
        selections.changed = true;
    } else {
        #[cfg(feature = "defmt")]
        defmt::warn!("Unable to delete selected program");
        selections.set_error("Unable to delete program. Press Up or Down to return.");
    }
}

pub async fn clear_selection() {
    let sel = SELECTIONS.lock().await;

    if let Some(area) = sel.last_bounds
        && let Some(framebuffer) = FRAMEBUFFER.lock().await.as_mut()
    {
        Rectangle::new(area.top_left, area.size)
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(framebuffer)
            .unwrap();
    }
}

async fn draw_selection() {
    let mut guard = SELECTIONS.lock().await;
    let mut framebuffer_guard = FRAMEBUFFER.lock().await;
    let Some(framebuffer) = framebuffer_guard.as_mut() else {
        return;
    };

    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let display_area = framebuffer.bounding_box();

    const NO_BINS: &str = "No Programs found on SD Card. Ensure programs end with '.bin', and are located in the root directory";
    let delete_prompt = guard.pending_delete.as_ref().map(|file| {
        crate::format!(
            512,
            "Delete {} from SD card?\nEnter: permanently delete\nEsc: cancel",
            file.long_name
        )
    });
    if guard.selections.is_empty() || guard.error.is_some() || delete_prompt.is_some() {
        TextBox::new(
            delete_prompt.as_deref().or(guard.error).unwrap_or(NO_BINS),
            Rectangle::new(
                Point::new(25, 25),
                Size::new(display_area.size.width - 50, display_area.size.width - 50),
            ),
            text_style,
        )
        .draw(framebuffer)
        .unwrap();
        guard.last_bounds = Some(display_area);
    } else {
        let first = guard.current_selection.saturating_sub(5);
        for (row, file) in guard.selections.iter().enumerate().skip(first).take(11) {
            let top = 10 + ((row - first) * 26) as i32;
            if row == guard.current_selection {
                let _ = Rectangle::new(Point::new(5, top), Size::new(310, 26))
                    .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
                    .draw(framebuffer);
            }
            let mut name = heapless::String::<120>::new();
            for character in file.long_name.chars().take(29) {
                let _ = name.push(character);
            }
            let _ = Text::new(&name, Point::new(10, top + 20), text_style).draw(framebuffer);
        }
        guard.last_bounds = Some(display_area);
    }

    guard.changed = false;
}

#[derive(Clone)]
pub struct SelectionList {
    // allows easy clearing of selection ui,
    // based on previous bounds
    last_bounds: Option<Rectangle>,
    current_selection: usize,
    selections: Vec<FileName>,
    changed: bool,
    error: Option<&'static str>,
    pending_delete: Option<FileName>,
}

impl SelectionList {
    pub const fn new() -> Self {
        Self {
            last_bounds: None,
            selections: Vec::new(),
            current_selection: 0,
            changed: true,
            error: None,
            pending_delete: None,
        }
    }

    pub fn set_changed(&mut self, changed: bool) {
        self.changed = changed
    }

    pub fn set_error(&mut self, message: &'static str) {
        self.error = Some(message);
        self.changed = true;
    }

    pub fn set_storage_available(&mut self, available: bool) {
        const MESSAGE: &str = "SD card unavailable or directory too large. Retrying...";
        if !available {
            self.set_error(MESSAGE);
        } else if self.error == Some(MESSAGE) {
            self.error = None;
            self.changed = true;
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
        self.current_selection = 0;
        self.error = None;
        self.pending_delete = None;
        self.changed = true;
    }

    fn up(&mut self) {
        if self.error.take().is_some() {
            self.changed = true;
        }
        if self.current_selection > 0 {
            self.current_selection -= 1;
            self.changed = true;
        }
    }

    fn down(&mut self) {
        if self.error.take().is_some() {
            self.changed = true;
        }
        if self.current_selection + 1 < self.selections.len() {
            self.current_selection += 1;
            self.changed = true;
        }
    }
}
