use crate::display::{SCREEN_HEIGHT, SCREEN_WIDTH};
#[cfg(feature = "psram")]
use {
    core::ops::Range,
    embedded_hal_2::digital::OutputPin,
    embedded_hal_async::{delay::DelayNs, spi::SpiDevice},
    st7365p_lcd::ST7365P,
};

pub const SIZE: usize = SCREEN_HEIGHT * SCREEN_WIDTH;
pub type FrameBuffer<'a> =
    st7365p_lcd::FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT, SIZE, &'a mut [u16]>;

/// Uploads full-width snapshot rows without borrowing the live framebuffer.
#[cfg(feature = "psram")]
pub async fn upload_rows<SPI, DC, RST, DELAY: DelayNs>(
    display: &mut ST7365P<SPI, DC, RST, DELAY>,
    pixels: &[u16],
    rows: Range<usize>,
) -> Result<(), ()>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
{
    if rows.start >= rows.end || rows.end > SCREEN_HEIGHT || pixels.len() != SIZE {
        return Err(());
    }
    display
        .set_pixels_buffered(
            0,
            rows.start as u16,
            SCREEN_WIDTH as u16 - 1,
            rows.end as u16 - 1,
            &pixels[rows.start * SCREEN_WIDTH..rows.end * SCREEN_WIDTH],
        )
        .await?;
    Ok(())
}

#[cfg(feature = "fps")]
pub mod fps {
    use crate::display::SCREEN_WIDTH;
    use core::fmt::Write;
    use embedded_graphics::{
        Drawable, Pixel,
        draw_target::DrawTarget,
        geometry::Point,
        mono_font::{MonoTextStyle, ascii::FONT_8X13},
        pixelcolor::Rgb565,
        prelude::{IntoStorage, OriginDimensions, RgbColor, Size},
        text::{Alignment, Text},
    };

    const FPS_LEN: usize = 14;
    pub const FPS_CANVAS_WIDTH: usize = FONT_8X13.character_size.width as usize * FPS_LEN;
    pub const FPS_CANVAS_HEIGHT: usize = FONT_8X13.character_size.height as usize;

    pub const FPS_CANVAS_X: usize = SCREEN_WIDTH - FPS_CANVAS_WIDTH;
    pub const FPS_CANVAS_Y: usize = 0;

    pub struct FpsCanvas {
        pub canvas: [u16; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
    }

    impl FpsCanvas {
        pub fn new() -> Self {
            let mut canvas = Self {
                canvas: [0; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
            };
            canvas.draw_fps(0);
            canvas
        }

        #[cfg(feature = "psram")]
        pub fn composite(&self, pixels: &mut [u16], rows: core::ops::Range<usize>) {
            for row in rows.start.max(FPS_CANVAS_Y)..rows.end.min(FPS_CANVAS_Y + FPS_CANVAS_HEIGHT)
            {
                let start = row * SCREEN_WIDTH + FPS_CANVAS_X;
                let source = (row - FPS_CANVAS_Y) * FPS_CANVAS_WIDTH;
                pixels[start..start + FPS_CANVAS_WIDTH]
                    .copy_from_slice(&self.canvas[source..source + FPS_CANVAS_WIDTH]);
            }
        }

        pub fn draw_fps(&mut self, tenths: u32) {
            let mut buf: heapless::String<FPS_LEN> = heapless::String::new();
            let tenths = tenths.min(9_999);
            let _ = write!(buf, "LCD FPS: {}.{}", tenths / 10, tenths % 10);

            self.canvas.fill(0);
            let text_style = MonoTextStyle::new(&FONT_8X13, Rgb565::WHITE);
            let _ = Text::with_alignment(
                buf.as_str(),
                Point::new(
                    FPS_CANVAS_WIDTH as i32 / 2,
                    (FPS_CANVAS_HEIGHT as i32 + 8) / 2,
                ),
                text_style,
                Alignment::Center,
            )
            .draw(self);
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
                self.canvas[index] = color.into_storage().swap_bytes();
            }
            Ok(())
        }
    }

    impl OriginDimensions for FpsCanvas {
        fn size(&self) -> Size {
            Size::new(FPS_CANVAS_WIDTH as u32, FPS_CANVAS_HEIGHT as u32)
        }
    }
}
