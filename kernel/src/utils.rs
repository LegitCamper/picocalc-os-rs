use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

/// Waits on core1 only; core0 keeps polling the async producers.
pub fn wait<F: Future>(future: F) -> F::Output {
    fn wake(_: *const ()) {
        cortex_m::asm::sev();
    }
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, |_| {});

    // SAFETY: the static vtable owns no data; SEV is safe on either core.
    let waker = unsafe { Waker::from_raw(clone(core::ptr::null())) };
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => cortex_m::asm::wfe(),
        }
    }
}

#[macro_export]
macro_rules! format {
    ($len:literal, $($arg:tt)*) => {{
        use heapless::String;
        use core::fmt::Write;

        let mut s: String<$len> = String::new();
        let _ = write!(&mut s, $($arg)*);
        s
    }}
}
