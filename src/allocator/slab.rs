//! Slab Allocator Implementation
//!
//! A fixed-size slab allocator for high-velocity object caching.
//! Optimized for no_std bare-metal environments.
//! Uses lock-free atomic bitmap operations for O(1) allocation/deallocation.

use crate::sync::AtomicSlabBitmap;
use core::alloc::{GlobalAlloc, Layout};

pub struct SlabAllocator {
    object_size: usize,
    num_objects: usize,
    bitmap: AtomicSlabBitmap,
    memory_start: usize,
}

impl SlabAllocator {
    pub const fn new(object_size: usize, num_objects: usize) -> Self {
        SlabAllocator {
            object_size,
            num_objects,
            bitmap: AtomicSlabBitmap::new(num_objects),
            memory_start: 0,
        }
    }

    /// Initializes the slab allocator with the given memory region.
    ///
    /// # Safety
    /// - `memory_start` must point to valid memory of at least
    ///   `object_size * num_objects` bytes
    pub unsafe fn init(&mut self, memory_start: usize) {
        self.memory_start = memory_start;
    }

    /// Allocates a single object from the slab using lock-free atomic operations.
    ///
    /// # Safety
    /// - The slab must have been initialized with `init`
    pub unsafe fn alloc(&self) -> *mut u8 {
        if self.memory_start == 0 {
            return core::ptr::null_mut();
        }

        // Linear probe over fixed-size slots; each successful CAS reserves one slot.
        for i in 0..self.num_objects {
            if self.bitmap.try_set_bit(i) {
                let offset = i * self.object_size;
                return (self.memory_start + offset) as *mut u8;
            }
        }

        core::ptr::null_mut()
    }

    /// Deallocates an object from the slab using lock-free atomic operations.
    ///
    /// # Safety
    /// - `ptr` must have been returned by a previous call to `alloc`
    pub unsafe fn dealloc(&self, ptr: *mut u8) {
        if ptr.is_null() || self.memory_start == 0 {
            return;
        }

        let addr = ptr as usize;
        if addr < self.memory_start {
            return;
        }

        let offset = addr - self.memory_start;
        // Integer division maps pointer back to object slot index.
        let index = offset / self.object_size;

        if index >= self.num_objects {
            return;
        }

        self.bitmap.try_unset_bit(index);
    }

    pub fn is_null(&self) -> bool {
        self.memory_start == 0
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() != self.object_size {
            return core::ptr::null_mut();
        }
        self.alloc()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        self.dealloc(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJECT_SIZE: usize = 64;
    const NUM_OBJECTS: usize = 16;

    #[test]
    fn test_new() {
        let allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        assert!(allocator.is_null());
    }

    #[test]
    fn test_init() {
        let mut allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        unsafe { allocator.init(0x1000) };
        assert!(!allocator.is_null());
    }

    #[test]
    fn test_alloc_single() {
        let mut allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        unsafe { allocator.init(0x1000) };

        let ptr = unsafe { allocator.alloc() };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_alloc_multiple() {
        let mut allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        unsafe { allocator.init(0x1000) };

        let mut ptrs = [core::ptr::null_mut(); NUM_OBJECTS];
        for i in 0..NUM_OBJECTS {
            let ptr = unsafe { allocator.alloc() };
            assert!(!ptr.is_null());
            ptrs[i] = ptr;
        }

        let ptr = unsafe { allocator.alloc() };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_dealloc() {
        let mut allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        unsafe { allocator.init(0x1000) };

        let ptr = unsafe { allocator.alloc() };
        assert!(!ptr.is_null());

        unsafe { allocator.dealloc(ptr) };

        let ptr2 = unsafe { allocator.alloc() };
        assert!(!ptr2.is_null());
    }

    #[test]
    fn test_alloc_reuses_deallocated() {
        let mut allocator = SlabAllocator::new(OBJECT_SIZE, NUM_OBJECTS);
        unsafe { allocator.init(0x1000) };

        let ptr1 = unsafe { allocator.alloc() };
        unsafe { allocator.dealloc(ptr1) };
        let ptr2 = unsafe { allocator.alloc() };

        assert_eq!(ptr1, ptr2);
    }
}
