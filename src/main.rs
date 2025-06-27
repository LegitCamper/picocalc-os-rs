#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt::*;
use {defmt_rtt as _, panic_probe as _};

extern crate alloc;

use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C1, PIO1};
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output},
    i2c,
    i2c::I2c,
    spi,
};
use embassy_rp::{pio, spi::Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{File, SdCard, ShortFileName, VolumeIdx, VolumeManager};
use static_cell::StaticCell;

mod peripherals;
use peripherals::{keyboard::KeyEvent, peripherals_task};
mod display;
use display::display_task;
mod heap;
use heap::{HEAP, init_heap};
mod psram;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
});

const MAX_SPI_FREQ: u32 = 62_500_000;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    init_heap();

    static KEYBOARD_EVENTS: StaticCell<Channel<NoopRawMutex, KeyEvent, 10>> = StaticCell::new();
    let keyboard_events = KEYBOARD_EVENTS.init(Channel::new());

    // configure keyboard event handler
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    spawner
        .spawn(peripherals_task(i2c1, keyboard_events.sender()))
        .unwrap();

    // configure display handler
    let mut config = spi::Config::default();
    config.frequency = MAX_SPI_FREQ;
    let spi1 = spi::Spi::new_blocking(p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, config);
    spawner
        .spawn(display_task(spi1, p.PIN_13, p.PIN_14, p.PIN_15))
        .unwrap();
}
