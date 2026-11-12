#![no_std]

extern crate alloc;

use embedded_graphics::{
    Drawable,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Dimensions, DrawTarget, Point, Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text, renderer::TextRenderer},
};
use userlib::{
    display::Display,
    get_key,
    keyboard::{KeyCode, KeyState},
};

#[derive(Debug)]
pub enum SelectionUiError<DisplayError> {
    SelectionListEmpty,
    DisplayError(DisplayError),
}

pub struct SelectionUi<'a> {
    selection: usize,
    items: &'a [&'a str],
}

impl<'a> SelectionUi<'a> {
    pub fn new(items: &'a [&'a str]) -> Self {
        Self {
            selection: 0,
            items,
        }
    }

    pub fn run_selection_ui(
        &mut self,
        display: &mut Display,
    ) -> Result<Option<usize>, SelectionUiError<<Display as DrawTarget>::Error>> {
        self.draw(display)?;
        loop {
            let key = get_key();
            if key.state != KeyState::Pressed {
                userlib::sleep(10);
                continue;
            }
            let selection = if key.key == KeyCode::Esc {
                None
            } else if let Some(selection) = self.update(display, key.key)? {
                Some(selection)
            } else {
                continue;
            };
            display
                .clear(Rgb565::BLACK)
                .map_err(SelectionUiError::DisplayError)?;
            return Ok(selection);
        }
    }

    /// updates the display with a new keypress.
    /// returns selection idx if selected
    pub fn update(
        &mut self,
        display: &mut Display,
        key: KeyCode,
    ) -> Result<Option<usize>, SelectionUiError<<Display as DrawTarget>::Error>> {
        if self.items.is_empty() {
            return Err(SelectionUiError::SelectionListEmpty);
        }
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

        display
            .clear(Rgb565::BLACK)
            .map_err(SelectionUiError::DisplayError)?;

        let rows = (display_area.size.height / 25).max(1) as usize;
        let visible = visible_range(self.items.len(), self.selection, rows);
        for (row, index) in visible.enumerate() {
            let top = row as i32 * 25;
            Text::with_alignment(
                self.items[index],
                Point::new(display_area.center().x, top + 20),
                text_style,
                Alignment::Center,
            )
            .draw(display)
            .map_err(SelectionUiError::DisplayError)?;
            if index == self.selection {
                Rectangle::new(Point::new(0, top), Size::new(display_area.size.width, 25))
                    .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
                    .draw(display)
                    .map_err(SelectionUiError::DisplayError)?;
            }
        }

        Ok(())
    }
}

fn visible_range(count: usize, selection: usize, rows: usize) -> core::ops::Range<usize> {
    let start = selection.saturating_sub(rows.saturating_sub(1));
    start..count.min(start.saturating_add(rows))
}

#[cfg(test)]
mod tests {
    use super::visible_range;

    #[test]
    fn selection_stays_inside_bounded_visible_rows() {
        assert_eq!(visible_range(0, 0, 12), 0..0);
        assert_eq!(visible_range(1, 0, 12), 0..1);
        assert_eq!(visible_range(25, 0, 12), 0..12);
        assert_eq!(visible_range(25, 12, 12), 1..13);
        assert_eq!(visible_range(25, 24, 12), 13..25);
    }

    #[test]
    fn empty_selection_rejects_input_before_drawing() {
        let mut selection = super::SelectionUi::new(&[]);
        let mut display = super::Display::take().unwrap();
        assert!(matches!(
            selection.update(&mut display, super::KeyCode::Down),
            Err(super::SelectionUiError::SelectionListEmpty)
        ));
        assert!(matches!(
            selection.update(&mut display, super::KeyCode::Enter),
            Err(super::SelectionUiError::SelectionListEmpty)
        ));
    }
}

pub fn draw_text_center<S>(
    display: &mut Display,
    text: &str,
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
