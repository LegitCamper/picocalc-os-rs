use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, watch::Watch};

const REG_ID_BAT: u8 = 0x0b;

pub static BATTERY_PCT: Watch<CriticalSectionRawMutex, u8, 1> = Watch::new();

pub async fn read_battery(i2c: &mut I2c<'static, I2C1, Async>) {
    let mut buf = [0_u8; 2];
    i2c.write_read_async(super::MCU_ADDR, [REG_ID_BAT], &mut buf)
        .await
        .unwrap();

    if buf[0] == REG_ID_BAT {
        BATTERY_PCT.sender().send(buf[0]);
    }
}
