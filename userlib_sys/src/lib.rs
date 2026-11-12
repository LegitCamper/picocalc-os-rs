#![cfg_attr(not(test), no_std)]
pub mod audio;
pub mod framebuffer;
pub mod memory;
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

pub const SYS_CALL_TABLE_COUNT: usize = 18;
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
    ReconfigureAudioSampleRate = 12,
    AudioBufferReady = 13,
    SendAudioBuffer = 14,
    FillRect = 15,
    Blit = 16,
    GetLastError = 17,
}

const _: () = {
    let slots = [
        SyscallTable::Alloc,
        SyscallTable::Dealloc,
        SyscallTable::PrintString,
        SyscallTable::SleepMs,
        SyscallTable::GetMs,
        SyscallTable::DrawIter,
        SyscallTable::GetKey,
        SyscallTable::GenRand,
        SyscallTable::ListDir,
        SyscallTable::ReadFile,
        SyscallTable::WriteFile,
        SyscallTable::FileLen,
        SyscallTable::ReconfigureAudioSampleRate,
        SyscallTable::AudioBufferReady,
        SyscallTable::SendAudioBuffer,
        SyscallTable::FillRect,
        SyscallTable::Blit,
        SyscallTable::GetLastError,
    ];
    let mut index = 0;
    while index < slots.len() {
        assert!(slots[index] as usize == index);
        index += 1;
    }
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SysError {
    Ok = 0,
    InvalidArgument = 1,
    NotFound = 2,
    Busy = 3,
    Io = 4,
    NoMemory = 5,
    Unsupported = 6,
    BufferTooSmall = 7,
}

impl SysError {
    pub fn from_code(code: i32) -> Result<(), Self> {
        match code {
            0 => Ok(()),
            1 => Err(Self::InvalidArgument),
            2 => Err(Self::NotFound),
            3 => Err(Self::Busy),
            5 => Err(Self::NoMemory),
            6 => Err(Self::Unsupported),
            7 => Err(Self::BufferTooSmall),
            _ => Err(Self::Io),
        }
    }
}

pub type GetLastError = extern "C" fn() -> i32;

/// Status of the last status-reporting syscall on the single app thread.
#[unsafe(no_mangle)]
pub extern "C" fn get_last_error() -> i32 {
    // SAFETY: the loader initializes every slot before calling the app entry.
    let function: GetLastError =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::GetLastError as usize]) };
    function()
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".syscall_table")]
pub static mut SYS_CALL_TABLE: [usize; SYS_CALL_TABLE_COUNT] = [0; SYS_CALL_TABLE_COUNT];

/// Maximum number of path components a syscall will walk.
pub const MAX_PATH_COMPONENTS: usize = 8;

/// Checks buffer arithmetic only, not memory ownership, mapping or access rights.
pub fn buffer_shape_valid(
    address: usize,
    len: usize,
    element_size: usize,
    alignment: usize,
) -> bool {
    if len == 0 {
        return true;
    }
    if address == 0 || !alignment.is_power_of_two() || !address.is_multiple_of(alignment) {
        return false;
    }
    len.checked_mul(element_size)
        .filter(|&bytes| bytes <= isize::MAX as usize)
        .and_then(|bytes| address.checked_add(bytes))
        .is_some()
}

/// Splits an absolute syscall path into its non-empty components.
///
/// Returns `None` for paths that are not absolute, are empty, contain an
/// empty component, contain a `.` or `..` component, or have more than
/// [`MAX_PATH_COMPONENTS`] components. Callers must treat `None` as
/// "reject this request".
pub fn split_path(path: &str) -> Option<([&str; MAX_PATH_COMPONENTS], usize)> {
    let rest = path.strip_prefix('/')?;
    if rest.is_empty() {
        return Some(([""; MAX_PATH_COMPONENTS], 0));
    }

    let mut out = [""; MAX_PATH_COMPONENTS];
    let mut count = 0;
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\0') {
            return None;
        }
        if count == MAX_PATH_COMPONENTS {
            return None;
        }
        out[count] = part;
        count += 1;
    }
    Some((out, count))
}

