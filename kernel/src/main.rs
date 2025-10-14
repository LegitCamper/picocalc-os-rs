#![feature(impl_trait_in_assoc_type)]
#![feature(str_from_raw_parts)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![allow(static_mut_refs)]

extern crate alloc;

mod abi;
mod display;
mod elf;
mod framebuffer;
mod peripherals;
mod psram;
mod scsi;
mod storage;
mod ui;
mod usb;
mod utils;

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
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{DrawTarget, RgbColor},
};

use {defmt_rtt as _, panic_probe as _};

use core::sync::atomic::{AtomicBool, Ordering};
use defmt::unwrap;
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
        PIO0, SPI0, SPI1, USB,
    },
    pio,
    spi::{self, Spi},
    usb as embassy_rp_usb,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Delay, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard as SdmmcSdCard;
use static_cell::StaticCell;
use talc::*;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp_usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

static mut CORE1_STACK: Stack<16384> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

static mut ARENA: [u8; 200 * 1024] = [0; 200 * 1024];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

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
            executor1.run(|spawner| unwrap!(spawner.spawn(userland_task())));
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
        unwrap!(spawner.spawn(kernel_task(spawner, display, sd, psram, mcu, p.USB)))
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
        defmt::info!("Executing Binary");
        entry();

        // enable kernel ui
        {
            ENABLE_UI.store(true, Ordering::Release);
            UI_CHANGE.signal(());
            unsafe { FRAMEBUFFER.clear(Rgb565::BLACK).unwrap() };

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
    config.frequency = 16_000_000;
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

async fn setup_psram(psram: Psram) {
    let psram = init_psram(
        psram.pio, psram.sclk, psram.mosi, psram.miso, psram.cs, psram.dma1, psram.dma2,
    )
    .await;

    defmt::info!("psram size: {}", psram.size);

    if psram.size == 0 {
        defmt::info!("\u{1b}[1mExternal PSRAM was NOT found!\u{1b}[0m");
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
    display: Display,
    sd: Sd,
    psram: Psram,
    mcu: Mcu,
    usb: Peri<'static, USB>,
) {
    setup_mcu(mcu).await;
    Timer::after_millis(250).await;
    setup_display(display, spawner).await;
    setup_psram(psram).await;
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
