//! handles all the peripherals exposed by mcu through i2c (keyboard & battery registers)
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

use crate::peripherals::keyboard::{configure_keyboard, read_keyboard_fifo};

const MCU_ADDR: u8 = 0x1F;

const REG_ID_VER: u8 = 0x01;
const REG_ID_RST: u8 = 0x08;
const REG_ID_INT: u8 = 0x03;

#[embassy_executor::task]
pub async fn peripherals_task(
    mut i2c: I2c<'static, I2C1, Async>,
    mut keyboard_channel: Sender<'static, NoopRawMutex, KeyEvent, 10>,
) {
    Timer::after(embassy_time::Duration::from_millis(100)).await;

    #[cfg(feature = "defmt")]
    {
        let mut ver = [0_u8; 1];
        if let Ok(_) = i2c.write_read_async(MCU_ADDR, [REG_ID_VER], &mut ver).await {
            info!("stm32 firmware version: v{}", ver[0]);
        }
    }

    let i2c: Mutex<NoopRawMutex, I2c<'static, I2C1, Async>> = Mutex::new(i2c);
    let mut guard = i2c.lock().await;

    configure_keyboard(&mut guard, 200, 100).await;
    lcd_backlight(&mut guard, 255).await;
    key_backlight(&mut guard, 0).await;

    loop {
        Timer::after(Duration::from_millis(200)).await;
        read_keyboard_fifo(&mut guard, &mut keyboard_channel).await;
    }
}

const REG_ID_BKL: u8 = 0x05;

pub async fn lcd_backlight(i2c: &mut I2c<'static, I2C1, Async>, brightness: u8) {
    i2c.write_read_async(MCU_ADDR, [REG_ID_BKL], &mut [brightness])
        .await
        .unwrap();
}

const REG_ID_BK2: u8 = 0x0A;

pub async fn key_backlight(i2c: &mut I2c<'static, I2C1, Async>, brightness: u8) {
    i2c.write_read_async(MCU_ADDR, [REG_ID_BK2], &mut [brightness])
        .await
        .unwrap();
}
