//! Framekernel Architecture - OSTD Framework
//!
//! The OS Framework (OSTD) is the minimal Trusted Computing Base (TCB).
//! This module contains the unsafe abstractions for hardware interaction.

pub mod ostd {
    use crate::allocator::BuddyAllocator;
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicPtr, Ordering};

    static HEAP: [u8; BuddyAllocator::HEAP_SIZE] = [0u8; BuddyAllocator::HEAP_SIZE];
    static ALLOCATOR: AtomicPtr<BuddyAllocator> = AtomicPtr::new(core::ptr::null_mut());

    fn get_allocator() -> &'static mut BuddyAllocator {
        let ptr = ALLOCATOR.load(Ordering::Acquire);
        if ptr.is_null() {
            let new_alloc =
                unsafe { BuddyAllocator::new(HEAP.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };
            let new_ptr = new_alloc;
            let exchange = ALLOCATOR
                .compare_exchange(
                    core::ptr::null_mut(),
                    new_ptr,
                    Ordering::Release,
                    Ordering::Acquire,
                )
                .unwrap_or(new_ptr);
            unsafe { &mut *exchange }
        } else {
            unsafe { &mut *ptr }
        }
    }

    pub struct OSTD {
        _heap_start: usize,
        _heap_size: usize,
    }

    impl OSTD {
        pub const HEAP_SIZE: usize = BuddyAllocator::HEAP_SIZE;

        pub unsafe fn new(heap_start: usize, heap_size: usize) -> OSTD {
            OSTD {
                _heap_start: heap_start,
                _heap_size: heap_size,
            }
        }

        pub unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            get_allocator().alloc(layout)
        }

        pub unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            get_allocator().dealloc(ptr, layout);
        }
    }

    unsafe impl GlobalAlloc for OSTD {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            self.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            self.dealloc(ptr, layout)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use core::alloc::Layout;
        use core::mem::MaybeUninit;

        #[test]
        fn test_ostd_new() {
            let ostd = unsafe { OSTD::new(0x10000, OSTD::HEAP_SIZE) };
            assert_eq!(ostd._heap_start, 0x10000);
        }

        #[test]
        fn test_ostd_alloc() {
            let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
            let allocator =
                unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };
            let layout = Layout::from_size_align(1024, 8).unwrap();
            let ptr = unsafe { allocator.alloc(layout) };
            assert!(!ptr.is_null());
        }
    }
}

pub use ostd::OSTD;
