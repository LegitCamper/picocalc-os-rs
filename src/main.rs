#![feature(impl_trait_in_assoc_type)]
#![feature(ascii_char)]
#![no_std]
#![no_main]

use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_rp::peripherals::I2C1;
use embassy_rp::{
    i2c,
    i2c::I2c,
    spi,
};

mod peripherals;
use peripherals::conf_peripherals;
mod display;
use display::display_task;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // configure keyboard event handler
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    conf_peripherals(i2c1).await;

    // configure display handler
    let mut config = spi::Config::default();
    config.frequency = 16_000_000;
    let spi1 = spi::Spi::new(
        p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, config,
    );
    spawner
        .spawn(display_task(spi1, p.PIN_13, p.PIN_14, p.PIN_15))
        .unwrap();
}
