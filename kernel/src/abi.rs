use abi::Syscall;

#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch(call: *const Syscall) -> usize {
    let call = unsafe { &*call };
    match call {
        Syscall::DrawPixels { x, y, color } => {
            draw_pixel(*x, *y, *color);
            0
        }
    }
}
