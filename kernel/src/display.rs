use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, lazy_lock::LazyLock, mutex::Mutex,
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::{Rgb565, RgbColor},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};

type DISPLAY = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

type FB = FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT, { SCREEN_WIDTH * SCREEN_HEIGHT }>;
pub static FRAMEBUFFER: LazyLock<Mutex<CriticalSectionRawMutex, FB>> =
    LazyLock::new(|| Mutex::new(FrameBuffer::new()));

pub async fn init_display(
    spi: Spi<'static, SPI1, Async>,
    cs: PIN_13,
    data: PIN_14,
    reset: PIN_15,
) -> DISPLAY {
    let spi_device = ExclusiveDevice::new(spi, Output::new(cs, Level::Low), Delay).unwrap();
    let mut display = ST7365P::new(
        spi_device,
        Output::new(data, Level::Low),
        Some(Output::new(reset, Level::High)),
        false,
        true,
        Delay,
    );
    let mut fb = FRAMEBUFFER.get().lock().await;
    display.init().await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap();
    display.draw(&mut fb).await.unwrap();
    display.set_on().await.unwrap();

    display
}

pub async fn clear_fb() {
    let mut fb = FRAMEBUFFER.get().lock().await;
    let fb = &mut *fb;
    fb.clear(Rgb565::BLACK).unwrap();
}

pub async fn display_handler(mut display: DISPLAY) {
    loop {
        {
            let mut fb = FRAMEBUFFER.get().lock().await;
            display.partial_draw_batched(&mut fb).await.unwrap();
        }

        Timer::after_millis(32).await; // 30 fps
    }
}
