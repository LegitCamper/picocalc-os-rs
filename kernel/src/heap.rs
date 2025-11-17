// This whole file was taken from:
// https://github.com/wezterm/picocalc-wezterm/blob/main/src/heap.rs

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use embedded_alloc::LlffHeap as Heap;

pub static mut HEAP: PsramHeap = PsramHeap::empty();

struct Region {
    start: AtomicUsize,
    size: AtomicUsize,
}

impl Region {
    const fn default() -> Self {
        Self {
            start: AtomicUsize::new(0),
            size: AtomicUsize::new(0),
        }
    }

    fn contains(&self, address: usize) -> bool {
        let start = self.start.load(Ordering::Relaxed);
        let end = self.start.load(Ordering::Relaxed);
        (start..start + end).contains(&address)
    }

    fn new(start: usize, size: usize) -> Self {
        Self {
            start: AtomicUsize::new(start),
            size: AtomicUsize::new(size),
        }
    }
}

/// FIXME: PSRAM-allocated memory isn't compatible with
/// CAS atomics, so we might need a bit of a think about this!
pub struct PsramHeap {
    heap: Heap,
    region: Region,
}

impl PsramHeap {
    pub const fn empty() -> Self {
        Self {
            heap: Heap::empty(),
            region: Region::default(),
        }
    }

    unsafe fn add_psram(&self, region: Region) {
        let start = region.start.load(Ordering::SeqCst);
        let size = region.size.load(Ordering::SeqCst);
        unsafe {
            self.heap.init(start, size);
        }
        self.region.start.store(start, Ordering::SeqCst);
        self.region.size.store(size, Ordering::SeqCst);
    }

    pub fn used(&self) -> usize {
        self.heap.used()
    }

    pub fn free(&self) -> usize {
        self.heap.free()
    }
}

unsafe impl GlobalAlloc for PsramHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            let ptr = self.heap.alloc(layout);
            if !ptr.is_null() {
                return ptr;
            } else {
                panic!("HEAP FULL");
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            let ptr_usize = ptr as usize;
            if self.region.contains(ptr_usize) {
                self.heap.dealloc(ptr, layout);
            }
        }
    }
}

pub fn init_qmi_psram_heap(size: u32) {
    unsafe { HEAP.add_psram(Region::new(0x11000000, size as usize)) }
}
