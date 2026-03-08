//! Binary-Buddy Allocator Implementation
//!
//! A buddy memory allocator that manages memory in power-of-two blocks.
//! Provides O(log n) allocation and deallocation with fast coalescing.
//! Optimized for no_std bare-metal environments.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

pub struct BuddyAllocator {
    base: usize,
    total_size: usize,
    max_order: usize,
    free_lists: [UnsafeCell<FreeList>; MAX_ORDERS],
}

const MAX_ORDERS: usize = 16;

struct FreeList {
    head: usize,
}

impl FreeList {
    const fn new() -> Self {
        FreeList { head: 0 }
    }

    fn is_empty(&self) -> bool {
        self.head == 0
    }

    fn push(&mut self, addr: usize) {
        let next_ptr = addr as *mut usize;
        unsafe { next_ptr.write(self.head) };
        self.head = addr;
    }

    fn pop(&mut self) -> usize {
        if self.head == 0 {
            return 0;
        }
        let addr = self.head;
        let next_ptr = addr as *mut usize;
        self.head = unsafe { next_ptr.read() };
        addr
    }

    fn remove(&mut self, addr: usize) -> bool {
        if self.head == 0 {
            return false;
        }

        if self.head == addr {
            self.pop();
            return true;
        }

        let mut current = self.head;
        while current != 0 {
            let next_ptr = current as *mut usize;
            let next = unsafe { next_ptr.read() };

            if next == addr {
                let addr_next_ptr = addr as *mut usize;
                let addr_next = unsafe { addr_next_ptr.read() };
                unsafe { next_ptr.write(addr_next) };
                return true;
            }

            current = next;
        }

        false
    }
}

impl BuddyAllocator {
    pub const HEAP_SIZE: usize = 64 * 1024;

    /// Creates a new BuddyAllocator with the given memory region.
    ///
    /// # Safety
    /// - `base_addr` must be valid and aligned
    /// - `size` must be a power of two
    /// - The memory region must not be used by anyone else
    pub unsafe fn new(base_addr: usize, size: usize) -> &'static mut BuddyAllocator {
        let max_order = (size.next_power_of_two().trailing_zeros() as usize).min(MAX_ORDERS - 1);

        static mut ALLOCATOR: BuddyAllocator = BuddyAllocator {
            base: 0,
            total_size: 0,
            max_order: 0,
            free_lists: [const { UnsafeCell::new(FreeList::new()) }; MAX_ORDERS],
        };

        ALLOCATOR.base = base_addr;
        ALLOCATOR.total_size = size;
        ALLOCATOR.max_order = max_order;
        ALLOCATOR.free_lists[max_order].get_mut().head = base_addr;

        #[allow(static_mut_refs)]
        &mut ALLOCATOR
    }

    fn order_to_size(order: usize) -> usize {
        1 << order
    }

    fn size_to_order(size: usize) -> usize {
        let order = size.saturating_sub(1).next_power_of_two().trailing_zeros() as usize;
        order.min(MAX_ORDERS - 1)
    }

    fn buddy_of(addr: usize, order: usize) -> usize {
        addr ^ (1 << order)
    }

    /// Allocates memory with the given layout.
    ///
    /// # Safety
    /// See GlobalAlloc::alloc
    pub unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::null_mut();
        }

        let required_order = Self::size_to_order(layout.size().max(layout.align()));

        let mut order = required_order;
        while order <= self.max_order {
            let free_list = self.free_lists[order].get();
            if !(*free_list).is_empty() {
                let block_addr = (*free_list).pop();

                while order > required_order {
                    order -= 1;
                    let buddy = block_addr + Self::order_to_size(order);
                    self.free_lists[order].get_mut().push(buddy);
                }

                return block_addr as *mut u8;
            }
            order += 1;
        }

        core::ptr::null_mut()
    }

    /// Deallocates memory with the given layout.
    ///
    /// # Safety
    /// See GlobalAlloc::dealloc
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let mut addr = ptr as usize;
        let mut order = Self::size_to_order(layout.size().max(layout.align()));

        if order > self.max_order {
            order = self.max_order;
        }

        self.free_lists[order].get_mut().push(addr);

        while order < self.max_order {
            let buddy = Self::buddy_of(addr, order);

            if self.free_lists[order].get_mut().remove(buddy) {
                addr = addr.min(buddy);
                order += 1;
                self.free_lists[order].get_mut().push(addr);
            } else {
                break;
            }
        }
    }

    pub fn is_null(&self) -> bool {
        self.base == 0
    }
}

unsafe impl GlobalAlloc for BuddyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = (self as *const BuddyAllocator as *mut BuddyAllocator)
            .as_mut()
            .unwrap();
        ptr.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ptr_mut = (self as *const BuddyAllocator as *mut BuddyAllocator)
            .as_mut()
            .unwrap();
        ptr_mut.dealloc(ptr, layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    #[test]
    fn test_new() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };
        assert!(!allocator.is_null());
    }

    #[test]
    fn test_alloc_single_block() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };

        let layout = Layout::from_size_align(4096, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };

        assert!(!ptr.is_null());
    }

    #[test]
    fn test_alloc_multiple_blocks() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };

        let layout = Layout::from_size_align(1024, 8).unwrap();

        let ptr1 = unsafe { allocator.alloc(layout) };
        let ptr2 = unsafe { allocator.alloc(layout) };

        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());
        assert_ne!(ptr1, ptr2);
    }

    #[test]
    fn test_alloc_dealloc_reuse() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };

        let layout = Layout::from_size_align(4096, 8).unwrap();

        let ptr1 = unsafe { allocator.alloc(layout) };
        assert!(!ptr1.is_null());

        unsafe { allocator.dealloc(ptr1, layout) };

        let ptr2 = unsafe { allocator.alloc(layout) };
        assert!(!ptr2.is_null());
    }

    #[test]
    #[allow(static_mut_refs)]
    fn test_alignment() {
        const ALIGNMENT: usize = 4096;
        static mut HEAP: [u8; BuddyAllocator::HEAP_SIZE + ALIGNMENT] =
            [0u8; BuddyAllocator::HEAP_SIZE + ALIGNMENT];
        let heap_addr = unsafe { HEAP.as_ptr() as usize };
        let aligned_addr = (heap_addr + ALIGNMENT - 1) & !(ALIGNMENT - 1);

        let allocator = unsafe { BuddyAllocator::new(aligned_addr, BuddyAllocator::HEAP_SIZE) };

        for align in &[1, 2, 4, 8, 16, 32, 64] {
            let layout = Layout::from_size_align(256, *align).unwrap();
            let ptr = unsafe { allocator.alloc(layout) };

            assert!(!ptr.is_null(), "Failed for align {}", align);
            assert_eq!(ptr as usize % *align, 0, "Misaligned for {}", align);
        }
    }

    #[test]
    fn test_oom() {
        let heap: MaybeUninit<[u8; 4096]> = MaybeUninit::uninit();
        let allocator = unsafe { BuddyAllocator::new(heap.as_ptr() as usize, 4096) };

        let layout = Layout::from_size_align(8192, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };

        assert!(ptr.is_null());
    }
}
