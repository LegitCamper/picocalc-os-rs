use core::ffi::{c_char, c_int, c_uchar, c_void};
use core::ptr;

#[unsafe(no_mangle)]
pub static _ctype_: [c_uchar; 256] = [0u8; 256];

#[repr(C)]
pub struct Reent {
    pub errno: c_int,
    _reserved: [u8; 32],
}

#[unsafe(no_mangle)]
pub static mut _reent_data: Reent = Reent {
    errno: 0,
    _reserved: [0; 32],
};

#[unsafe(no_mangle)]
pub static mut _impure_ptr: *mut Reent = unsafe { &mut _reent_data as *mut Reent };

#[unsafe(no_mangle)]
pub extern "C" fn __errno() -> *mut c_int {
    unsafe { &mut (*_impure_ptr).errno as *mut c_int }
}

#[unsafe(no_mangle)]
pub extern "C" fn exit(_status: i32) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn close(_file: i32) -> i32 {
    -1
}

#[repr(C)]
pub struct Stat {
    st_mode: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn fstat(_file: i32, st: *mut Stat) -> i32 {
    unsafe {
        if !st.is_null() {
            (*st).st_mode = 0x2000; // S_IFCHR
        }
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn isatty(_file: i32) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn lseek(_file: i32, _ptr: i32, _dir: i32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn lseek64(_fd: c_int, _offset: i64, _whence: c_int) -> i64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn open(_name: *const u8, _flags: i32, _mode: i32) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn read(_file: i32, _ptr: *mut u8, len: usize) -> i32 {
    len as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn write(_file: i32, _ptr: *const u8, len: usize) -> i32 {
    len as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn rename(_old: *const c_char, _new: *const c_char) -> c_int {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn kill(_pid: i32, _sig: i32) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn getpid() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn link(_old: *const u8, _new: *const u8) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn unlink(_name: *const u8) -> i32 {
    -1
}

#[unsafe(no_mangle)]
pub extern "C" fn mkdir(_path: *const u8, _mode: u32) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn sbrk(incr: isize) -> *mut c_void {
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "C" fn fini() {}
