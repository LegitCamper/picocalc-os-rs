#![no_std]
#[cfg(feature = "alloc")]
use core::alloc::Layout;

use core::ffi::c_char;
use embedded_graphics::{
    Pixel,
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::{IntoStorage, Point},
};
use strum::{EnumCount, EnumIter};

pub type EntryFn = fn();

pub const SYS_CALL_TABLE_COUNT: usize = 14;
const _: () = assert!(SYS_CALL_TABLE_COUNT == SyscallTable::COUNT);

#[derive(Clone, Copy, EnumIter, EnumCount)]
#[repr(u8)]
pub enum SyscallTable {
    Alloc = 0,
    Dealloc = 1,
    PrintString = 2,
    SleepMs = 3,
    GetMs = 4,
    DrawIter = 5,
    GetKey = 6,
    GenRand = 7,
    ListDir = 8,
    ReadFile = 9,
    WriteFile = 10,
    FileLen = 11,
    AudioBufferReady = 12,
    SendAudioBuffer = 13,
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".syscall_table")]
pub static mut SYS_CALL_TABLE: [usize; SYS_CALL_TABLE_COUNT] = [0; SYS_CALL_TABLE_COUNT];

#[cfg(feature = "alloc")]
#[repr(C)]
pub struct CLayout {
    size: usize,
    alignment: usize,
}

#[cfg(feature = "alloc")]
impl From<CLayout> for Layout {
    fn from(val: CLayout) -> Self {
        unsafe { Layout::from_size_align_unchecked(val.size, val.alignment) }
    }
}

#[cfg(feature = "alloc")]
impl From<Layout> for CLayout {
    fn from(value: Layout) -> Self {
        Self {
            size: value.size(),
            alignment: value.align(),
        }
    }
}

pub type Alloc = extern "C" fn(layout: CLayout) -> *mut u8;

#[unsafe(no_mangle)]
pub extern "C" fn alloc(layout: CLayout) -> *mut u8 {
    let f: Alloc = unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Alloc as usize]) };
    f(layout)
}

pub type Dealloc = extern "C" fn(ptr: *mut u8, layout: CLayout);

#[unsafe(no_mangle)]
pub extern "C" fn dealloc(ptr: *mut u8, layout: CLayout) {
    let f: Dealloc =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Dealloc as usize]) };
    f(ptr, layout)
}

pub type Print = extern "C" fn(ptr: *const u8, len: usize);

#[unsafe(no_mangle)]
pub extern "C" fn print(ptr: *const u8, len: usize) {
    let f: Print =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::PrintString as usize]) };
    f(ptr, len);
}

pub type SleepMs = extern "C" fn(ms: u64);

#[unsafe(no_mangle)]
pub extern "C" fn sleep(ms: u64) {
    let f: SleepMs =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::SleepMs as usize]) };
    f(ms);
}

pub type GetMs = extern "C" fn() -> u64;

#[unsafe(no_mangle)]
pub extern "C" fn get_ms() -> u64 {
    let f: GetMs = unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::GetMs as usize]) };
    f()
}

#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct CPixel {
    pub x: i32,
    pub y: i32,
    pub color: u16,
}

impl CPixel {
    pub const fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            color: 0,
        }
    }
}

impl From<CPixel> for Pixel<Rgb565> {
    fn from(value: CPixel) -> Self {
        Pixel(
            Point::new(value.x, value.y),
            RawU16::new(value.color).into(),
        )
    }
}

impl From<Pixel<Rgb565>> for CPixel {
    fn from(value: Pixel<Rgb565>) -> Self {
        CPixel {
            x: value.0.x,
            y: value.0.y,
            color: value.1.into_storage(),
        }
    }
}

pub type DrawIter = extern "C" fn(ptr: *const CPixel, len: usize);

#[unsafe(no_mangle)]
pub extern "C" fn draw_iter(ptr: *const CPixel, len: usize) {
    let f: DrawIter =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::DrawIter as usize]) };
    f(ptr, len);
}

pub mod keyboard {
    use crate::{SYS_CALL_TABLE, SyscallTable};

    bitflags::bitflags! {
        #[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
        #[repr(C)]
        pub struct Modifiers: u8 {
            const NONE = 0;
            const CTRL = 1;
            const ALT = 2;
            const LSHIFT = 4;
            const RSHIFT = 8;
            const SYM = 16;
        }
    }

    #[repr(C)]
    pub struct KeyEventC {
        pub key: u8,
        pub state: KeyState,
        pub mods: Modifiers,
    }

    impl From<KeyEventC> for KeyEvent {
        fn from(val: KeyEventC) -> Self {
            KeyEvent {
                key: val.key.into(),
                state: val.state,
                mods: val.mods,
            }
        }
    }

    #[derive(Debug)]
    pub struct KeyEvent {
        pub key: KeyCode,
        pub state: KeyState,
        pub mods: Modifiers,
    }

    impl From<KeyEvent> for KeyEventC {
        fn from(val: KeyEvent) -> Self {
            KeyEventC {
                key: val.key.into(),
                state: val.state,
                mods: val.mods,
            }
        }
    }

    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
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
                _ => KeyState::Idle,
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

