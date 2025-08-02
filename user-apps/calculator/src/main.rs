#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {} // or call your ABI trap, or `abort()`
}

#[unsafe(no_mangle)]
fn main() {
    loop {}
}
