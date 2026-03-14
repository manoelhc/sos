//! Framekernel Architecture - OSTD Framework
//!
//! The OS Framework (OSTD) is the minimal Trusted Computing Base (TCB).
//! This module contains the unsafe abstractions for hardware interaction.

pub mod ostd {
    use crate::allocator::BuddyAllocator;
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicPtr, Ordering};

    static mut HEAP: [u8; BuddyAllocator::HEAP_SIZE] = [0u8; BuddyAllocator::HEAP_SIZE];
    static ALLOCATOR: AtomicPtr<BuddyAllocator> = AtomicPtr::new(core::ptr::null_mut());

    fn get_allocator() -> &'static mut BuddyAllocator {
        // Fast path: allocator already initialized.
        let ptr = ALLOCATOR.load(Ordering::Acquire);
        if ptr.is_null() {
            // Slow path: initialize once with CAS so concurrent callers race safely.
            let new_alloc = unsafe {
                BuddyAllocator::new(
                    core::ptr::addr_of_mut!(HEAP) as *mut u8 as usize,
                    BuddyAllocator::HEAP_SIZE,
                )
            };
            let new_ptr = new_alloc;
            match ALLOCATOR.compare_exchange(
                core::ptr::null_mut(),
                new_ptr,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => &mut *new_ptr,
                Err(existing) => unsafe { &mut *existing },
            }
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

        /// # Safety
        /// The caller must ensure `heap_start` and `heap_size` are valid and
        /// the heap region does not overlap with other allocated memory.
        pub unsafe fn new(heap_start: usize, heap_size: usize) -> OSTD {
            // This stores metadata only; allocator backing memory is managed by `get_allocator`.
            OSTD {
                _heap_start: heap_start,
                _heap_size: heap_size,
            }
        }

        /// # Safety
        /// The `layout` must be valid and properly aligned.
        pub unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            get_allocator().alloc(layout)
        }

        /// # Safety
        /// The `ptr` must have been allocated by this allocator with the same `layout`.
        pub unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            get_allocator().dealloc(ptr, layout);
        }
    }

    #[cfg(all(not(test), target_os = "none"))]
    #[global_allocator]
    static GLOBAL_ALLOCATOR: OSTD = OSTD {
        _heap_start: 0,
        _heap_size: 0,
    };

    unsafe impl GlobalAlloc for OSTD {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            get_allocator().alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            get_allocator().dealloc(ptr, layout);
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
