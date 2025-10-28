use crate::framebuffer::{self, AtomicFrameBuffer};
use alloc::vec;
use core::alloc::GlobalAlloc;
use core::alloc::Layout;
#[cfg(feature = "pimoroni2w")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use embassy_rp::{
    Peri,
    gpio::{Level, Output},
    peripherals::{PIN_13, PIN_14, PIN_15, SPI1},
    spi::{Async, Spi},
};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use once_cell::unsync::Lazy;
use st7365p_lcd::ST7365P;

type DISPLAY = ST7365P<
    ExclusiveDevice<Spi<'static, SPI1, Async>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Delay,
>;

pub const SCREEN_WIDTH: usize = 320;
pub const SCREEN_HEIGHT: usize = 320;

// used to switch which fb is being used for read/write
#[cfg(feature = "pimoroni2w")]
pub static WRITE_FRAMEBUFFER2: AtomicBool = AtomicBool::new(false);

// the default screen buffer - the only one on boards without extra ram or psram
pub static mut FRAMEBUFFER: Lazy<AtomicFrameBuffer> = Lazy::new(|| {
    static mut BUF: [u16; framebuffer::SIZE] = [0; framebuffer::SIZE];
    AtomicFrameBuffer::new(unsafe { &mut BUF })
});

#[cfg(feature = "pimoroni2w")]
pub static mut FRAMEBUFFER2: Lazy<AtomicFrameBuffer> = Lazy::new(|| {
    use embedded_graphics::{
        pixelcolor::Rgb565,
        prelude::{DrawTarget, RgbColor},
    };

    let buf = unsafe {
        let slab =
            crate::heap::HEAP.alloc(Layout::array::<u16>(framebuffer::SIZE).unwrap()) as *mut u16;
        core::slice::from_raw_parts_mut(slab, framebuffer::SIZE)
    };
    let mut fb = AtomicFrameBuffer::new(buf);
    fb.set_all_tiles(false);
    fb.clear(Rgb565::BLACK).unwrap();
    fb
});

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

#[embassy_executor::task]
pub async fn display_handler(mut display: DISPLAY) {
    loop {
        let fb = if cfg!(feature = "pimoroni2w") {
            if WRITE_FRAMEBUFFER2.load(Ordering::Acquire) {
                unsafe { &mut FRAMEBUFFER }
            } else {
                unsafe { &mut FRAMEBUFFER2 }
            }
        } else {
            unsafe { &mut FRAMEBUFFER }
        };
        fb.safe_draw(&mut display).await.unwrap();

        // small yield to allow other tasks to run
        Timer::after_nanos(100).await;
    }
}
