#![feature(impl_trait_in_assoc_type)]
#![feature(str_from_raw_parts)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![allow(static_mut_refs)]
#![feature(allocator_api)]
#![feature(slice_ptr_get)]

extern crate alloc;

mod abi;
mod display;
mod elf;
mod framebuffer;
#[allow(unused)]
mod heap;
mod peripherals;
#[allow(unused)]
mod psram;
mod scsi;
mod storage;
mod ui;
mod usb;
mod utils;

#[cfg(feature = "pimoroni2w")]
use crate::{heap::init_qmi_psram_heap, psram::init_psram_qmi};

use crate::{
    abi::{KEY_CACHE, MS_SINCE_LAUNCH},
    display::{FRAMEBUFFER, display_handler, init_display},
    peripherals::{
        conf_peripherals,
        keyboard::{KeyState, read_keyboard_fifo},
    },
    psram::init_psram,
    scsi::MSC_SHUTDOWN,
    storage::{SDCARD, SdCard},
    ui::{SELECTIONS, clear_selection, ui_handler},
};
use abi_sys::EntryFn;
use bumpalo::Bump;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::{Executor, Spawner};
use embassy_futures::{join::join, select::select};
use embassy_rp::{
    Peri,
    gpio::{Input, Level, Output, Pull},
    i2c::{self, I2c},
    multicore::{Stack, spawn_core1},
    peripherals::{
        DMA_CH0, DMA_CH1, DMA_CH3, DMA_CH4, I2C1, PIN_2, PIN_3, PIN_6, PIN_7, PIN_10, PIN_11,
        PIN_12, PIN_13, PIN_14, PIN_15, PIN_16, PIN_17, PIN_18, PIN_19, PIN_20, PIN_21, PIN_22,
        PIO0, SPI0, SPI1, USB, WATCHDOG,
    },
    pio,
    spi::{self, Spi},
    usb as embassy_rp_usb,
    watchdog::{ResetReason, Watchdog},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Delay, Duration, Instant, Ticker, Timer};
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{DrawTarget, RgbColor},
};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard as SdmmcSdCard;
use static_cell::StaticCell;
use talc::*;
use {defmt_rtt as _, panic_probe as _};

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp_usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

static mut CORE1_STACK: Stack<16384> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

#[cfg(not(feature = "pimoroni2w"))]
static mut ARENA: [u8; 250 * 1024] = [0; 250 * 1024];
#[cfg(feature = "pimoroni2w")]
static mut ARENA: [u8; 400 * 1024] = [0; 400 * 1024];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

#[embassy_executor::task]
async fn watchdog_task(mut watchdog: Watchdog) {
    if let Some(reason) = watchdog.reset_reason() {
        let _reason = match reason {
            ResetReason::Forced => "forced",
            ResetReason::TimedOut => "timed out",
        };
        #[cfg(feature = "debug")]
        defmt::error!("Watchdog reset reason: {}", _reason);
    }

    watchdog.start(Duration::from_secs(3));

    let mut ticker = Ticker::every(Duration::from_secs(2));
    loop {
        watchdog.feed();
        ticker.next().await;
    }
}

static ENABLE_UI: AtomicBool = AtomicBool::new(true);
static UI_CHANGE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| spawner.spawn(userland_task()).unwrap());
        },
    );

    let display = Display {
        spi: p.SPI1,
        clk: p.PIN_10,
        mosi: p.PIN_11,
        miso: p.PIN_12,
        dma1: p.DMA_CH0,
        dma2: p.DMA_CH1,
        cs: p.PIN_13,
        data: p.PIN_14,
        reset: p.PIN_15,
    };
    let sd = Sd {
        spi: p.SPI0,
        clk: p.PIN_18,
        mosi: p.PIN_19,
        miso: p.PIN_16,
        cs: p.PIN_17,
        det: p.PIN_22,
    };
    let psram = Psram {
        pio: p.PIO0,
        sclk: p.PIN_21,
        mosi: p.PIN_2,
        miso: p.PIN_3,
        cs: p.PIN_20,
        dma1: p.DMA_CH3,
        dma2: p.DMA_CH4,
    };
    let mcu = Mcu {
        i2c: p.I2C1,
        clk: p.PIN_7,
        data: p.PIN_6,
    };
    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        spawner
            .spawn(kernel_task(
                spawner, p.WATCHDOG, display, sd, psram, mcu, p.USB,
            ))
            .unwrap()
    });
}

// One-slot channel to pass EntryFn from core1
static BINARY_CH: Channel<CriticalSectionRawMutex, (EntryFn, Bump), 1> = Channel::new();

// runs dynamically loaded elf files
#[embassy_executor::task]
async fn userland_task() {
    let recv = BINARY_CH.receiver();
    loop {
        let (entry, _bump) = recv.receive().await;

        // disable kernel ui
        {
            ENABLE_UI.store(false, Ordering::Release);
            UI_CHANGE.signal(());

            clear_selection().await;

            MSC_SHUTDOWN.signal(());
        }

        unsafe { MS_SINCE_LAUNCH = Some(Instant::now()) };
        #[cfg(feature = "defmt")]
        defmt::info!("Executing Binary");
        entry();

        // enable kernel ui
        {
            ENABLE_UI.store(true, Ordering::Release);
            UI_CHANGE.signal(());
            unsafe { FRAMEBUFFER.as_mut().unwrap().clear(Rgb565::BLACK).unwrap() };

            let mut selections = SELECTIONS.lock().await;
            selections.set_changed(true);
        }
    }
}

