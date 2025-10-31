use abi::{
    abi_sys::{get_ms, sleep},
    display::Display,
};
use core::ffi::{c_int, c_uchar};
use embedded_graphics::{
    Drawable, Pixel,
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
};

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
static mut DG_ScreenBuffer: *const u8 = core::ptr::null();

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RGBA {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

unsafe extern "C" {
    fn D_DoomMain();
    fn doomgeneric_Tick();
    fn M_FindResponseFile();

    pub static colors: [RGBA; 256];
}

pub fn tick() {
    unsafe { doomgeneric_Tick() };
}

pub struct ScreenBuffer<const RESX: usize, const RESY: usize, const SIZE: usize>(pub [u8; SIZE]);

impl<const RESX: usize, const RESY: usize, const SIZE: usize> ScreenBuffer<RESX, RESY, SIZE> {
    #[allow(dead_code)]
    const NA: () = assert!(SIZE == RESX * RESY);

    pub const fn new() -> Self {
        Self([0_u8; SIZE])
    }
}

#[unsafe(no_mangle)]
extern "C" fn DG_SetWindowTitle() {}

pub fn create<const RESX: usize, const RESY: usize, const SIZE: usize>(
    screenbuffer: &ScreenBuffer<RESX, RESY, SIZE>,
) {
    unsafe {
        M_FindResponseFile();

        DG_ScreenBuffer = screenbuffer.0.as_ptr();

        D_DoomMain();
    }
}

const X: usize = 320;
const Y: usize = 200;
const SIZE: usize = X * Y;

pub static mut DISPLAY: Option<Display> = None;
pub static mut SCREEN_BUFFER: ScreenBuffer<X, Y, SIZE> = ScreenBuffer::new();

#[unsafe(no_mangle)]
extern "C" fn DG_DrawFrame() {
    let palette565: [Rgb565; 256] = unsafe {
        colors.map(|c| {
            Rgb565::new(
                ((c.r as u16 * 31) / 255) as u8, // red 5 bits
                ((c.g as u16 * 63) / 255) as u8, // green 6 bits
                ((c.b as u16 * 31) / 255) as u8, // blue 5 bits
            )
        })
    };
    let buf = unsafe { &SCREEN_BUFFER.0 };

    let display = unsafe { DISPLAY.as_mut().unwrap() };
    display.clear(Rgb565::BLACK).unwrap();

    for y in 0..Y {
        for x in 0..X {
            let idx = y * X + x;
            let color = palette565[buf[idx] as usize];

            Pixel(Point::new(x as i32, y as i32), color)
                .draw(display)
                .unwrap();
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn DG_GetTicksMs() -> u32 {
    get_ms() as u32
}

#[unsafe(no_mangle)]
extern "C" fn DG_GetKey(pressed: *mut c_int, key: *mut c_uchar) -> c_int {
    0
}

#[unsafe(no_mangle)]
extern "C" fn DG_SleepMs(ms: u32) {
    sleep(ms.into());
}
