#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use crate::display::{SCREEN_HEIGHT, SCREEN_WIDTH, UI, init_display};
use crate::peripherals::{keyboard::KeyEvent, peripherals_task};
use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C1, SPI1};
use embassy_rp::spi::Spi;
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output},
    i2c,
    i2c::I2c,
    spi,
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Timer};
use embedded_hal_1::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{File, SdCard, ShortFileName, VolumeIdx, VolumeManager};
use st7365p_lcd::{FrameBuffer, ST7365P};
use static_cell::StaticCell;

mod display;
mod peripherals;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    static KEYBOARD_EVENTS: StaticCell<Channel<NoopRawMutex, KeyEvent, 10>> = StaticCell::new();
    let keyboard_events = KEYBOARD_EVENTS.init(Channel::new());

    // configure keyboard event handler
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    spawner
        .spawn(peripherals_task(i2c1, keyboard_events.sender()))
        .unwrap();

    let mut config = spi::Config::default();
    config.frequency = 16_000_000;
    let spi1 = Spi::new(
        p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, config,
    );
    let mut framebuffer = init_display(spi1, p.PIN_13, p.PIN_14, p.PIN_15).await;

    let mut ui: UI<50, 25> = UI::new();
    ui.draw(&mut framebuffer);

    loop {
        framebuffer.draw().await.unwrap();
        Timer::after_millis(500).await;
    }
}