/// Bounds of a loaded image: the `[min_vaddr, min_vaddr + size)` window that
/// every virtual address in the ELF must fall inside.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ElfBounds {
    pub min_vaddr: u32,
    pub size: usize,
}

impl ElfBounds {
    /// Converts a virtual address to an offset into the loaded image,
    /// returning `None` if `vaddr` is below the image or if `[vaddr, vaddr+len)`
    /// does not fit entirely inside it.
    pub fn offset_of(&self, vaddr: u32, len: usize) -> Option<usize> {
        let offset = vaddr.checked_sub(self.min_vaddr)? as usize;
        let end = offset.checked_add(len)?;
        (end <= self.size).then_some(offset)
    }
}

/// Clips a rectangle given as `(x, y, w, h)` in signed screen coordinates to
/// a `width` × `height` screen, returning `(x, y, w, h)` with every component
/// in range, or `None` if the rectangle is empty or entirely off-screen.
pub fn clip_rect(
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    // i64 intermediates: `x + w` must not overflow for `x = i32::MAX`.
    let x1 = (x as i64) + (w as i64);
    let y1 = (y as i64) + (h as i64);
    let cx0 = x.max(0) as i64;
    let cy0 = y.max(0) as i64;
    let cx1 = x1.min(width as i64);
    let cy1 = y1.min(height as i64);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    Some((
        cx0 as u32,
        cy0 as u32,
        (cx1 - cx0) as u32,
        (cy1 - cy0) as u32,
    ))
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CLayout {
    size: usize,
    alignment: usize,
}

#[cfg(feature = "alloc")]
impl TryFrom<CLayout> for Layout {
    type Error = core::alloc::LayoutError;

    fn try_from(val: CLayout) -> Result<Self, Self::Error> {
        Layout::from_size_align(val.size, val.alignment)
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

pub type Alloc = unsafe extern "C" fn(layout: CLayout) -> *mut u8;

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alloc(layout: CLayout) -> *mut u8 {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: Alloc = unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Alloc as usize]) };
    // SAFETY: the caller supplies the allocator layout contract.
    unsafe { f(layout) }
}

pub type Dealloc = unsafe extern "C" fn(ptr: *mut u8, layout: CLayout);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
/// The pointer and layout must identify an allocation from `alloc`, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, layout: CLayout) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: Dealloc =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Dealloc as usize]) };
    // SAFETY: the caller supplies a live allocation with its original layout.
    unsafe { f(ptr, layout) }
}

pub type Print = unsafe extern "C" fn(ptr: *const u8, len: usize);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn print(ptr: *const u8, len: usize) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: Print =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::PrintString as usize]) };
    // SAFETY: the caller supplies the readable buffer contract.
    unsafe { f(ptr, len) };
}

pub type SleepMs = extern "C" fn(ms: u64);

#[unsafe(no_mangle)]
pub extern "C" fn sleep(ms: u64) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: SleepMs =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::SleepMs as usize]) };
    f(ms);
}

pub type GetMs = extern "C" fn() -> u64;

#[unsafe(no_mangle)]
pub extern "C" fn get_ms() -> u64 {
    // SAFETY: the loader initializes this slot with the declared function type.
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

pub type DrawIter = unsafe extern "C" fn(ptr: *const CPixel, len: usize);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn draw_iter(ptr: *const CPixel, len: usize) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: DrawIter =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::DrawIter as usize]) };
    // SAFETY: the caller supplies the readable pixel buffer contract.
    unsafe { f(ptr, len) };
}

/// Fills an axis-aligned rectangle with a single color in one call, instead
/// of marshaling it as a stream of individual `CPixel`s through `draw_iter`.
pub type FillRect = extern "C" fn(x: u16, y: u16, w: u16, h: u16, color: u16);

#[unsafe(no_mangle)]
pub extern "C" fn fill_rect(x: u16, y: u16, w: u16, h: u16, color: u16) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: FillRect =
        unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::FillRect as usize]) };
    f(x, y, w, h, color);
}

