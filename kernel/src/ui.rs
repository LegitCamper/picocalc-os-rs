use crate::{
    BINARY_CH, display::FRAMEBUFFER, elf::load_binary, framebuffer::FB_PAUSED,
    peripherals::keyboard, storage::FileName,
};
use alloc::{str::FromStr, string::String, vec::Vec};
use core::sync::atomic::Ordering;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Dimensions, Point, Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::{FixedMargin, LinearLayout},
    prelude::*,
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
            match event.key {
                KeyCode::Up => {
                    let mut selections = SELECTIONS.lock().await;
                    selections.up();
                }
                KeyCode::Down => {
                    let mut selections = SELECTIONS.lock().await;
                    selections.down();
                }
                KeyCode::Enter | KeyCode::Right => {
                    let selections = SELECTIONS.lock().await;
                    let selection =
                        selections.selections[selections.current_selection as usize].clone();

                    let entry = unsafe {
                        load_binary(&selection.short_name)
                            .await
                            .expect("unable to load binary")
                    };
                    BINARY_CH.send(entry).await;
                }
                _ => (),
            }
        }

        let changed = SELECTIONS.lock().await.changed;
        if changed {
            clear_selection().await;
            draw_selection().await;
        }
    }
}

pub async fn clear_selection() {
    let sel = SELECTIONS.lock().await;

    if let Some(area) = sel.last_bounds {
        Rectangle::new(area.top_left, area.size)
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
            .unwrap();
    }
}

async fn draw_selection() {
    let mut guard = SELECTIONS.lock().await;
    let file_names = guard.selections.clone();

    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let display_area = unsafe { FRAMEBUFFER.as_mut().unwrap().bounding_box() };

    const NO_BINS: &str = "No Programs found on SD Card. Ensure programs end with '.bin', and are located in the root directory";
    let no_bins = String::from_str(NO_BINS).unwrap();

    FB_PAUSED.store(true, Ordering::Release); // ensure all elements show up at once

    if file_names.is_empty() {
        TextBox::new(
            &no_bins,
            Rectangle::new(
                Point::new(25, 25),
                Size::new(display_area.size.width - 50, display_area.size.width - 50),
            ),
            text_style,
        )
        .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
        .unwrap();
    } else {
        let mut views: alloc::vec::Vec<Text<MonoTextStyle<Rgb565>>> = Vec::new();

        for i in &file_names {
            views.push(Text::new(&i.long_name, Point::zero(), text_style));
        }

        let views_group = Views::new(views.as_mut_slice());

        let layout = LinearLayout::vertical(views_group)
            .with_alignment(horizontal::Center)
            .with_spacing(FixedMargin(5))
            .arrange()
            .align_to(&display_area, horizontal::Center, vertical::Center);

        // draw selected box
        let selected_bounds = layout
            .inner()
            .get(guard.current_selection as usize)
            .expect("Selected binary missing")
            .bounding_box();
        Rectangle::new(selected_bounds.top_left, selected_bounds.size)
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
            .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
            .unwrap();

        guard.last_bounds = Some(layout.bounds());

        layout
            .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
            .unwrap();
    }

    guard.changed = false;
    FB_PAUSED.store(false, Ordering::Release); // ensure all elements show up at once
}

#[derive(Clone)]
pub struct SelectionList {
    // allows easy clearing of selection ui,
    // based on previous bounds
    last_bounds: Option<Rectangle>,
    current_selection: u16,
    selections: Vec<FileName>,
    changed: bool,
}

impl SelectionList {
    pub const fn new() -> Self {
        Self {
            last_bounds: None,
            selections: Vec::new(),
            current_selection: 0,
            changed: false,
        }
    }

    pub fn set_changed(&mut self, changed: bool) {
        self.changed = changed
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
        self.changed = true;
    }

    fn up(&mut self) {
        if self.current_selection > 0 {
            self.current_selection -= 1;
            self.changed = true;
        }
    }

    fn down(&mut self) {
        if self.current_selection + 1 < self.selections.len() as u16 {
            self.current_selection += 1;
            self.changed = true;
        }
    }
}
