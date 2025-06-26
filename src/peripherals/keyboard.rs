use defmt::{info, warn};
use embassy_rp::{
    i2c::{Async, I2c},
    peripherals::I2C1,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex},
    channel::Sender,
};

const REG_ID_CFG: u8 = 0x02;
const REG_ID_KEY: u8 = 0x04;
const REG_ID_FIF: u8 = 0x09;
// const REG_ID_C64_MTX: u8 = 0x0c;
// const REG_ID_C64_JS: u8 = 0x0d;

const KEY_CAPSLOCK: u8 = 1 << 5;
const KEY_NUMLOCK: u8 = 1 << 6;
const KEY_COUNT_MASK: u8 = 0x1F; // 0x1F == 31

pub async fn read_keyboard_fifo(
    i2c: &mut I2c<'static, I2C1, Async>,
    channel: &mut Sender<'static, NoopRawMutex, KeyEvent, 10>,
) {
    let mut key_status = [0_u8; 1];

    if i2c
        .write_read_async(super::MCU_ADDR, [REG_ID_KEY], &mut key_status)
        .await
        .is_ok()
    {
        // TODO: use caps & num lock
        let caps = key_status[0] & KEY_CAPSLOCK == KEY_CAPSLOCK;
        let num = key_status[0] & KEY_NUMLOCK == KEY_NUMLOCK;
        let fifo_count = key_status[0] & KEY_COUNT_MASK;

        if fifo_count >= 1 {
            let mut event = [0_u8; 2];

            if i2c
                .write_read_async(super::MCU_ADDR, [REG_ID_FIF], &mut event)
                .await
                .is_ok()
            {
                if let Ok(state) = KeyState::try_from(event[0]) {
                    if let Ok(key) = KeyCode::try_from(event[1]) {
                        channel
                            .try_send(KeyEvent { key, state })
                            .expect("Failed to push key");
                    }
                }
            }
        }
    }
}

const REG_ID_DEB: u8 = 0x06;
const REG_ID_FRQ: u8 = 0x07;

pub async fn configure_keyboard(i2c: &mut I2c<'static, I2C1, Async>, debounce: u8, poll_freq: u8) {
    i2c.write_read_async(super::MCU_ADDR, [REG_ID_DEB], &mut [debounce])
        .await
        .unwrap();

    i2c.write_read_async(super::MCU_ADDR, [REG_ID_FRQ], &mut [poll_freq])
        .await
        .unwrap();
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub state: KeyState,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

    // Printable ASCII (0x20 - 0x7F)
    Space = 0x20,
    Exclamation = 0x21, // !
    Quote = 0x22,       // "
    Hash = 0x23,        // #
    Dollar = 0x24,      // $
    Percent = 0x25,     // %
    Ampersand = 0x26,   // &
    Apostrophe = 0x27,  // '
    LeftParen = 0x28,   // (
    RightParen = 0x29,  // )
    Asterisk = 0x2A,    // *
    Plus = 0x2B,        // +
    Comma = 0x2C,       // ,
    Minus = 0x2D,       // -
    Period = 0x2E,      // .
    Slash = 0x2F,       // /
    Num0 = 0x30,
    Num1 = 0x31,
    Num2 = 0x32,
    Num3 = 0x33,
    Num4 = 0x34,
    Num5 = 0x35,
    Num6 = 0x36,
    Num7 = 0x37,
    Num8 = 0x38,
    Num9 = 0x39,
    Colon = 0x3A,
    Semicolon = 0x3B,
    LessThan = 0x3C,
    Equal = 0x3D,
    GreaterThan = 0x3E,
    Question = 0x3F,
    At = 0x40,
    A = 0x41,
    B = 0x42,
    C = 0x43,
    D = 0x44,
    E = 0x45,
    F = 0x46,
    G = 0x47,
    H = 0x48,
    I = 0x49,
    J = 0x4A,
    K = 0x4B,
    L = 0x4C,
    M = 0x4D,
    N = 0x4E,
    O = 0x4F,
    P = 0x50,
    Q = 0x51,
    R = 0x52,
    S = 0x53,
    T = 0x54,
    U = 0x55,
    V = 0x56,
    W = 0x57,
    X = 0x58,
    Y = 0x59,
    Z = 0x5A,
    LeftBracket = 0x5B,
    Backslash = 0x5C,
    RightBracket = 0x5D,
    Caret = 0x5E,
    Underscore = 0x5F,
    Backtick = 0x60,
    a = 0x61,
    b = 0x62,
    c = 0x63,
    d = 0x64,
    e = 0x65,
    f = 0x66,
    g = 0x67,
    h = 0x68,
    i = 0x69,
    j = 0x6A,
    k = 0x6B,
    l = 0x6C,
    m = 0x6D,
    n = 0x6E,
    o = 0x6F,
    p = 0x70,
    q = 0x71,
    r = 0x72,
    s = 0x73,
    t = 0x74,
    u = 0x75,
    v = 0x76,
    w = 0x77,
    x = 0x78,
    y = 0x79,
    z = 0x7A,
    LeftBrace = 0x7B,
    Pipe = 0x7C,
    RightBrace = 0x7D,
    Tilde = 0x7E,
    Delete = 0x7F,
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
            // ASCII 0x20 to 0x7F
            0x20..=0x7F => unsafe { Ok(core::mem::transmute(value)) },
            _ => Err(()),
        }
    }
}
