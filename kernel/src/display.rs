use crate::framebuffer::{self, AtomicFrameBuffer, FB_PAUSED};
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::Ordering;
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{DrawTarget, RgbColor},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::ST7365P;

#[cfg(feature = "fps")]
pub use fps::FPS_COUNTER;

type DISPLAY = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

pub static mut FRAMEBUFFER: Option<AtomicFrameBuffer> = None;

fn init_fb() {
    unsafe {
        FRAMEBUFFER = Some(if cfg!(not(feature = "pimoroni2w")) {
            static mut BUF: [u16; framebuffer::SIZE] = [0; framebuffer::SIZE];
            AtomicFrameBuffer::new(&mut BUF)
        } else {
            let slab = crate::heap::HEAP.alloc(Layout::array::<u16>(framebuffer::SIZE).unwrap())
                as *mut u16;
            let buf = core::slice::from_raw_parts_mut(slab, framebuffer::SIZE);

            let mut fb = AtomicFrameBuffer::new(buf);
            fb.clear(Rgb565::BLACK).unwrap();
            fb
        });
    }
}

pub async fn init_display(
    spi: Spi<'static, SPI1, Async>,
    cs: Peri<'static, PIN_13>,
    data: Peri<'static, PIN_14>,
    reset: Peri<'static, PIN_15>,
) -> DISPLAY {
    init_fb();

    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let mut display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        Delay,
    );
    display.init().await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap();
    unsafe {
        FRAMEBUFFER
            .as_mut()
            .unwrap()
            .draw(&mut display)
            .await
            .unwrap()
    }
    display.set_on().await.unwrap();

    display
}

#[embassy_executor::task]
pub async fn display_handler(mut display: DISPLAY) {
    loop {
        if !FB_PAUSED.load(Ordering::Acquire) {
            unsafe {
                FRAMEBUFFER
                    .as_mut()
                    .unwrap()
                    .partial_draw(&mut display)
                    .await
                    .unwrap()
            };
        }

        #[cfg(feature = "fps")]
        if unsafe { FPS_COUNTER.should_draw() } {
            fps::draw_fps(&mut display).await;
        }

        // small yield to allow other tasks to run
        Timer::after_millis(10).await;
    }
}

#[cfg(feature = "fps")]
mod fps {
    use crate::display::{DISPLAY, SCREEN_WIDTH};
    use core::fmt::Write;
    use embassy_time::{Duration, Instant};
    use embedded_graphics::{
        Drawable, Pixel,
        draw_target::DrawTarget,
        geometry::Point,
        mono_font::{MonoTextStyle, ascii::FONT_8X13},
        pixelcolor::Rgb565,
        prelude::{IntoStorage, OriginDimensions, RgbColor, Size},
        text::{Alignment, Text},
    };

    pub static mut FPS_COUNTER: FpsCounter = FpsCounter::new();
    pub static mut FPS_CANVAS: FpsCanvas = FpsCanvas::new();

    pub async fn draw_fps(mut display: &mut DISPLAY) {
        let mut buf: heapless::String<FPS_LEN> = heapless::String::new();
        let fps = unsafe { FPS_COUNTER.smoothed };
        let _ = write!(buf, "FPS: {}", fps as u8);

        unsafe { FPS_CANVAS.clear() };
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
        .draw(unsafe { &mut FPS_CANVAS })
        .unwrap();

        unsafe { FPS_CANVAS.draw(&mut display).await };
    }

    // "FPS: 120" = 8 len
    const FPS_LEN: usize = 8;
    const FPS_CANVAS_WIDTH: usize = (FONT_8X13.character_size.width + 4) as usize * FPS_LEN;
    const FPS_CANVAS_HEIGHT: usize = FONT_8X13.character_size.height as usize;

    pub struct FpsCanvas {
        canvas: [u16; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
        top_left: Point,
    }

    impl FpsCanvas {
        const fn new() -> Self {
            let top_right = Point::new((SCREEN_WIDTH - FPS_CANVAS_WIDTH) as i32, 0);
            Self {
                canvas: [0; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
                top_left: top_right,
            }
        }

        fn clear(&mut self) {
            for p in &mut self.canvas {
                *p = 0;
            }
        }

        async fn draw(&self, display: &mut DISPLAY) {
            let top_left = self.top_left;

            for y in 0..FPS_CANVAS_HEIGHT {
                let row_start = y * FPS_CANVAS_WIDTH;
                let row_end = row_start + FPS_CANVAS_WIDTH;
                let row = &self.canvas[row_start..row_end];

                display
                    .set_pixels_buffered(
                        top_left.x as u16,
                        top_left.y as u16 + y as u16,
                        top_left.x as u16 + FPS_CANVAS_WIDTH as u16 - 1,
                        y as u16,
                        row,
                    )
                    .await
                    .unwrap();
            }
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
