const X: usize = 320;
const Y: usize = 200;
const SIZE: usize = X * Y;

pub static mut DISPLAY: Option<SimulatorDisplay<Rgb565>> = None;
pub static mut SCREEN_BUFFER: ScreenBuffer<X, Y, SIZE> = ScreenBuffer::new();
pub static mut START: Option<Instant> = None;

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
    let start = unsafe { START.unwrap() };
    Instant::now()
        .duration_since(start)
        .as_millis()
        .try_into()
        .expect("Cannot Fit Start time into u32")
}

#[unsafe(no_mangle)]
extern "C" fn DG_GetKey(pressed: *mut raw::c_int, key: *mut raw::c_uchar) -> raw::c_int {
    0
}

#[unsafe(no_mangle)]
extern "C" fn DG_SleepMs(ms: u32) {
    sleep(Duration::from_millis(ms as u64));
}
