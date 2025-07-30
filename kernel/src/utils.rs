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