struct Display {
    spi: Peri<'static, SPI1>,
    clk: Peri<'static, PIN_10>,
    mosi: Peri<'static, PIN_11>,
    miso: Peri<'static, PIN_12>,
    dma1: Peri<'static, DMA_CH0>,
    dma2: Peri<'static, DMA_CH1>,
    cs: Peri<'static, PIN_13>,
    data: Peri<'static, PIN_14>,
    reset: Peri<'static, PIN_15>,
}
struct Sd {
    spi: Peri<'static, SPI0>,
    clk: Peri<'static, PIN_18>,
    mosi: Peri<'static, PIN_19>,
    miso: Peri<'static, PIN_16>,
    cs: Peri<'static, PIN_17>,
    det: Peri<'static, PIN_22>,
}
#[allow(dead_code)]
struct Psram {
    pio: Peri<'static, PIO0>,
    sclk: Peri<'static, PIN_21>,
    mosi: Peri<'static, PIN_2>,
    miso: Peri<'static, PIN_3>,
    cs: Peri<'static, PIN_20>,
    dma1: Peri<'static, DMA_CH3>,
    dma2: Peri<'static, DMA_CH4>,
}
struct Mcu {
    i2c: Peri<'static, I2C1>,
    clk: Peri<'static, PIN_7>,
    data: Peri<'static, PIN_6>,
}

async fn setup_mcu(mcu: Mcu) {
    // MCU i2c bus for peripherals( keyboard)
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    let i2c1 = I2c::new_async(mcu.i2c, mcu.clk, mcu.data, Irqs, config);
    conf_peripherals(i2c1).await;
}

async fn setup_display(display: Display, spawner: Spawner) {
    let mut config = spi::Config::default();
    config.frequency = 64_000_000;
    let spi = Spi::new(
        display.spi,
        display.clk,
        display.mosi,
        display.miso,
        display.dma1,
        display.dma2,
        config,
    );
    let display = init_display(spi, display.cs, display.data, display.reset).await;
    spawner.spawn(display_handler(display)).unwrap();
}

// psram is kind of useless on the pico calc
// ive opted to use the pimoroni with on onboard xip psram instead
async fn setup_psram(psram: Psram) {
    let psram = init_psram(
        psram.pio, psram.sclk, psram.mosi, psram.miso, psram.cs, psram.dma1, psram.dma2,
    )
    .await;

    #[cfg(feature = "defmt")]
    defmt::info!("psram size: {}", psram.size);

    if psram.size == 0 {
        #[cfg(feature = "defmt")]
        defmt::info!("\u{1b}[1mExternal PSRAM was NOT found!\u{1b}[0m");
    }
}

#[cfg(feature = "pimoroni2w")]
async fn setup_qmi_psram() {
    Timer::after_millis(250).await;
    let psram_qmi_size = init_psram_qmi(&embassy_rp::pac::QMI, &embassy_rp::pac::XIP_CTRL);
    #[cfg(feature = "debug")]
    defmt::info!("size:  {}", psram_qmi_size);
    Timer::after_millis(100).await;

    if psram_qmi_size > 0 {
        init_qmi_psram_heap(psram_qmi_size);
        return;
    } else {
        panic!("qmi psram not initialized");
    }
}

async fn setup_sd(sd: Sd) {
    let mut config = spi::Config::default();
    config.frequency = 400_000;
    let spi = Spi::new_blocking(sd.spi, sd.clk, sd.mosi, sd.miso, config.clone());
    let cs = Output::new(sd.cs, Level::High);
    let det = Input::new(sd.det, Pull::None);

    let device = ExclusiveDevice::new(spi, cs, Delay).unwrap();
    let sdcard = SdmmcSdCard::new(device, Delay);

    config.frequency = 32_000_000;
    sdcard.spi(|dev| dev.bus_mut().set_config(&config));
    SDCARD.get().lock().await.replace(SdCard::new(sdcard, det));
}

#[embassy_executor::task]
async fn kernel_task(
    spawner: Spawner,
    watchdog: Peri<'static, WATCHDOG>,
    display: Display,
    sd: Sd,
    psram: Psram,
    mcu: Mcu,
    usb: Peri<'static, USB>,
) {
    spawner
        .spawn(watchdog_task(Watchdog::new(watchdog)))
        .unwrap();

    setup_mcu(mcu).await;

    // setup_psram(psram).await;
    #[cfg(feature = "pimoroni2w")]
    setup_qmi_psram().await;

    setup_display(display, spawner).await;
    setup_sd(sd).await;

    let _usb = embassy_rp_usb::Driver::new(usb, Irqs);
    // spawner.spawn(usb_handler(usb)).unwrap();

    loop {
        let ui_enabled = ENABLE_UI.load(Ordering::Relaxed);
        if ui_enabled {
            select(join(ui_handler(), prog_search_handler()), UI_CHANGE.wait()).await;
        } else {
            select(key_handler(), UI_CHANGE.wait()).await;
        }
    }
}

async fn prog_search_handler() {
    loop {
        {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().unwrap();

            let files = sd.list_files_by_extension(".bin").unwrap();
            let mut select = SELECTIONS.lock().await;

            if *select.selections() != files {
                select.update_selections(files);
                select.reset();
            }
        }
        Timer::after_secs(5).await;
    }
}

async fn key_handler() {
    loop {
        if let Some(event) = read_keyboard_fifo().await {
            if let KeyState::Pressed = event.state {
                unsafe {
                    let _ = KEY_CACHE.enqueue(event);
                }
            }
        }
        Timer::after_millis(50).await;
    }
}
