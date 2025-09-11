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
static FRAMEBUFFER_CELL: StaticCell<FB> = StaticCell::new();
static FRAMEBUFFER: Mutex<CriticalSectionRawMutex, Option<&'static mut FB>> = Mutex::new(None);

pub fn access_framebuffer_blocking(mut access: impl FnMut(&mut FB)) -> Result<(), ()> {
    let mut guard = FRAMEBUFFER.try_lock().ok().ok_or(())?;
    let fb = guard.as_mut().ok_or(())?;
    access(fb);
    Ok(())
}

pub async fn access_framebuffer(mut access: impl FnMut(&mut FB)) -> Result<(), ()> {
    let mut guard = FRAMEBUFFER.lock().await;
    let fb: Option<&mut &'static mut FB> = guard.as_mut();
    if let Some(fb) = fb {
        access(&mut *fb);
        return Ok(());
    }
    Err(())
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
    let mut framebuffer = FRAMEBUFFER_CELL.init(FrameBuffer::new());
    display.draw(&mut framebuffer).await.unwrap();
    display.set_on().await.unwrap();
    FRAMEBUFFER.lock().await.replace(framebuffer);

    display
}

static DISPLAYREF: StaticCell<DISPLAY> = StaticCell::new();

pub async fn display_handler(mut display: DISPLAY) {
    let mut guard = FRAMEBUFFER.lock().await;
    if let Some(fb) = guard.as_mut() {
        display.partial_draw_batched(fb).await.unwrap();
    }
}
