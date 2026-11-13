// This whole file was taken from:
// https://github.com/wezterm/picocalc-wezterm/blob/main/src/heap.rs

use core::alloc::{AllocError, Allocator};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
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
        let end = start + self.size.load(Ordering::Relaxed);
        (start..end).contains(&address)
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
        // SAFETY: the caller supplies a valid nonzero allocation layout.
        unsafe { self.heap.alloc(layout) }
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

unsafe impl Allocator for PsramHeap {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let pointer = if layout.size() == 0 {
            layout.align() as *mut u8
        } else {
            // SAFETY: layout is valid and nonzero; the heap synchronizes access.
            unsafe { self.heap.alloc(layout) }
        };
        let pointer = NonNull::new(pointer).ok_or(AllocError)?;
        Ok(NonNull::slice_from_raw_parts(pointer, layout.size()))
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            // SAFETY: the caller owns this allocation with the original layout.
            unsafe { GlobalAlloc::dealloc(self, pointer.as_ptr(), layout) };
        }
    }
}

pub fn init_qmi_psram_heap(size: u32) {
    unsafe { HEAP.add_psram(Region::new(0x11000000, size as usize)) }
}
