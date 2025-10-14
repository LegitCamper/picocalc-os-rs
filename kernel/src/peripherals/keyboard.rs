use crate::peripherals::PERIPHERAL_BUS;
pub use abi_sys::keyboard::{KeyCode, KeyEvent, KeyState, Modifiers};

const REG_ID_KEY: u8 = 0x04;
const REG_ID_FIF: u8 = 0x09;

const KEY_CAPSLOCK: u8 = 1 << 5;
const KEY_NUMLOCK: u8 = 1 << 6;
const KEY_COUNT_MASK: u8 = 0x1F; // 0x1F == 31

pub async fn read_keyboard_fifo() -> Option<KeyEvent> {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let mut key_status = [0_u8; 1];

    if i2c
        .write_read_async(super::MCU_ADDR, [REG_ID_KEY], &mut key_status)
        .await
        .is_ok()
    {
        let _caps = key_status[0] & KEY_CAPSLOCK == KEY_CAPSLOCK;
        let _num = key_status[0] & KEY_NUMLOCK == KEY_NUMLOCK;
        let fifo_count = key_status[0] & KEY_COUNT_MASK;

        if fifo_count >= 1 {
            let mut event = [0_u8; 2];

            if i2c
                .write_read_async(super::MCU_ADDR, [REG_ID_FIF], &mut event)
                .await
                .is_ok()
            {
                return Some(KeyEvent {
                    state: KeyState::from(event[0]),
                    key: KeyCode::from(event[1]),
                    mods: Modifiers::NONE,
                });
            }
        }
    }
    None
}

const REG_ID_DEB: u8 = 0x06;
const REG_ID_FRQ: u8 = 0x07;

pub async fn configure_keyboard(debounce: u8, poll_freq: u8) {
    let mut i2c = PERIPHERAL_BUS.get().lock().await;
    let i2c = i2c.as_mut().unwrap();

    let _ = i2c
        .write_read_async(super::MCU_ADDR, [REG_ID_DEB], &mut [debounce])
        .await;

    let _ = i2c
        .write_read_async(super::MCU_ADDR, [REG_ID_FRQ], &mut [poll_freq])
        .await;
}
