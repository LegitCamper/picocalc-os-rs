//! handles all the peripherals exposed by mcu through i2c (keyboard & battery registers)
//!

use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, lazy_lock::LazyLock, mutex::Mutex,
};
use embassy_time::Timer;

pub mod keyboard;

use crate::peripherals::keyboard::{configure_keyboard, read_keyboard_fifo};

const MCU_ADDR: u8 = 0x1F;

type I2CBUS = I2c<'static, I2C1, Async>;
pub static PERIPHERAL_BUS: LazyLock<Mutex<CriticalSectionRawMutex, Option<I2CBUS>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Debug)]
pub enum PeripheralError {
    Unavailable,
    Bus(embassy_rp::i2c::Error),
}

async fn write_register(register: u8, value: u8) -> Result<(), PeripheralError> {
    let mut bus = PERIPHERAL_BUS.get().lock().await;
    let bus = bus.as_mut().ok_or(PeripheralError::Unavailable)?;
    bus.write_async(MCU_ADDR, [register, value])
        .await
        .map_err(PeripheralError::Bus)
}

const REG_ID_VER: u8 = 0x01;
const REG_ID_RST: u8 = 0x08;
const REG_ID_INT: u8 = 0x03;

pub async fn conf_peripherals(i2c: I2CBUS) {
    Timer::after(embassy_time::Duration::from_millis(100)).await;

    PERIPHERAL_BUS.get().lock().await.replace(i2c);

    if configure_keyboard(200, 100).await.is_err() {
        #[cfg(feature = "defmt")]
        defmt::warn!("Keyboard configuration failed");
    }

    // empty keys
    while read_keyboard_fifo().await.is_some() {}

    // set_lcd_backlight(255).await;
    if set_key_backlight(0).await.is_err() {
        #[cfg(feature = "defmt")]
        defmt::warn!("Keyboard backlight configuration failed");
    }
}

/// return major & minor mcu version
async fn get_version() -> (u8, u8) {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let mut ver = [0_u8; 1];
    let _ = i2c.write_read_async(MCU_ADDR, [REG_ID_VER], &mut ver).await;

    (ver[0] >> 4, ver[0] & 0x0F)
}

const REG_ID_BKL: u8 = 0x05;
pub async fn set_lcd_backlight(brightness: u8) -> Result<(), PeripheralError> {
    write_register(REG_ID_BKL, brightness).await
}
pub async fn get_lcd_backlight() -> u8 {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let mut buf = [0_u8; 2];

    let _ = i2c.write_read_async(MCU_ADDR, [REG_ID_BKL], &mut buf).await;
    buf[1]
}

const REG_ID_BK2: u8 = 0x0A;
pub async fn set_key_backlight(brightness: u8) -> Result<(), PeripheralError> {
    write_register(REG_ID_BK2, brightness).await
}
pub async fn get_key_backlight() -> u8 {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let mut buf = [0_u8; 2];

    let _ = i2c.write_read_async(MCU_ADDR, [REG_ID_BK2], &mut buf).await;
    buf[1]
}

const REG_ID_BAT: u8 = 0x0b;
pub async fn get_battery() -> u8 {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let mut buf = [0_u8; 2];

    let _ = i2c.write_read_async(MCU_ADDR, [REG_ID_BAT], &mut buf).await;

    buf[1]
}
