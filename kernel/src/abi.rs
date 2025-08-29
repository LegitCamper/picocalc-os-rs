use abi_sys::Syscall;
use defmt::info;
use embassy_futures::block_on;
use embedded_graphics::{
    Drawable,
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};

use crate::display::FRAMEBUFFER;

#[allow(unused)]
pub extern "C" fn call_abi(call: *const Syscall) {
    info!("called abi");
    let call = unsafe { &*call };
    match call {
        Syscall::DrawIter { pixels, len } => {
            // SAFETY: we're trusting the user program here
            let slice = unsafe { core::slice::from_raw_parts(*pixels, *len) };

            let framebuffer = block_on(FRAMEBUFFER.lock());
            framebuffer
                .borrow_mut()
                .as_mut()
                .unwrap()
                .draw_iter(slice.iter().copied())
                .unwrap();
        }
        Syscall::Print { msg, len } => {
            // SAFETY: we're trusting the user program here
            let slice = unsafe { core::slice::from_raw_parts(*msg, *len) };

            if let Ok(str) = str::from_utf8(slice) {
                defmt::info!("{:?}", str);
            } else {
                defmt::error!("Failed to parse user print str")
            }
        }
    }
}
