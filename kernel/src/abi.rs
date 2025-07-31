use abi::Syscall;
use embassy_futures::block_on;
use embedded_graphics::{
    Drawable,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};

use crate::display::FRAMEBUFFER;

#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch(call: *const Syscall) -> usize {
    let call = unsafe { &*call };
    match call {
        Syscall::DrawPixel { x, y, color } => {
            draw_pixel(*x, *y, *color);
            0
        }
    }
}

fn draw_pixel(x: u32, y: u32, color: u16) {
    let framebuffer = block_on(FRAMEBUFFER.lock());
    Rectangle::new(Point::new(x as i32, y as i32), Size::new(1, 1))
        .draw_styled(
            &PrimitiveStyle::with_fill(Rgb565::RED),
            *framebuffer.borrow_mut().as_mut().unwrap(),
        )
        .unwrap();
}
