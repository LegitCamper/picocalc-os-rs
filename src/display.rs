use defmt::info;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    Drawable,
    mono_font::{MonoFont, MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Point, WebColors},
    text::{Baseline, Text, TextStyle},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};

type SPI = Spi<'static, SPI1, Async>;

type FRAMEBUFFER = FrameBuffer<
    SCREEN_WIDTH,
    SCREEN_HEIGHT,
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
>;

const SCREEN_WIDTH: usize = 320;
const SCREEN_HEIGHT: usize = 320;
const SCREEN_ROWS: usize = 15;
const SCREEN_COLS: usize = 31;
const FONT: MonoFont = FONT_10X20;
const COLOR: Rgb565 = Rgb565::CSS_LAWN_GREEN;

#[embassy_executor::task]
pub async fn display_task(spi: SPI, cs: PIN_13, data: PIN_14, reset: PIN_15) {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    );
    let mut framebuffer: FRAMEBUFFER = FrameBuffer::new(display);

    framebuffer.init(&mut Delay).await.unwrap();
    framebuffer.display.set_offset(0, 0);
    framebuffer
        .display
        .set_custom_orientation(0x60)
        .await
        .unwrap();

    let mut textbuffer = TextBuffer::new();
    textbuffer.fill('A');
    textbuffer.draw(&mut framebuffer);
    info!("finished rendering");

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}

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

    pub fn draw(&mut self, target: &mut FRAMEBUFFER) {
        let baseline = TextStyle::with_baseline(Baseline::Top);
        let style = MonoTextStyle::new(&FONT, COLOR);

        for (i, row) in self.grid.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                if let Some(ch) = cell {
                    let pos = Point::new(
                        (j as i32) * FONT.character_size.width as i32,
                        (i as i32) * FONT.character_size.height as i32 + baseline.baseline as i32,
                    );

                    Text::new(ch.as_ascii().unwrap().as_str(), pos, style)
                        .draw(target)
                        .unwrap();
                }
            }
        }
    }
}
