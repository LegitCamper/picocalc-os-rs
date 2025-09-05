#![feature(impl_trait_in_assoc_type)]
#![feature(type_alias_impl_trait)]
#![feature(ascii_char)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![allow(static_mut_refs)]

extern crate alloc;

mod abi;
mod display;
mod elf;
mod peripherals;
mod scsi;
mod storage;
mod ui;
mod usb;
mod utils;

use core::sync::atomic::Ordering;

use crate::{
    display::{display_handler, init_display},
    elf::load_binary,
    peripherals::{
        conf_peripherals,
        keyboard::{KeyCode, KeyState, read_keyboard_fifo},
    },
    storage::{SDCARD, SdCard},
    ui::{SELECTIONS, ui_handler},
    usb::usb_handler,
};
use alloc::vec::Vec;

use {defmt_rtt as _, panic_probe as _};

use defmt::unwrap;
use embassy_executor::{Executor, Spawner};
use embassy_futures::join::{join, join3, join4, join5};
use embassy_rp::{
    gpio::{Input, Level, Output, Pull},
    i2c::{self, I2c},
    multicore::{Stack, spawn_core1},
    peripherals::{
        DMA_CH0, DMA_CH1, I2C1, PIN_6, PIN_7, PIN_10, PIN_11, PIN_12, PIN_13, PIN_14, PIN_15,
        PIN_16, PIN_17, PIN_18, PIN_19, PIN_22, SPI0, SPI1, USB,
    },
    spi::{self, Spi},
    usb as embassy_rp_usb,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard as SdmmcSdCard;
use heapless::spsc::Queue;
use shared::keyboard::KeyEvent;
use static_cell::StaticCell;
use talc::*;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp_usb::InterruptHandler<USB>;
});

static mut CORE1_STACK: Stack<16384> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

static mut ARENA: [u8; 10000] = [0; 10000];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

static TASK_STATE: Mutex<CriticalSectionRawMutex, TaskState> = Mutex::new(TaskState::Ui);

enum TaskState {
    Ui,
    Kernel,
}

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
    let mcu = Mcu {
        i2c: p.I2C1,
        clk: p.PIN_7,
        data: p.PIN_6,
    };
    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| unwrap!(spawner.spawn(kernel_task(display, sd, mcu, p.USB))));
}

// runs dynamically loaded elf files
#[embassy_executor::task]
async fn userland_task() {
    // DRIVERS_READY.wait().await;

    // defmt::info!("Loading binary");
    // let binary_data: &[u8] =
    //     include_bytes!("../../target/thumbv8m.main-none-eabihf/release/calculator");

    // defmt::info!("Running binary");
    // let entry = unsafe { load_binary(binary_data).unwrap() };

    // entry().await;
}

struct Display {
    spi: SPI1,
    clk: PIN_10,
    mosi: PIN_11,
    miso: PIN_12,
    dma1: DMA_CH0,
    dma2: DMA_CH1,
    cs: PIN_13,
    data: PIN_14,
    reset: PIN_15,
}

struct Sd {
    spi: SPI0,
    clk: PIN_18,
    mosi: PIN_19,
    miso: PIN_16,
    cs: PIN_17,
    det: PIN_22,
}

struct Mcu {
    i2c: I2C1,
    clk: PIN_7,
    data: PIN_6,
}

#[embassy_executor::task]
async fn kernel_task(display: Display, sd: Sd, mcu: Mcu, usb: USB) {
    // MCU i2c bus for peripherals
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    let i2c1 = I2c::new_async(mcu.i2c, mcu.clk, mcu.data, Irqs, config);
    conf_peripherals(i2c1).await;

    Timer::after_millis(250).await;

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

    let display_fut = display_handler(display);

    let ui_fut = ui_handler();

    let binary_search_fut = async {
        loop {
            {
                let mut guard = SDCARD.get().lock().await;

                if let Some(sd) = guard.as_mut() {
                    let files = sd.list_files_by_extension(".bin").unwrap();
                    let mut select = SELECTIONS.lock().await;

                    if select.selections != files {
                        select.selections = files;
                        select.reset();
                    }
                }
            }
            Timer::after_secs(5).await;
        }
    };

    {
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
    };

    let usb = embassy_rp_usb::Driver::new(usb, Irqs);
    let usb_fut = usb_handler(usb);

    let key_abi_fut = async {
        loop {
            Timer::after_millis(100).await;
            get_keys().await
        }
    };

    join4(display_fut, ui_fut, binary_search_fut, key_abi_fut).await;
}

static mut KEY_CACHE: Queue<KeyEvent, 32> = Queue::new();

async fn get_keys() {
    if let Some(event) = read_keyboard_fifo().await {
        unsafe {
            let _ = KEY_CACHE.enqueue(event);
        }
    }
}
