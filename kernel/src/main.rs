#![feature(impl_trait_in_assoc_type)]
#![feature(ascii_char)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

mod abi;
mod display;
mod peripherals;
mod scsi;
mod storage;
mod usb;
mod utils;

use crate::{
    display::{display_handler, init_display},
    peripherals::{
        conf_peripherals,
        keyboard::{KeyCode, KeyState, read_keyboard_fifo},
    },
    storage::SdCard,
    usb::usb_handler,
};

use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::join::{join, join3};
use embassy_rp::{
    gpio::{Input, Level, Output, Pull},
    peripherals::{I2C1, USB},
    spi::Spi,
    usb as embassy_rp_usb,
};
use embassy_rp::{i2c, i2c::I2c, spi};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard as SdmmcSdCard;

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

    Timer::after_millis(250).await;

    let display_fut = {
        let mut config = spi::Config::default();
        config.frequency = 16_000_000;
        let spi = Spi::new(
            p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, config,
        );
        let cs = p.PIN_13;
        let data = p.PIN_14;
        let reset = p.PIN_15;

        let display = init_display(spi, cs, data, reset).await;
        display_handler(display)
    };

    let sdcard = {
        let mut config = spi::Config::default();
        config.frequency = 400_000;
        let clk = p.PIN_18;
        let mosi = p.PIN_19;
        let miso = p.PIN_16;
        let spi = Spi::new_blocking(p.SPI0, clk, mosi, miso, config.clone());
        let cs = Output::new(p.PIN_17, Level::High);
        let det = Input::new(p.PIN_22, Pull::None);

        let device = ExclusiveDevice::new(spi, cs, Delay).unwrap();
        let sdcard = SdmmcSdCard::new(device, Delay);

        config.frequency = 32_000_000;
        sdcard.spi(|dev| dev.bus_mut().set_config(&config));
        SdCard::new(sdcard, det)
    };

    let usb = embassy_rp_usb::Driver::new(p.USB, Irqs);
    let usb_fut = usb_handler(usb, sdcard);

    join(usb_fut, display_fut).await;
}
