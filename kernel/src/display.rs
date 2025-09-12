use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::{FrameBuffer, ST7365P};
use static_cell::StaticCell;

type DISPLAY = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

type FB = FrameBuffer<SCREEN_WIDTH, SCREEN_HEIGHT, { SCREEN_WIDTH * SCREEN_HEIGHT }>;
static mut FRAMEBUFFER: Option<FB> = None;

pub fn framebuffer_mut() -> &'static mut FB {
    unsafe { FRAMEBUFFER.as_mut().unwrap() }
}

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

    display.init().await.unwrap();
    display.set_custom_orientation(0x40).await.unwrap();
    unsafe { FRAMEBUFFER.replace(FrameBuffer::new()) };
    display.draw(framebuffer_mut()).await.unwrap();
    display.set_on().await.unwrap();

    display
}

pub async fn display_handler(mut display: DISPLAY) {
    let fb = framebuffer_mut();
    loop {
        display.partial_draw_batched(fb).await.unwrap();
        embassy_time::Timer::after_millis(32).await; // 30 fps
    }
}