/// Writes a row-major block of raw RGB565 colors into a rectangle in one
/// call, instead of marshaling it pixel-by-pixel through `draw_iter`.
pub type Blit =
    unsafe extern "C" fn(x: u16, y: u16, w: u16, h: u16, colors: *const u16, len: usize);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blit(x: u16, y: u16, w: u16, h: u16, colors: *const u16, len: usize) {
    // SAFETY: the loader initializes this slot with the declared function type.
    let f: Blit = unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::Blit as usize]) };
    // SAFETY: the caller supplies the readable color buffer contract.
    unsafe { f(x, y, w, h, colors, len) };
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
        pub mods: u8,
    }

    impl From<KeyEventC> for KeyEvent {
        fn from(val: KeyEventC) -> Self {
            KeyEvent {
                key: val.key.into(),
                state: val.state,
                mods: Modifiers::from_bits_retain(val.mods),
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
                mods: val.mods.bits(),
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

    pub type GetKey = unsafe extern "C" fn(output: *mut KeyEventC);

    #[unsafe(no_mangle)]
    pub extern "C" fn get_key() -> KeyEventC {
        // SAFETY: the loader initializes this slot with the declared function type.
        let f: GetKey =
            unsafe { core::mem::transmute(SYS_CALL_TABLE[SyscallTable::GetKey as usize]) };
        let mut event = KeyEventC {
            key: 0,
            state: KeyState::Idle,
            mods: 0,
        };
        // SAFETY: event is live, aligned, exclusively borrowed storage until f returns.
        unsafe { f(&mut event) };
        event
    }
}

#[repr(C, u32)]
pub enum RngRequest {
    U32(u32) = 0,
    U64(u64) = 1,
    Bytes { ptr: *mut u8, len: usize } = 2,
}

pub type GenRand = unsafe extern "C" fn(req: *mut RngRequest);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gen_rand(req: *mut RngRequest) {
    // SAFETY: the loader initializes this slot; caller owns request and output storage.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::GenRand as usize];
        let f: GenRand = core::mem::transmute(ptr);
        f(req)
    }
}

pub type ListDir = unsafe extern "C" fn(
    str: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    file_len: usize,
    max_entry_str_len: usize,
) -> usize;

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn list_dir(
    str: *const u8,
    len: usize,
    entries: *mut *mut c_char,
    entry_count: usize,
    max_entry_str_len: usize,
) -> usize {
    // SAFETY: the loader initializes this slot; caller supplies disjoint writable entries.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::ListDir as usize];
        let f: ListDir = core::mem::transmute(ptr);
        f(str, len, entries, entry_count, max_entry_str_len)
    }
}

pub type ReadFile = unsafe extern "C" fn(
    str: *const u8,
    len: usize,
    read_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize;

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn read_file(
    str: *const u8,
    len: usize,
    read_from: usize,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    // SAFETY: the loader initializes this slot; caller supplies disjoint path and output.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::ReadFile as usize];
        let f: ReadFile = core::mem::transmute(ptr);
        f(str, len, read_from, buf, buf_len)
    }
}

pub type WriteFile = unsafe extern "C" fn(
    str: *const u8,
    len: usize,
    write_from: usize,
    buf: *const u8,
    buf_len: usize,
);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn write_file(
    str: *const u8,
    len: usize,
    write_from: usize,
    buf: *const u8,
    buf_len: usize,
) {
    // SAFETY: the loader initializes this slot; caller supplies readable path and payload.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::WriteFile as usize];
        let f: WriteFile = core::mem::transmute(ptr);
        f(str, len, write_from, buf, buf_len)
    }
}

pub type FileLen = unsafe extern "C" fn(str: *const u8, len: usize) -> usize;

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn file_len(str: *const u8, len: usize) -> usize {
    // SAFETY: the loader initializes this slot; caller supplies a readable path.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::FileLen as usize];
        let f: FileLen = core::mem::transmute(ptr);
        f(str, len)
    }
}

