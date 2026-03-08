#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

use core::arch::asm;

#[cfg(not(test))]
use core::arch::global_asm;

#[cfg(not(test))]
global_asm!(
    ".section .note.Xen, \"a\"
    .align 4
    .long 4, 8, 18
    .asciz \"Xen\"
    .align 4
    .quad _start"
);

const COM1: u16 = 0x3F8;

#[inline]
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1 + 0, 1);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0x07);
        outb(COM1 + 4, 0x0B);
    }
}

pub fn serial_putc(c: u8) {
    unsafe {
        while (inb(COM1 + 5) & 0x20) == 0 {}
        outb(COM1, c);
    }
}

pub fn serial_puts(s: &str) {
    for c in s.bytes() {
        serial_putc(c);
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    serial_puts("[sos] panic\r\n");
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    asm!(
        "mov rsp, 0x00200000",
        "mov rbp, 0x00200000",
        "call kernel_main",
        options(noreturn)
    );
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    serial_init();
    serial_puts("[sos] boot OK\r\n");

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
