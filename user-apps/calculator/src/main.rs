#![no_std]
#![no_main]

use abi::{Syscall, call_abi};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    for i in 0..300 {
        for o in 0..300 {
            let call = Syscall::DrawPixel {
                x: i,
                y: o,
                color: 0,
            };
            unsafe { call_abi(&call) };
        }
    }
}
