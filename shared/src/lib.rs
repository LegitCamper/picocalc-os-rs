#![feature(ascii_char)]

use embedded_graphics::{
    mono_font::{
        MonoFont, MonoTextStyle,
        ascii::{FONT_6X9, FONT_10X20},
    },
    pixelcolor::{BinaryColor, Rgb565},
    prelude::{Point, WebColors, *},
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyle},
};

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;
const SCREEN_ROWS: usize = 15;
const SCREEN_COLS: usize = 31;
const FONT: MonoFont = FONT_10X20;
const COLOR: Rgb565 = Rgb565::CSS_LAWN_GREEN;

pub struct Cursor {
    x: u16,
    y: u16,
}

impl Cursor {
    fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

pub struct TextBuffer {
    grid: [[Option<char>; SCREEN_COLS]; SCREEN_ROWS],
    cursor: Cursor,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            grid: [[None; SCREEN_COLS]; SCREEN_ROWS],
            cursor: Cursor { x: 0, y: 0 },
        }
    }

    /// writes char at cursor
    pub fn write_char(&mut self, ch: char) {
        for (i, row) in self.grid.iter_mut().enumerate() {
            for (j, col) in row.iter_mut().enumerate() {
                if i as u16 == self.cursor.x && j as u16 == self.cursor.y {
                    *col = Some(ch)
                }
            }
        }
    }

    /// fills text buffer with char
    pub fn fill(&mut self, ch: char) {
        for i in 0..SCREEN_ROWS {
            for j in 0..SCREEN_COLS {
                self.cursor = Cursor::new(i as u16, j as u16);
                self.write_char(ch);
            }
        }
    }

    pub fn scroll_up(&mut self) {
        let (top, bottom) = self.grid.split_at_mut(SCREEN_ROWS - 1);
        for (dest, src) in top.iter_mut().zip(&bottom[1..]) {
            dest.copy_from_slice(src);
        }

        self.grid[SCREEN_ROWS - 1].fill(None);
    }

    pub fn draw<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) {
        let style = MonoTextStyle::new(&FONT, COLOR);

        for (i, row) in self.grid.iter().enumerate() {
            for (j, col) in row[1..].iter().enumerate() {
                if let Some(ch) = col {
                    let pos = Point::new(
                        (j as i32) * FONT.character_size.width as i32,
                        (i as i32) * FONT.character_size.height as i32,
                    );

                    let _ = Text::with_baseline(
                        ch.as_ascii().unwrap().as_str(),
                        pos,
                        style,
                        Baseline::Top,
                    )
                    .draw(target);
                }
            }
        }
    }
}
