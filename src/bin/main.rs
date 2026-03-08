#![no_std]
#![no_main]

use core::arch::asm;

#[cfg(not(test))]
use sos::BuddyAllocator;

const HEAP_START: usize = 0x0010_0000;
const HEAP_SIZE: usize = 64 * 1024;

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    asm!(
        "mov rsp, 0x00100000",
        "mov rbp, 0x00100000",
        "call kernel_main",
        options(noreturn)
    );
}

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn kernel_main() -> ! {
    let allocator = BuddyAllocator::new(HEAP_START, HEAP_SIZE);

    let layout = core::alloc::Layout::from_size_align(1024, 8).unwrap();
    let ptr = allocator.alloc(layout);

    if !ptr.is_null() {
        let slice = core::slice::from_raw_parts_mut(ptr, 1024);
        slice[0] = 0x42;
    }

    loop {
        asm!("hlt");
    }
}
