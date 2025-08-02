#![feature(impl_trait_in_assoc_type)]
#![feature(ascii_char)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

mod abi;
mod display;
mod peripherals;
mod scsi;
mod storage;
mod ui;
mod usb;
mod utils;

use crate::{
    display::{display_handler, init_display},
    peripherals::{
        conf_peripherals,
        keyboard::{KeyCode, KeyState, read_keyboard_fifo},
    },
    storage::{SDCARD, SdCard},
    usb::{ENABLE_SCSI, usb_handler},
};

use defmt::unwrap;
use elf_loader::{
    Loader, load_exec,
    mmap::MmapImpl,
    object::{ElfBinary, ElfObject},
};
use static_cell::StaticCell;
use talc::*;

static mut ARENA: [u8; 10000] = [0; 10000];

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ClaimOnOom> =
    Talc::new(unsafe { ClaimOnOom::new(Span::from_array(core::ptr::addr_of!(ARENA).cast_mut())) })
        .lock();

use {defmt_rtt as _, panic_probe as _};

use embassy_executor::{Executor, Spawner};
use embassy_futures::join::join;
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
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard as SdmmcSdCard;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp_usb::InterruptHandler<USB>;
});

static mut CORE1_STACK: Stack<4096> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

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
    let binary_data: &[u8] = include_bytes!("../../example.bin");
    let bin = load_exec!("example", binary_data).unwrap();
    let entry = bin.entry();

    let entry_fn: extern "C" fn() = unsafe { core::mem::transmute(entry) };
    entry_fn(); // jump into user code
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

    let display_fut = {
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
        display_handler(display)
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

    ENABLE_SCSI.store(true, core::sync::atomic::Ordering::Relaxed);
    join(usb_fut, display_fut).await;
}