    impl From<KeyCode> for u8 {
        fn from(val: KeyCode) -> Self {
            match val {
                KeyCode::JoyUp => 0x01,
                KeyCode::JoyDown => 0x02,
                KeyCode::JoyLeft => 0x03,
                KeyCode::JoyRight => 0x04,
                KeyCode::JoyCenter => 0x05,
                KeyCode::BtnLeft1 => 0x06,
                KeyCode::BtnRight1 => 0x07,
                KeyCode::BtnLeft2 => 0x11,
                KeyCode::BtnRight2 => 0x12,
                KeyCode::Backspace => 0x08,
                KeyCode::Tab => 0x09,
                KeyCode::Enter => 0x0A,
                KeyCode::ModAlt => 0xA1,
                KeyCode::ModShiftLeft => 0xA2,
                KeyCode::ModShiftRight => 0xA3,
                KeyCode::ModSym => 0xA4,
                KeyCode::ModCtrl => 0xA5,
                KeyCode::Esc => 0xB1,
                KeyCode::Left => 0xB4,
                KeyCode::Up => 0xB5,
                KeyCode::Down => 0xB6,
                KeyCode::Right => 0xB7,
                KeyCode::Break => 0xD0,
                KeyCode::Insert => 0xD1,
                KeyCode::Home => 0xD2,
                KeyCode::Del => 0xD4,
                KeyCode::End => 0xD5,
                KeyCode::PageUp => 0xD6,
                KeyCode::PageDown => 0xD7,
                KeyCode::CapsLock => 0xC1,
                KeyCode::F1 => 0x81,
                KeyCode::F2 => 0x82,
                KeyCode::F3 => 0x83,
                KeyCode::F4 => 0x84,
                KeyCode::F5 => 0x85,
                KeyCode::F6 => 0x86,
                KeyCode::F7 => 0x87,
                KeyCode::F8 => 0x88,
                KeyCode::F9 => 0x89,
                KeyCode::F10 => 0x90,
                KeyCode::Char(char) => char as u8,
                KeyCode::Unknown(i) => i,
            }
        }
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

    pub type GetKey = extern "C" fn() -> KeyEventC;

    #[unsafe(no_mangle)]
    pub extern "C" fn get_key() -> KeyEventC {
        let f: GetKey =
            unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::GetKey as usize]) };
        f()
    }
}

#[repr(C)]
pub enum RngRequest {
    U32(u32),
    U64(u64),
    Bytes { ptr: *mut u8, len: usize },
}

pub type GenRand = extern "C" fn(req: &mut RngRequest);

#[unsafe(no_mangle)]
pub extern "C" fn gen_rand(req: &mut RngRequest) {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::GenRand as usize];
        let f: GenRand = core::mem::transmute(ptr);
        f(req)
    }
}

pub type ListDir = extern "C" fn(
    str: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    file_len: usize,
    max_entry_str_len: usize,
) -> usize;

#[unsafe(no_mangle)]
pub extern "C" fn list_dir(
    str: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    entry_count: usize,
    max_entry_str_len: usize,
) -> usize {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::ListDir as usize];
        let f: ListDir = core::mem::transmute(ptr);
        f(str, len, entries, entry_count, max_entry_str_len)
    }
}

pub type ReadFile = extern "C" fn(
    str: *const u8,
    len: usize,
    read_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize;

#[unsafe(no_mangle)]
pub extern "C" fn read_file(
    str: *const u8,
    len: usize,
    read_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::ReadFile as usize];
        let f: ReadFile = core::mem::transmute(ptr);
        f(str, len, read_from, buf, buf_len)
    }
}

pub type WriteFile =
    extern "C" fn(str: *const u8, len: usize, write_from: usize, buf: *const u8, buf_len: usize);

#[unsafe(no_mangle)]
pub extern "C" fn write_file(
    str: *const u8,
    len: usize,
    write_from: usize,
    buf: *const u8,
    buf_len: usize,
) {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::WriteFile as usize];
        let f: WriteFile = core::mem::transmute(ptr);
        f(str, len, write_from, buf, buf_len)
    }
}

pub type FileLen = extern "C" fn(str: *const u8, len: usize) -> usize;

#[unsafe(no_mangle)]
pub extern "C" fn file_len(str: *const u8, len: usize) -> usize {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::FileLen as usize];
        let f: FileLen = core::mem::transmute(ptr);
        f(str, len)
    }
}

pub type AudioBufferReady = extern "C" fn() -> bool;

#[allow(unused)]
pub fn audio_buffer_ready() -> bool {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::AudioBufferReady as usize];
        let f: AudioBufferReady = core::mem::transmute(ptr);
        f()
    }
}

pub const AUDIO_BUFFER_SAMPLES: usize = 1024;
pub const AUDIO_BUFFER_LEN: usize = AUDIO_BUFFER_SAMPLES * 2;

pub type SendAudioBuffer = extern "C" fn(ptr: *const u8, len: usize);

#[allow(unused)]
pub fn send_audio_buffer(buf: *const u8, len: usize) {
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::SendAudioBuffer as usize];
        let f: SendAudioBuffer = core::mem::transmute(ptr);
        f(buf, len)
    }
}
