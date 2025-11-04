#![no_std]

extern crate alloc;

use abi::{
    display::Display,
    fs::{Entries, FileName},
    get_key,
    keyboard::{KeyCode, KeyState},
};
use alloc::vec::Vec;
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

pub struct SelectionUi<'a> {
    selection: usize,
    items: &'a Entries,
    error: &'a str,
    last_bounds: Option<Rectangle>,
}

impl<'a> SelectionUi<'a> {
    pub fn new(items: &'a Entries, error: &'a str) -> Self {
        Self {
            selection: 0,
            items,
            error,
            last_bounds: None,
        }
    }

    pub fn run_selection_ui(&mut self, display: &mut Display) -> Result<Option<usize>, ()> {
        self.draw(display)?;
        let selection;
        loop {
            let key = get_key();
            if key.state == KeyState::Pressed {
                if let Some(s) = self.update(display, key.key)? {
                    selection = Some(s);
                    break;
                }
            }
        }
        Ok(selection)
    }

    /// updates the display with a new keypress.
    /// returns selection idx if selected
    pub fn update(&mut self, display: &mut Display, key: KeyCode) -> Result<Option<usize>, ()> {
        match key {
            KeyCode::JoyUp => {
                let _ = self.selection.saturating_sub(1);
            }
            KeyCode::JoyDown => {
                let _ = self.selection.saturating_add(1);
            }
            KeyCode::Enter | KeyCode::JoyRight => return Ok(Some(self.selection)),
            _ => (),
        };
        self.draw(display)?;
        Ok(None)
    }

    fn draw(&mut self, display: &mut Display) -> Result<(), ()> {
        let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let display_area = display.bounding_box();

        let entries = self.items.entries();

        if entries.is_empty() {
            TextBox::new(
                &self.error,
                Rectangle::new(
                    Point::new(25, 25),
                    Size::new(display_area.size.width - 50, display_area.size.width - 50),
                ),
                text_style,
            )
            .draw(display)
            .unwrap();
        }

        let mut views: Vec<Text<MonoTextStyle<Rgb565>>> = Vec::new();

        for i in &entries {
            views.push(Text::new(i.full_name(), Point::zero(), text_style));
        }

        let views_group = Views::new(views.as_mut_slice());

        let layout = LinearLayout::vertical(views_group)
            .with_alignment(horizontal::Center)
            .with_spacing(FixedMargin(5))
            .arrange()
            .align_to(&display_area, horizontal::Center, vertical::Center);

        // draw selected box
        let selected_bounds = layout.inner().get(self.selection).ok_or(())?.bounding_box();
        Rectangle::new(selected_bounds.top_left, selected_bounds.size)
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
            .draw(display)?;

        self.last_bounds = Some(layout.bounds());

        layout.draw(display)?;
        Ok(())
    }
}
