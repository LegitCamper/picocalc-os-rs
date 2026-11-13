use crate::display::{SCREEN_HEIGHT, SCREEN_WIDTH};

pub const SIZE: usize = SCREEN_HEIGHT * SCREEN_WIDTH;
pub type FrameBuffer<'a> =
    st7365p_lcd::FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT, SIZE, &'a mut [u16]>;

#[cfg(feature = "fps")]
pub mod fps {
    use crate::display::SCREEN_WIDTH;
    use core::fmt::Write;
    use embassy_time::{Duration, Instant};
    use embedded_graphics::{
        Drawable, Pixel,
        draw_target::DrawTarget,
        geometry::Point,
        mono_font::{MonoTextStyle, ascii::FONT_8X13},
        pixelcolor::{Rgb565, raw::RawU16},
        prelude::{IntoStorage, OriginDimensions, RgbColor, Size},
        primitives::Rectangle,
        text::{Alignment, Text},
    };

    pub static mut FPS_COUNTER: FpsCounter = FpsCounter::new();
    pub static mut FPS_CANVAS: FpsCanvas = FpsCanvas::new();

    pub fn draw_into(framebuffer: &mut super::FrameBuffer<'_>) {
        // SAFETY: only the core0 display task accesses FPS state.
        let canvas = unsafe { &FPS_CANVAS.canvas };
        let area = Rectangle::new(
            Point::new(FPS_CANVAS_X as i32, FPS_CANVAS_Y as i32),
            Size::new(FPS_CANVAS_WIDTH as u32, FPS_CANVAS_HEIGHT as u32),
        );
        let _ = framebuffer.fill_contiguous(
            &area,
            canvas.iter().map(|pixel| Rgb565::from(RawU16::new(*pixel))),
        );
    }

    // "FPS: 120" = 8 len
    const FPS_LEN: usize = 8;
    pub const FPS_CANVAS_WIDTH: usize = (FONT_8X13.character_size.width + 4) as usize * FPS_LEN;
    pub const FPS_CANVAS_HEIGHT: usize = FONT_8X13.character_size.height as usize;

    // puts canvas in the top right of the display
    // top left point of canvas
    pub const FPS_CANVAS_X: usize = SCREEN_WIDTH - FPS_CANVAS_WIDTH;
    pub const FPS_CANVAS_Y: usize = 0;

    pub struct FpsCanvas {
        pub canvas: [u16; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
    }

    impl FpsCanvas {
        const fn new() -> Self {
            Self {
                canvas: [0; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
            }
        }

        fn clear(&mut self) {
            for p in &mut self.canvas {
                *p = 0;
            }
        }

        pub async fn draw_fps(&mut self) {
            let mut buf: heapless::String<FPS_LEN> = heapless::String::new();
            // SAFETY: only the core0 display task accesses FPS state.
            let fps = unsafe { FPS_COUNTER.smoothed };
            let _ = write!(buf, "FPS: {}", fps as u8);

            self.clear();
            let text_style = MonoTextStyle::new(&FONT_8X13, Rgb565::WHITE);
            Text::with_alignment(
                buf.as_str(),
                Point::new(
                    FPS_CANVAS_WIDTH as i32 / 2,
                    (FPS_CANVAS_HEIGHT as i32 + 8) / 2,
                ),
                text_style,
                Alignment::Center,
            )
            .draw(self)
            .unwrap();
        }
    }

    impl DrawTarget for FpsCanvas {
        type Error = ();
        type Color = Rgb565;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(point, color) in pixels {
                if point.x < 0
                    || point.x >= FPS_CANVAS_WIDTH as i32
                    || point.y < 0
                    || point.y >= FPS_CANVAS_HEIGHT as i32
                {
                    continue;
                }

                let index = (point.y as usize) * FPS_CANVAS_WIDTH + point.x as usize;
                self.canvas[index] = color.into_storage();
            }
            Ok(())
        }
    }

    impl OriginDimensions for FpsCanvas {
        fn size(&self) -> Size {
            Size::new(FPS_CANVAS_WIDTH as u32, FPS_CANVAS_HEIGHT as u32)
        }
    }

    pub struct FpsCounter {
        last_frame: Option<Instant>,
        smoothed: f32,
        last_draw: Option<Instant>,
    }

    impl FpsCounter {
        pub const fn new() -> Self {
            Self {
                last_frame: None,
                smoothed: 0.0,
                last_draw: None,
            }
        }

        // Is called once per frame or partial frame to update FPS
        pub fn measure(&mut self) {
            let now = Instant::now();

            if let Some(last) = self.last_frame {
                let dt_us = (now - last).as_micros() as f32;
                if dt_us > 0.0 {
                    let current = 1_000_000.0 / dt_us;
                    self.smoothed = if self.smoothed == 0.0 {
                        current
                    } else {
                        0.9 * self.smoothed + 0.1 * current
                    };
                }
            }

            self.last_frame = Some(now);
        }

        pub fn should_draw(&mut self) -> bool {
            let now = Instant::now();
            match self.last_draw {
                Some(last) if now - last < Duration::from_millis(200) => false,
                _ => {
                    self.last_draw = Some(now);
                    true
                }
            }
        }
    }
}
