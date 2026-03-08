#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate std;

pub mod allocator;
pub mod framekernel;
pub mod sync;

pub use allocator::{BuddyAllocator, SlabAllocator};
pub use framekernel::OSTD;
pub use sync::{AtomicSlabBitmap, Mutex, Spinlock};

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;
    use core::mem::MaybeUninit;

    #[test]
    fn test_buddy_allocator() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };
        let layout = Layout::from_size_align(1024, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null());
        unsafe { allocator.dealloc(ptr, layout) };
    }

    #[test]
    fn test_slab_allocator() {
        let mut allocator = SlabAllocator::new(64, 16);
        unsafe { allocator.init(0x1000) };
        let ptr = unsafe { allocator.alloc() };
        assert!(!ptr.is_null());
        unsafe { allocator.dealloc(ptr) };
    }

    #[test]
    fn test_spinlock() {
        let lock = Spinlock::new();
        assert!(!lock.is_locked());
        lock.lock();
        assert!(lock.is_locked());
        lock.unlock();
        assert!(!lock.is_locked());
    }

    #[test]
    fn test_mutex() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock();
        assert_eq!(*guard, 42);
    }
}
