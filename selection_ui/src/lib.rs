#![no_std]

extern crate alloc;

use abi::{
    display::Display,
    get_key,
    keyboard::{KeyCode, KeyState},
};
use alloc::vec::Vec;
use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Dimensions, DrawTarget, Point, Primitive, RgbColor},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text, renderer::TextRenderer},
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::{FixedMargin, LinearLayout},
    prelude::*,
};

#[derive(Debug)]
pub enum SelectionUiError<DisplayError> {
    SelectionListEmpty,
    DisplayError(DisplayError),
}

pub struct SelectionUi<'a> {
    selection: usize,
    items: &'a [&'a str],
    last_bounds: Option<Rectangle>,
}

impl<'a> SelectionUi<'a> {
    pub fn new(items: &'a [&'a str]) -> Self {
        Self {
            selection: 0,
            items,
            last_bounds: None,
        }
    }

    pub fn run_selection_ui(
        &mut self,
        display: &mut Display,
    ) -> Result<Option<usize>, SelectionUiError<<Display as DrawTarget>::Error>> {
        self.draw(display)?;
        let selection;
        loop {
            let key = get_key();
            if key.state == KeyState::Pressed {
                if let Some(s) = self.update(display, key.key)? {
                    selection = Some(s);
                    display
                        .clear(Rgb565::BLACK)
                        .map_err(|e| SelectionUiError::DisplayError(e))?;
                    break;
                }
            }
        }
        Ok(selection)
    }

    /// updates the display with a new keypress.
    /// returns selection idx if selected
    pub fn update(
        &mut self,
        display: &mut Display,
        key: KeyCode,
    ) -> Result<Option<usize>, SelectionUiError<<Display as DrawTarget>::Error>> {
        match key {
            KeyCode::Down => {
                self.selection = (self.selection + 1).min(self.items.len() - 1);
            }
            KeyCode::Up => {
                self.selection = self.selection.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Right => return Ok(Some(self.selection)),
            _ => return Ok(None),
        };
        self.draw(display)?;
        Ok(None)
    }

    fn draw(
        &mut self,
        display: &mut Display,
    ) -> Result<(), SelectionUiError<<Display as DrawTarget>::Error>> {
        let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let display_area = display.bounding_box();

        if self.items.is_empty() {
            return Err(SelectionUiError::SelectionListEmpty);
        }

        if let Some(bounds) = self.last_bounds {
            Rectangle::new(bounds.top_left, bounds.size)
                .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
                .draw(display)
                .map_err(|e| SelectionUiError::DisplayError(e))?;
        }

        let mut views: Vec<Text<MonoTextStyle<Rgb565>>> = Vec::new();

        for i in self.items {
            views.push(Text::new(i, Point::zero(), text_style));
        }

        let views_group = Views::new(views.as_mut_slice());

        let layout = LinearLayout::vertical(views_group)
            .with_alignment(horizontal::Center)
            .with_spacing(FixedMargin(5))
            .arrange()
            .align_to(&display_area, horizontal::Center, vertical::Center);

        layout
            .draw(display)
            .map_err(|e| SelectionUiError::DisplayError(e))?;

        // draw selected box
        if let Some(selected_bounds) = layout.inner().get(self.selection) {
            let selected_bounds = selected_bounds.bounding_box();
            Rectangle::new(selected_bounds.top_left, selected_bounds.size)
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
                .draw(display)
                .map_err(|e| SelectionUiError::DisplayError(e))?;

            self.last_bounds = Some(selected_bounds);
        }

        Ok(())
    }
}

pub fn draw_text_center<'a, S>(
    display: &mut Display,
    text: &'a str,
    style: S,
) -> Result<Point, <Display as DrawTarget>::Error>
where
    S: TextRenderer<Color = <Display as DrawTarget>::Color>,
{
    Text::with_alignment(
        text,
        display.bounding_box().center(),
        style,
        Alignment::Center,
    )
    .draw(display)
}
