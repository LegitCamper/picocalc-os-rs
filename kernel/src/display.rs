use crate::framebuffer::{self, AtomicFrameBuffer};
use embassy_futures::yield_now;
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::ST7365P;

#[cfg(feature = "psram")]
use {
    crate::heap::HEAP,
    core::alloc::{GlobalAlloc, Layout},
    embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565, prelude::RgbColor},
};

#[cfg(feature = "fps")]
pub use framebuffer::fps::{FPS_CANVAS, FPS_COUNTER};

type Display = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

pub static FRAMEBUFFER: Mutex<CriticalSectionRawMutex, Option<AtomicFrameBuffer<'static>>> =
    Mutex::new(None);

async fn init_fb() {
    let mut framebuffer = FRAMEBUFFER.lock().await;
    // SAFETY: initialization runs once before display or userland can access the pixels.
    unsafe {
        #[cfg(feature = "psram")]
        {
            let layout = Layout::array::<u16>(framebuffer::SIZE).unwrap();
            let slab = HEAP.alloc(layout) as *mut u16;
            if slab.is_null() {
                alloc::alloc::handle_alloc_error(layout);
            }
            let buf = core::slice::from_raw_parts_mut(slab, framebuffer::SIZE);

            let mut fb = AtomicFrameBuffer::new(buf);
            fb.clear(Rgb565::BLACK).unwrap();
            *framebuffer = Some(fb);
        }

        #[cfg(not(feature = "psram"))]
        {
            static mut BUF: [u16; framebuffer::SIZE] = [0; framebuffer::SIZE];
            *framebuffer = Some(AtomicFrameBuffer::new(&mut BUF));
        }
    }
}

pub async fn init_display(
    spi: Spi<'static, SPI1, Async>,
    cs: Peri<'static, PIN_13>,
    data: Peri<'static, PIN_14>,
    reset: Peri<'static, PIN_15>,
) -> Display {
    init_fb().await;

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
    if let Some(framebuffer) = FRAMEBUFFER.lock().await.as_mut()
        && framebuffer.draw(&mut display).await.is_err()
    {
        #[cfg(feature = "defmt")]
        defmt::warn!("Initial framebuffer transfer failed");
    }
    display.set_on().await.unwrap();

    display
}

#[embassy_executor::task]
pub async fn display_handler(mut display: Display) {
    use embassy_time::{Instant, Timer};

    // Target ~60 Hz refresh (≈16.67 ms per frame)
    const FRAME_TIME_MS: u64 = 1000 / 60;

    loop {
        let start = Instant::now();

        #[cfg(feature = "fps")]
        // SAFETY: FPS state is used only by this core0 display task after initialization.
        unsafe {
            if FPS_COUNTER.should_draw() {
                FPS_CANVAS.draw_fps().await;
            }
        }

        if let Some(framebuffer) = FRAMEBUFFER.lock().await.as_mut()
            && framebuffer.partial_draw(&mut display).await.is_err()
        {
            #[cfg(feature = "defmt")]
            defmt::warn!("Framebuffer transfer failed; retrying dirty tiles");
        }

        let elapsed = start.elapsed().as_millis();
        if elapsed < FRAME_TIME_MS {
            Timer::after_millis(FRAME_TIME_MS - elapsed).await;
        } else {
            yield_now().await;
        }
    }
}
