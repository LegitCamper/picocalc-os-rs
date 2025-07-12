#![feature(impl_trait_in_assoc_type)]
#![feature(ascii_char)]
#![no_std]
#![no_main]

use crate::peripherals::keyboard::{KeyCode, read_keyboard_fifo};

use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::peripherals::I2C1;
use embassy_rp::{i2c, i2c::I2c, spi};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use heapless::String;

mod peripherals;
use peripherals::conf_peripherals;
mod display;
use display::display_handler;

embassy_rp::bind_interrupts!(struct Irqs {
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

static STRING: Mutex<ThreadModeRawMutex, String<25>> = Mutex::new(String::new());

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    STRING.lock().await.push_str("T: ").unwrap();

    // configure keyboard event handler
    let mut config = i2c::Config::default();
    config.frequency = 100_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, config);
    conf_peripherals(i2c1).await;

    let mut config = spi::Config::default();
    config.frequency = 16_000_000;
    let spi1 = spi::Spi::new(
        p.SPI1, p.PIN_10, p.PIN_11, p.PIN_12, p.DMA_CH0, p.DMA_CH1, config,
    );

    join(
        async {
            loop {
                Timer::after_millis(100).await;
                if let Some(key) = read_keyboard_fifo().await {
                    if let KeyCode::Char(c) = key.key {
                        STRING.lock().await.push(c).unwrap();
                    }
                }
            }
        },
        display_handler(spi1, p.PIN_13, p.PIN_14, p.PIN_15),
    )
    .await;
}
