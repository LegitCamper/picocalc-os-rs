use crate::framebuffer::AtomicFrameBuffer;
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::{Rgb565, RgbColor},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use st7365p_lcd::ST7365P;

type DISPLAY = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

pub static mut FRAMEBUFFER: AtomicFrameBuffer = AtomicFrameBuffer::new();

pub async fn init_display(
    spi: Spi<'static, SPI1, Async>,
    cs: Peri<'static, PIN_13>,
    data: Peri<'static, PIN_14>,
    reset: Peri<'static, PIN_15>,
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
    unsafe { FRAMEBUFFER.draw(&mut display).await.unwrap() }
    display.set_on().await.unwrap();

    display
}

pub fn clear_fb() {
    unsafe {
        FRAMEBUFFER.clear(Rgb565::WHITE).unwrap();
    }
}

#[embassy_executor::task]
pub async fn display_handler(mut display: DISPLAY) {
    loop {
        unsafe {
            FRAMEBUFFER
                .partial_draw_batched(&mut display)
                .await
                .unwrap()
        }

        Timer::after_millis(32).await; // 30 fps
    }
}