pub type ReconfigureAudioSampleRate = extern "C" fn(sample_rate: u32);

#[unsafe(no_mangle)]
pub extern "C" fn reconfigure_audio_sample_rate(sample_rate: u32) {
    // SAFETY: the loader initializes this slot with the declared function type.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::ReconfigureAudioSampleRate as usize];
        let f: ReconfigureAudioSampleRate = core::mem::transmute(ptr);
        f(sample_rate)
    }
}

pub type AudioBufferReady = extern "C" fn() -> bool;

#[unsafe(no_mangle)]
pub extern "C" fn audio_buffer_ready() -> bool {
    // SAFETY: the loader initializes this slot with the declared function type.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::AudioBufferReady as usize];
        let f: AudioBufferReady = core::mem::transmute(ptr);
        f()
    }
}

pub const AUDIO_BUFFER_SAMPLES: usize = 1024;
pub const AUDIO_BUFFER_LEN: usize = AUDIO_BUFFER_SAMPLES * 2;

pub type SendAudioBuffer = unsafe extern "C" fn(ptr: *const u8, len: usize);

/// # Safety
/// The loader must initialize the syscall table. Pointer arguments must reference
/// live, correctly aligned storage of the stated length for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn send_audio_buffer(buf: *const u8, len: usize) {
    // SAFETY: the loader initializes this slot; caller supplies a readable sample buffer.
    unsafe {
        let ptr = SYS_CALL_TABLE[SyscallTable::SendAudioBuffer as usize];
        let f: SendAudioBuffer = core::mem::transmute(ptr);
        f(buf, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_stub_supplies_explicit_initialized_output() {
        unsafe extern "C" fn write_key(output: *mut keyboard::KeyEventC) {
            // SAFETY: get_key passes its initialized local event for this synchronous call.
            let output = unsafe { &mut *output };
            assert_eq!(output.state, keyboard::KeyState::Idle);
            output.key = 0x0a;
            output.state = keyboard::KeyState::Pressed;
            output.mods = 0xff;
        }
        // SAFETY: only this test uses the keyboard slot in this host process.
        unsafe { SYS_CALL_TABLE[SyscallTable::GetKey as usize] = write_key as *const () as usize };
        let event = keyboard::get_key();
        assert_eq!(event.key, 0x0a);
        assert_eq!(event.state, keyboard::KeyState::Pressed);
        assert_eq!(event.mods, 0xff);
    }
    use strum::EnumCount;

    #[test]
    fn launcher_delete_key_is_distinct_from_backspace() {
        use keyboard::KeyCode;

        assert_eq!(KeyCode::from(0xD4), KeyCode::Del);
        assert_eq!(u8::from(KeyCode::Del), 0xD4);
        assert_eq!(KeyCode::from(0x08), KeyCode::Backspace);
        assert_ne!(KeyCode::Del, KeyCode::Backspace);
    }

    #[test]
    fn syscall_table_count_matches_enum() {
        assert_eq!(SYS_CALL_TABLE_COUNT, SyscallTable::COUNT);
    }

    #[test]
    fn buffer_shape_rejects_null_alignment_and_overflow() {
        assert!(buffer_shape_valid(0, 0, 1, 1));
        assert!(!buffer_shape_valid(0, 1, 1, 1));
        assert!(!buffer_shape_valid(3, 1, 4, 4));
        assert!(!buffer_shape_valid(4, 1, 1, 0));
        assert!(!buffer_shape_valid(4, 1, 1, 3));
        assert!(!buffer_shape_valid(4, usize::MAX, 2, 4));
        assert!(!buffer_shape_valid(4, isize::MAX as usize + 1, 1, 4));
        assert!(!buffer_shape_valid(usize::MAX, 1, 1, 1));
        assert!(buffer_shape_valid(4, 2, 4, 4));
    }

    #[test]
    fn status_codes_preserve_errors() {
        assert_eq!(SysError::from_code(0), Ok(()));
        assert_eq!(SysError::from_code(7), Err(SysError::BufferTooSmall));
        assert_eq!(SysError::from_code(-1), Err(SysError::Io));
    }

    #[test]
    fn keyboard_abi_preserves_modifier_bits() {
        let event = keyboard::KeyEventC {
            key: 0,
            state: keyboard::KeyState::Idle,
            mods: u8::MAX,
        };
        assert_eq!(
            keyboard::KeyEventC::from(keyboard::KeyEvent::from(event)).mods,
            u8::MAX
        );
    }

    #[test]
    fn split_path_accepts_root() {
        let (_, count) = split_path("/").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn split_path_splits_components() {
        let (parts, count) = split_path("/gifs/a.gif").unwrap();
        assert_eq!(count, 2);
        assert_eq!(&parts[..count], &["gifs", "a.gif"]);
    }

    #[test]
    fn split_path_rejects_relative_path() {
        assert!(split_path("gifs/a.gif").is_none());
    }

    #[test]
    fn split_path_rejects_empty_input() {
        assert!(split_path("").is_none());
    }

    #[test]
    fn split_path_rejects_double_slash() {
        assert!(split_path("/gifs//a.gif").is_none());
    }

    #[test]
    fn split_path_rejects_parent_component() {
        assert!(split_path("/gifs/../secret").is_none());
    }

    #[test]
    fn split_path_rejects_too_many_components() {
        assert!(split_path("/a/b/c/d/e/f/g/h/i").is_none());
    }

    #[test]
    fn offset_of_accepts_address_at_image_start() {
        let b = ElfBounds {
            min_vaddr: 0x1000,
            size: 64,
        };
        assert_eq!(b.offset_of(0x1000, 4), Some(0));
    }

    #[test]
    fn offset_of_accepts_last_fitting_window() {
        let b = ElfBounds {
            min_vaddr: 0x1000,
            size: 64,
        };
        assert_eq!(b.offset_of(0x103c, 4), Some(60));
    }

    #[test]
    fn offset_of_rejects_address_below_image() {
        let b = ElfBounds {
            min_vaddr: 0x1000,
            size: 64,
        };
        assert_eq!(b.offset_of(0x0fff, 4), None);
    }

    #[test]
    fn offset_of_rejects_window_past_image_end() {
        let b = ElfBounds {
            min_vaddr: 0x1000,
            size: 64,
        };
        assert_eq!(b.offset_of(0x103d, 4), None);
    }

    #[test]
    fn offset_of_rejects_length_overflow() {
        let b = ElfBounds {
            min_vaddr: 0,
            size: 64,
        };
        assert_eq!(b.offset_of(0, usize::MAX), None);
    }

    #[test]
    fn clip_rect_passes_through_fully_visible_rect() {
        assert_eq!(clip_rect(10, 10, 20, 20, 320, 320), Some((10, 10, 20, 20)));
    }

    #[test]
    fn clip_rect_clips_negative_origin() {
        assert_eq!(clip_rect(-5, -5, 20, 20, 320, 320), Some((0, 0, 15, 15)));
    }

    #[test]
    fn clip_rect_clips_past_right_edge() {
        assert_eq!(clip_rect(310, 0, 20, 10, 320, 320), Some((310, 0, 10, 10)));
    }

    #[test]
    fn clip_rect_rejects_offscreen_rect() {
        assert_eq!(clip_rect(400, 400, 10, 10, 320, 320), None);
    }

    #[test]
    fn clip_rect_rejects_zero_size() {
        assert_eq!(clip_rect(0, 0, 0, 10, 320, 320), None);
    }

    #[test]
    fn clip_rect_does_not_overflow_on_extreme_origin() {
        assert_eq!(clip_rect(i32::MAX, 0, 16, 16, 320, 320), None);
        assert_eq!(clip_rect(i32::MIN, 0, 16, 16, 320, 320), None);
    }

    #[test]
    fn split_path_rejects_embedded_nul_and_trailing_slash() {
        assert!(split_path("/gifs/a\0.gif").is_none());
        assert!(split_path("/gifs/").is_none());
        assert!(split_path("/./file").is_none());
    }
}
