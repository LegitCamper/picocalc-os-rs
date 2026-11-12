use core::{alloc::Layout, cell::RefCell};
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use userlib_sys::{
    SysError,
    memory::{Allocation, AppMemory, Region},
};
#[cfg(feature = "psram")]
use {crate::heap::HEAP, core::alloc::GlobalAlloc};

// ponytail: 256 live app allocations; raise the bound if measured app demand needs it.
static MEMORY: Mutex<CriticalSectionRawMutex, RefCell<AppMemory<256>>> =
    Mutex::new(RefCell::new(AppMemory::new()));

pub fn begin(image: Region, stack: Region) {
    MEMORY.lock(|memory| {
        let mut memory = memory.borrow_mut();
        memory.image = image;
        memory.stack = stack;
    });
}

pub fn valid<T>(pointer: *const T, len: usize) -> bool {
    MEMORY.lock(|memory| {
        memory
            .borrow()
            .permits(pointer as usize, len, size_of::<T>(), align_of::<T>())
    })
}

pub fn allocate(layout: Layout) -> Result<*mut u8, SysError> {
    if layout.size() == 0 {
        return Err(SysError::InvalidArgument);
    }
    // SAFETY: layout is nonempty; both allocators are initialized before app entry.
    let pointer = unsafe {
        #[cfg(feature = "psram")]
        {
            HEAP.alloc(layout)
        }
        #[cfg(not(feature = "psram"))]
        {
            alloc::alloc::alloc(layout)
        }
    };
    if pointer.is_null() {
        return Err(SysError::NoMemory);
    }
    let allocation = Allocation {
        address: pointer as usize,
        layout,
    };
    if !MEMORY.lock(|memory| memory.borrow_mut().track(allocation)) {
        free(allocation);
        return Err(SysError::NoMemory);
    }
    Ok(pointer)
}

pub fn deallocate(pointer: *mut u8, layout: Layout) -> Result<(), SysError> {
    let allocation = Allocation {
        address: pointer as usize,
        layout,
    };
    if !MEMORY.lock(|memory| memory.borrow_mut().release(allocation)) {
        return Err(SysError::InvalidArgument);
    }
    free(allocation);
    Ok(())
}

fn free(allocation: Allocation) {
    // SAFETY: only exact live allocator records reach here, removed before freeing.
    unsafe {
        #[cfg(feature = "psram")]
        HEAP.dealloc(allocation.address as *mut u8, allocation.layout);
        #[cfg(not(feature = "psram"))]
        alloc::alloc::dealloc(allocation.address as *mut u8, allocation.layout);
    }
}

pub fn finish() {
    while let Some(allocation) = MEMORY.lock(|memory| memory.borrow_mut().pop()) {
        free(allocation);
    }
    begin(Region::default(), Region::default());
}
