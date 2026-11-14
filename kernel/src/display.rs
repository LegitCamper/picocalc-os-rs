use crate::framebuffer::{self, FrameBuffer};
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

pub static FRAMEBUFFER: Mutex<CriticalSectionRawMutex, Option<FrameBuffer<'static>>> =
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

            let mut fb = FrameBuffer::with_storage(buf);
            fb.clear(Rgb565::BLACK).unwrap();
            *framebuffer = Some(fb);
        }

        #[cfg(not(feature = "psram"))]
        {
            static mut BUF: [u16; framebuffer::SIZE] = [0; framebuffer::SIZE];
            *framebuffer = Some(FrameBuffer::with_storage(&mut BUF));
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
pub async fn display_handler(mut display: Display, write_config: embassy_rp::spi::Config) {
    use embassy_time::{Duration, Instant, Timer};
    use userlib_sys::framebuffer::scan_band_delay_us;

    let timing = calibrate_scan(&mut display, write_config.frequency).await;
    #[cfg(feature = "defmt")]
    defmt::info!("LCD scan timing={:?}; phase wait capped at 500 us", timing);

    #[cfg(feature = "psram")]
    let staging = alloc_staging();
    let mut next_flush = Instant::now();
    let mut _last_report = Instant::now();
    let mut _transfers = 0u32;
    let mut _scan_failures = 0u32;
    let mut _upload_failures = 0u32;
    let mut _max_snapshot_us = 0u64;
    let mut _max_upload_us = 0u64;
    let mut _max_scan_us = 0u64;
    loop {
        Timer::at(next_flush).await;
        #[cfg(feature = "fps")]
        // SAFETY: FPS state is used only by this core0 display task after initialization.
        unsafe {
            if FPS_COUNTER.should_draw() {
                FPS_CANVAS.draw_fps().await;
            }
        }
        let mut guard = FRAMEBUFFER.lock().await;
        let _snapshot_start = Instant::now();
        let Some(framebuffer) = guard.as_mut() else {
            drop(guard);
            Timer::after_millis(1).await;
            continue;
        };
        #[cfg(feature = "fps")]
        framebuffer::draw_fps_into_fb(framebuffer);
        #[cfg(feature = "psram")]
        let rows = match framebuffer.take_dirty_rows(staging) {
            Ok(rows) => rows,
            Err(()) => {
                #[cfg(feature = "defmt")]
                defmt::warn!("Invalid framebuffer staging size");
                None
            }
        };
        // ponytail: no room for a staging copy in SRAM; lock is held through upload.
        #[cfg(not(feature = "psram"))]
        let rows = framebuffer.dirty_rows();
        #[cfg(feature = "psram")]
        drop(guard);
        _max_snapshot_us = _max_snapshot_us.max(_snapshot_start.elapsed().as_micros());
        let Some(rows) = rows else {
            #[cfg(not(feature = "psram"))]
            drop(guard);
            Timer::after_millis(1).await;
            continue;
        };
        next_flush = Instant::now() + Duration::from_micros(16_667);
        let scan_start = Instant::now();
        let delay_us = if let Some((frame_us, counter_period)) = timing {
            display.spi_mut().bus_mut().set_frequency(6_000_000);
            let scanline = display.read_scanline().await;
            display
                .spi_mut()
                .bus_mut()
                .set_frequency(write_config.frequency);
            match scanline {
                Ok(scanline) => {
                    scan_band_delay_us(frame_us, counter_period, scanline, rows.clone())
                }
                Err(_) => {
                    _scan_failures = _scan_failures.saturating_add(1);
                    0
                }
            }
        } else {
            0
        };
        _max_scan_us = _max_scan_us.max(scan_start.elapsed().as_micros());
        if delay_us != 0 {
            Timer::at(scan_start + Duration::from_micros(delay_us)).await;
        }
        let upload_start = Instant::now();
        #[cfg(feature = "psram")]
        let result = framebuffer::upload_rows(&mut display, staging, rows.clone()).await;
        #[cfg(not(feature = "psram"))]
        let result = match guard.as_mut() {
            Some(framebuffer) => framebuffer.partial_draw_batched(&mut display).await,
            None => Err(()),
        };
        #[cfg(not(feature = "psram"))]
        drop(guard);
        let upload_us = upload_start.elapsed().as_micros();
        _max_upload_us = _max_upload_us.max(upload_us);
        if result.is_err() {
            _upload_failures = _upload_failures.saturating_add(1);
            #[cfg(feature = "psram")]
            if let Some(framebuffer) = FRAMEBUFFER.lock().await.as_mut() {
                if framebuffer.retry_rows(rows.clone()).is_err() {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("Invalid framebuffer retry rows");
                }
            }
            Timer::after_millis(1).await;
        } else {
            _transfers = _transfers.saturating_add(1);
            #[cfg(all(feature = "fps", not(feature = "psram")))]
            // SAFETY: only the core0 display task accesses FPS state.
            unsafe {
                FPS_COUNTER.measure();
            }
        }
        if _last_report.elapsed().as_secs() >= 5 {
            #[cfg(feature = "defmt")]
            defmt::info!(
                "LCD band: rows={}..{}, delay_us={}, upload_us={}, transfers={}, scan_failures={}, upload_failures={}, max_snapshot_us={}, max_scan_us={}, max_upload_us={}",
                rows.start,
                rows.end,
                delay_us,
                upload_us,
                _transfers,
                _scan_failures,
                _upload_failures,
                _max_snapshot_us,
                _max_scan_us,
                _max_upload_us
            );
            _last_report = Instant::now();
        }
        yield_now().await;
    }
}

/// Whole-screen staging copy uploaded by the display task. Snapshots are
/// taken under the framebuffer lock; the panel upload runs without it.
#[cfg(feature = "psram")]
fn alloc_staging() -> &'static mut [u16] {
    let layout = Layout::array::<u16>(framebuffer::SIZE).unwrap();
    // SAFETY: called once by the display task; the allocation is never freed or aliased.
    unsafe {
        let slab = HEAP.alloc(layout) as *mut u16;
        if slab.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        slab.write_bytes(0, framebuffer::SIZE);
        core::slice::from_raw_parts_mut(slab, framebuffer::SIZE)
    }
}

async fn calibrate_scan(display: &mut Display, write_frequency: u32) -> Option<(u64, u16)> {
    use embassy_time::{Instant, Timer};

    display.spi_mut().bus_mut().set_frequency(6_000_000);
    let result = async {
        let started = Instant::now();
        let mut previous = display.read_scanline().await.ok()?;
        let mut wrap_at: Option<Instant> = None;
        let mut peak = 0;
        while started.elapsed().as_millis() < 60 {
            Timer::after_micros(50).await;
            let scanline = display.read_scanline().await.ok()?;
            let now = Instant::now();
            if scanline > 483 {
                return None;
            }
            if scanline < previous {
                if let Some(last) = wrap_at {
                    let frame_us = (now - last).as_micros();
                    if (5_000..=100_000).contains(&frame_us) && peak > 1 {
                        return Some((frame_us, peak + 1));
                    }
                    return None;
                }
                wrap_at = Some(now);
                peak = 0;
            }
            peak = peak.max(scanline);
            previous = scanline;
        }
        None
    }
    .await;
    display.spi_mut().bus_mut().set_frequency(write_frequency);
    result
}
