use defmt::{info, warn};
use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
    channel::Sender,
};

use crate::peripherals::PERIPHERAL_BUS;

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

bitflags::bitflags! {
    #[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct Modifiers: u8 {
        const NONE = 0;
        const CTRL = 1;
        const ALT = 2;
        const LSHIFT = 4;
        const RSHIFT = 8;
        const SYM = 16;
    }
}

#[derive(Debug)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub state: KeyState,
    pub mods: Modifiers,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Idle = 0,
    Pressed = 1,
    Hold = 2,
    Released = 3,
}

impl From<u8> for KeyState {
    fn from(value: u8) -> Self {
        match value {
            1 => KeyState::Pressed,
            2 => KeyState::Hold,
            3 => KeyState::Released,
            0 | _ => KeyState::Idle,
        }
    }
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    JoyUp = 0x01,
    JoyDown = 0x02,
    JoyLeft = 0x03,
    JoyRight = 0x04,
    JoyCenter = 0x05,
    BtnLeft1 = 0x06,
    BtnRight1 = 0x07,
    BtnLeft2 = 0x11,
    BtnRight2 = 0x12,
    Backspace = 0x08,
    Tab = 0x09,
    Enter = 0x0A,
    ModAlt = 0xA1,
    ModShiftLeft = 0xA2,
    ModShiftRight = 0xA3,
    ModSym = 0xA4,
    ModCtrl = 0xA5,
    Esc = 0xB1,
    Left = 0xB4,
    Up = 0xB5,
    Down = 0xB6,
    Right = 0xB7,
    Break = 0xD0,
    Insert = 0xD1,
    Home = 0xD2,
    Del = 0xD4,
    End = 0xD5,
    PageUp = 0xD6,
    PageDown = 0xD7,
    CapsLock = 0xC1,
    F1 = 0x81,
    F2 = 0x82,
    F3 = 0x83,
    F4 = 0x84,
    F5 = 0x85,
    F6 = 0x86,
    F7 = 0x87,
    F8 = 0x88,
    F9 = 0x89,
    F10 = 0x90,
    Char(char),
    Unknown(u8),
}

impl From<u8> for KeyCode {
    fn from(value: u8) -> Self {
        match value {
            0x01 => Self::JoyUp,
            0x02 => Self::JoyDown,
            0x03 => Self::JoyLeft,
            0x04 => Self::JoyRight,
            0x05 => Self::JoyCenter,
            0x06 => Self::BtnLeft1,
            0x07 => Self::BtnRight1,
            0x08 => Self::Backspace,
            0x09 => Self::Tab,
            0x0A => Self::Enter,
            0x11 => Self::BtnLeft2,
            0x12 => Self::BtnRight2,
            0xA1 => Self::ModAlt,
            0xA2 => Self::ModShiftLeft,
            0xA3 => Self::ModShiftRight,
            0xA4 => Self::ModSym,
            0xA5 => Self::ModCtrl,
            0xB1 => Self::Esc,
            0xB4 => Self::Left,
            0xB5 => Self::Up,
            0xB6 => Self::Down,
            0xB7 => Self::Right,
            0xC1 => Self::CapsLock,
            0xD0 => Self::Break,
            0xD1 => Self::Insert,
            0xD2 => Self::Home,
            0xD4 => Self::Del,
            0xD5 => Self::End,
            0xD6 => Self::PageUp,
            0xD7 => Self::PageDown,
            0x81 => Self::F1,
            0x82 => Self::F2,
            0x83 => Self::F3,
            0x84 => Self::F4,
            0x85 => Self::F5,
            0x86 => Self::F6,
            0x87 => Self::F7,
            0x88 => Self::F8,
            0x89 => Self::F9,
            0x90 => Self::F10,
            _ => match char::from_u32(value as u32) {
                Some(c) => Self::Char(c),
                None => Self::Unknown(value),
            },
        }
    }
}
