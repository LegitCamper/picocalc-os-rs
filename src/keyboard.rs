use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Sender};

const KEYBOARD_ADDR: u8 = 0x1F;

pub struct KeyEvent {
    key: KeyCode,
    state: KeyState,
}

#[embassy_executor::task]
pub async fn keyboard(
    mut i2c: I2c<'static, I2C1, Async>,
    channel: Sender<'static, NoopRawMutex, KeyEvent, 10>,
) {
    embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;

    let mut res = [0_u8; 2];
    if i2c.read_async(KEYBOARD_ADDR, &mut res).await.is_ok() {
        if let Ok(state) = KeyState::try_from(res[0]) {
            if let Ok(key) = KeyCode::try_from(res[1]) {
                let _ = channel.try_send(KeyEvent { key, state });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Idle = 0,
    Pressed,
    Hold,
    Released,
}

impl TryFrom<u8> for KeyState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(KeyState::Idle),
            1 => Ok(KeyState::Pressed),
            2 => Ok(KeyState::Hold),
            3 => Ok(KeyState::Released),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    // Joystick
    JoyUp = 0x01,
    JoyDown = 0x02,
    JoyLeft = 0x03,
    JoyRight = 0x04,
    JoyCenter = 0x05,

    // Buttons
    BtnLeft1 = 0x06,
    BtnRight1 = 0x07,
    BtnLeft2 = 0x11,
    BtnRight2 = 0x12,

    // Basic Keys
    Backspace = 0x08,
    Tab = 0x09,
    Enter = 0x0A,

    // Modifiers
    ModAlt = 0xA1,
    ModShiftLeft = 0xA2,
    ModShiftRight = 0xA3,
    ModSym = 0xA4,
    ModCtrl = 0xA5,

    // Navigation
    Esc = 0xB1,
    Left = 0xB4,
    Up = 0xB5,
    Down = 0xB6,
    Right = 0xB7,

    // Specials
    Break = 0xD0,
    Insert = 0xD1,
    Home = 0xD2,
    Del = 0xD4,
    End = 0xD5,
    PageUp = 0xD6,
    PageDown = 0xD7,

    // Locks
    CapsLock = 0xC1,

    // Function keys
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
}

impl TryFrom<u8> for KeyCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use KeyCode::*;
        match value {
            0x01 => Ok(JoyUp),
            0x02 => Ok(JoyDown),
            0x03 => Ok(JoyLeft),
            0x04 => Ok(JoyRight),
            0x05 => Ok(JoyCenter),
            0x06 => Ok(BtnLeft1),
            0x07 => Ok(BtnRight1),
            0x08 => Ok(Backspace),
            0x09 => Ok(Tab),
            0x0A => Ok(Enter),
            0x11 => Ok(BtnLeft2),
            0x12 => Ok(BtnRight2),
            0xA1 => Ok(ModAlt),
            0xA2 => Ok(ModShiftLeft),
            0xA3 => Ok(ModShiftRight),
            0xA4 => Ok(ModSym),
            0xA5 => Ok(ModCtrl),
            0xB1 => Ok(Esc),
            0xB4 => Ok(Left),
            0xB5 => Ok(Up),
            0xB6 => Ok(Down),
            0xB7 => Ok(Right),
            0xC1 => Ok(CapsLock),
            0xD0 => Ok(Break),
            0xD1 => Ok(Insert),
            0xD2 => Ok(Home),
            0xD4 => Ok(Del),
            0xD5 => Ok(End),
            0xD6 => Ok(PageUp),
            0xD7 => Ok(PageDown),
            0x81 => Ok(F1),
            0x82 => Ok(F2),
            0x83 => Ok(F3),
            0x84 => Ok(F4),
            0x85 => Ok(F5),
            0x86 => Ok(F6),
            0x87 => Ok(F7),
            0x88 => Ok(F8),
            0x89 => Ok(F9),
            0x90 => Ok(F10),
            _ => Err(()),
        }
    }
}
