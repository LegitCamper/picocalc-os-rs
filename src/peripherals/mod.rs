//! handles polling keyboard events and battery levels from mcu over i2c1
//!

use embassy_futures::join::join;
use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Sender, mutex::Mutex};
use embassy_time::{Duration, Timer};

#[cfg(feature = "defmt")]
use defmt::info;

pub mod keyboard;
use keyboard::{KeyCode, KeyEvent, KeyState};
mod battery;
pub use battery::BATTERY_PCT;
use battery::read_battery;

use crate::peripherals::keyboard::read_keyboard_fifo;

const MCU_ADDR: u8 = 0x1F;

const REG_ID_VER: u8 = 0x01;
const REG_ID_CFG: u8 = 0x02;
const REG_ID_INT: u8 = 0x03;
const REG_ID_KEY: u8 = 0x04;
const REG_ID_BKL: u8 = 0x05;
const REG_ID_DEB: u8 = 0x06;
const REG_ID_FRQ: u8 = 0x07;
const REG_ID_RST: u8 = 0x08;
const REG_ID_FIF: u8 = 0x09;
const REG_ID_BK2: u8 = 0x0A;
const REG_ID_C64_MTX: u8 = 0x0c;
const REG_ID_C64_JS: u8 = 0x0d;

#[embassy_executor::task]
pub async fn peripherals_task(
    mut i2c: I2c<'static, I2C1, Async>,
    mut keyboard_channel: Sender<'static, NoopRawMutex, KeyEvent, 10>,
) {
    Timer::after(embassy_time::Duration::from_millis(100)).await;

    #[cfg(feature = "defmt")]
    {
        let mut ver = [0_u8; 1];
        if let Ok(firm_ver) = i2c.write_read_async(MCU_ADDR, [REG_ID_VER], &mut ver).await {
            info!("stm32 firmware version: v{}", ver[0]);
        }
    }

    let i2c: Mutex<NoopRawMutex, I2c<'static, I2C1, Async>> = Mutex::new(i2c);

    join(
        async {
            loop {
                Timer::after(Duration::from_secs(10)).await;
                let mut guard = i2c.lock().await;
                read_battery(&mut guard).await;
            }
        },
        async {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                let mut guard = i2c.lock().await;
                read_keyboard_fifo(&mut guard, &mut keyboard_channel).await;
            }
        },
    )
    .await;
}
