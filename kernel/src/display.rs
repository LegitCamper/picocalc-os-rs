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

#[cfg(feature = "psram")]
use crate::heap::HEAP;

#[cfg(feature = "fps")]
pub use framebuffer::fps::{FPS_CANVAS, FPS_COUNTER};

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
        #[cfg(feature = "psram")]
        {
            let slab = HEAP.alloc(Layout::array::<u16>(framebuffer::SIZE).unwrap()) as *mut u16;
            let buf = core::slice::from_raw_parts_mut(slab, framebuffer::SIZE);

            let mut fb = AtomicFrameBuffer::new(buf);
            fb.clear(Rgb565::BLACK).unwrap();
            FRAMEBUFFER = Some(fb);
        }

        #[cfg(not(feature = "psram"))]
        {
            static mut BUF: [u16; framebuffer::SIZE] = [0; framebuffer::SIZE];
            FRAMEBUFFER = Some(AtomicFrameBuffer::new(&mut BUF));
        }
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
        // renders fps text to canvas
        #[cfg(feature = "fps")]
        unsafe {
            if FPS_COUNTER.should_draw() {
                FPS_CANVAS.draw_fps().await;
            }
        }

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

        // small yield to allow other tasks to run
        Timer::after_millis(10).await;
    }
}
