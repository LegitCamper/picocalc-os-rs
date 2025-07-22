#![feature(impl_trait_in_assoc_type)]
#![feature(ascii_char)]
#![no_std]
#![no_main]

use crate::{
    display::DISPLAY_SIGNAL,
    peripherals::keyboard::{KeyCode, KeyState, read_keyboard_fifo},
    storage::SdCard,
    usb::usb_handler,
};

use {defmt_rtt as _, panic_probe as _};

use core::cell::RefCell;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::{
    gpio::{Input, Level, Output, Pull},
    peripherals::{I2C1, USB},
    spi::Spi,
    usb as embassy_rp_usb,
};
use embassy_rp::{i2c, i2c::I2c, spi};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Timer};
use embedded_graphics::primitives::Rectangle;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::SdCard as SdmmcSdCard;
use heapless::String;

mod peripherals;
use peripherals::conf_peripherals;
mod display;
use display::display_handler;
mod scsi;
mod storage;
mod usb;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp_usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // MCU i2c bus for peripherals
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    conf_peripherals(i2c1).await;

    // SPI1 bus display
    let mut config = spi::Config::default();
    config.frequency = 16_000_000;
    let spi1 = spi::Spi::new(
        p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, config,
    );

    let usb = embassy_rp_usb::Driver::new(p.USB, Irqs);

    let sdcard = {
        let mut config = spi::Config::default();
        config.frequency = 400_000;
        let spi = Spi::new(
            p.SPI0,
            p.PIN_18,
            p.PIN_19,
            p.PIN_16,
            p.DMA_CH2,
            p.DMA_CH3,
            config.clone(),
        );
        let cs = Output::new(p.PIN_5, Level::High);

        let device = ExclusiveDevice::new(spi, cs, Delay).unwrap();
        let sdcard = SdmmcSdCard::new(device, Delay);

        config.frequency = 32_000_000;
        sdcard.spi(|dev| dev.bus_mut().set_config(&config));
        SdCard::new(sdcard, Input::new(p.PIN_22, Pull::None))
    };

    usb_handler(usb, sdcard).await;
}
