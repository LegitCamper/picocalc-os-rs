#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::I2C1;
use embassy_rp::spi::{self, Spi};
use embassy_rp::{bind_interrupts, i2c};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{File, SdCard, ShortFileName, VolumeIdx, VolumeManager};
use static_cell::StaticCell;

mod peripherals;
use peripherals::{keyboard::KeyEvent, peripherals_task};

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    static KEYBOARD_EVENTS: StaticCell<Channel<NoopRawMutex, KeyEvent, 10>> = StaticCell::new();
    let keyboard_events = KEYBOARD_EVENTS.init(Channel::new());

    // configure keyboard event handler
    let config = embassy_rp::i2c::Config::default();
    let bus = embassy_rp::i2c::I2c::new_async(p.I2C1, p.PIN_27, p.PIN_26, Irqs, config);
    spawner
        .spawn(peripherals_task(bus, keyboard_events.sender()))
        .unwrap();
}
