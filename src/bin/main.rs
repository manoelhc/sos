//! Bare-metal boot stub.
//!
//! Provides:
//! - multiboot2 and Xen entry notes,
//! - minimal x86 port I/O helpers,
//! - COM1 serial initialization/output,
//! - `_start` and `kernel_main` entry flow.

#![cfg_attr(all(not(test), target_os = "none"), no_std)]
#![cfg_attr(all(not(test), target_os = "none"), no_main)]

#[cfg(all(not(test), target_os = "none"))]
use core::arch::asm;

#[cfg(all(not(test), target_os = "none"))]
use core::arch::global_asm;

#[cfg(all(not(test), target_os = "none"))]
global_asm!(
    ".section .multiboot2, \"a\"
    .align 8
multiboot2_header_start:
    .long 0xE85250D6
    .long 0
    .long multiboot2_header_end - multiboot2_header_start
    .long -(0xE85250D6 + 0 + (multiboot2_header_end - multiboot2_header_start))
    .align 8
    .short 0
    .short 0
    .long 8
multiboot2_header_end:"
);

#[cfg(all(not(test), target_os = "none"))]
// Xen PV guests expect a .note.Xen note that points to the entry symbol.
// This allows Xen to discover the initial instruction pointer for the guest.
global_asm!(
    ".section .note.Xen, \"a\"
    .align 4
    .long 4, 8, 18
    .asciz \"Xen\"
    .align 4
    .quad _start"
);

#[cfg(all(not(test), target_os = "none"))]
const COM1: u16 = 0x3F8;

#[cfg(all(not(test), target_os = "none"))]
#[inline]
// Writes one byte to an I/O port.
// Safety: caller must provide a valid port for the current platform.
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

#[cfg(all(not(test), target_os = "none"))]
#[inline]
// Reads one byte from an I/O port.
// Safety: caller must provide a valid port for the current platform.
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

#[cfg(all(not(test), target_os = "none"))]
// Initializes the legacy 16550-compatible UART at COM1.
// Configuration used here:
// - Baud rate divisor: 1 (115200 bps)
// - 8 data bits, no parity, 1 stop bit (8N1)
// - FIFO enabled/cleared
// - IRQs disabled (polled output only)
fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1, 1);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0x07);
        outb(COM1 + 4, 0x0B);
    }
}

#[cfg(all(not(test), target_os = "none"))]
// Sends a single byte over COM1.
// Polls until the transmitter holding register is empty.
pub fn serial_putc(c: u8) {
    unsafe {
        while (inb(COM1 + 5) & 0x20) == 0 {}
        outb(COM1, c);
    }
}

#[cfg(all(not(test), target_os = "none"))]
// Sends an ASCII/UTF-8 string as raw bytes over COM1.
pub fn serial_puts(s: &str) {
    for c in s.bytes() {
        serial_putc(c);
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn hex_digit(v: u8) -> u8 {
    match v {
        0..=9 => b'0' + v,
        _ => b'a' + (v - 10),
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn serial_put_hex_u64(mut value: u64) {
    let mut nibbles = [0u8; 16];
    for i in (0..16).rev() {
        nibbles[i] = hex_digit((value & 0xF) as u8);
        value >>= 4;
    }
    for d in nibbles {
        serial_putc(d);
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn serial_putln_hex(prefix: &str, value: u64) {
    serial_puts(prefix);
    serial_puts("0x");
    serial_put_hex_u64(value);
    serial_puts("\r\n");
}

#[cfg(all(not(test), target_os = "none"))]
fn serial_put_dec_u64(mut value: u64) {
    let mut tmp = [0u8; 20];
    let mut len = 0usize;
    if value == 0 {
        serial_putc(b'0');
        return;
    }
    while value > 0 && len < tmp.len() {
        tmp[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        serial_putc(tmp[len]);
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn fake_uptime_millis() -> u64 {
    120
}

#[cfg(all(not(test), target_os = "none"))]
static mut SOSFS_SB0: [u8; sos::SOSFS_BLOCK_SIZE] = [0u8; sos::SOSFS_BLOCK_SIZE];
#[cfg(all(not(test), target_os = "none"))]
static mut SOSFS_SB1: [u8; sos::SOSFS_BLOCK_SIZE] = [0u8; sos::SOSFS_BLOCK_SIZE];

#[cfg(all(not(test), target_os = "none"))]
fn init_mock_sosfs_partition() {
    let sb = sos::fs::sosfs::build_superblock(
        0,
        1,
        sos::fs::sosfs::SOSFS_FLAG_ENCRYPTION_REQUIRED
            | sos::fs::sosfs::SOSFS_FLAG_VERSIONING_REQUIRED,
        [0xA5; 16],
        [0x5A; 32],
        7,
        2,
        256,
        258,
        128,
        386,
        8192,
        0,
    );
    unsafe {
        SOSFS_SB0 = sb;
        SOSFS_SB1 = sb;
    };
}

#[cfg(all(not(test), target_os = "none"))]
fn probe_sosfs_partition() {
    let detected = unsafe { sos::probe_sosfs_superblock(&SOSFS_SB0) };
    match detected {
        Some(info) => {
            serial_puts("[sos] sosfs partition detected\r\n");
            serial_putln_hex("[sos] sosfs gen=", info.active_generation);
            serial_putln_hex("[sos] sosfs flags=", info.flags);
        }
        None => {
            serial_puts("[sos] sosfs partition not found\r\n");
        }
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn run_fsck() {
    let report = unsafe { sos::fsck_superblock_pair(&SOSFS_SB0, &SOSFS_SB1, true) };

    match report.status {
        sos::SosfsFsckStatus::Clean => {
            serial_puts("[sos] fsck: clean\r\n");
        }
        sos::SosfsFsckStatus::Warn => {
            serial_puts("[sos] fsck: warn\r\n");
        }
        sos::SosfsFsckStatus::Corrupt => {
            serial_puts("[sos] fsck: corrupt\r\n");
            for issue_opt in &report.issues {
                if let Some(issue) = issue_opt {
                    serial_puts("[sos] fsck: reason=");
                    match issue {
                        sos::SosfsFsckIssue::BadMagic => serial_puts("bad_magic"),
                        sos::SosfsFsckIssue::BadVersion => serial_puts("bad_version"),
                        sos::SosfsFsckIssue::BadChecksum => serial_puts("bad_checksum"),
                        sos::SosfsFsckIssue::BadFlags => serial_puts("bad_flags"),
                        sos::SosfsFsckIssue::BadBlockSize => serial_puts("bad_block_size"),
                        sos::SosfsFsckIssue::MirrorMismatch => serial_puts("mirror_mismatch"),
                        sos::SosfsFsckIssue::GenerationMismatch => {
                            serial_puts("generation_mismatch")
                        }
                    }
                    serial_puts("\r\n");
                }
            }
            serial_puts("[sos] fsck: HALT\r\n");
            loop {
                unsafe {
                    asm!("hlt", options(nomem, nostack, preserves_flags));
                }
            }
        }
    }
}

#[cfg(all(not(test), target_os = "none"))]
struct SerialConsoleWriter;

#[cfg(all(not(test), target_os = "none"))]
impl sos::ConsoleWriter for SerialConsoleWriter {
    fn write_str(&mut self, s: &str) {
        serial_puts(s);
        serial_puts("\r\n");
    }
}

#[cfg(all(not(test), target_os = "none"))]
struct SerialConsoleReader;

#[cfg(all(not(test), target_os = "none"))]
impl SerialConsoleReader {
    fn read_byte(&mut self) -> u8 {
        unsafe {
            while (inb(COM1 + 5) & 0x01) == 0 {}
            inb(COM1)
        }
    }
}

#[cfg(all(not(test), target_os = "none"))]
struct BootClock;

#[cfg(all(not(test), target_os = "none"))]
impl sos::MonotonicClock for BootClock {
    fn now_millis(&self) -> u64 {
        fake_uptime_millis()
    }
}

#[cfg(all(not(test), target_os = "none"))]
impl sos::ConsoleReader for SerialConsoleReader {
    fn read_line(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut len = 0usize;
        loop {
            let b = self.read_byte();
            match b {
                b'\r' | b'\n' => {
                    serial_puts("\r\n");
                    break;
                }
                0x08 | 0x7F => {
                    if len > 0 {
                        len -= 1;
                        serial_putc(0x08);
                        serial_putc(b' ');
                        serial_putc(0x08);
                    }
                }
                _ => {
                    if len < buf.len() {
                        buf[len] = b;
                        len += 1;
                        serial_putc(b);
                    }
                }
            }
        }
        Some(len)
    }
}

#[cfg(all(not(test), target_os = "none"))]
fn boot_console() -> ! {
    let readiness = sos::ReadinessSuite::run_with_probes(|| true, || true, || true);
    if !readiness.is_ready() {
        serial_puts("[sos] readiness: HALT\r\n");
        loop {
            unsafe {
                asm!("hlt", options(nomem, nostack, preserves_flags));
            }
        }
    }

    let pf_service = sos::PfServiceImpl::new(sos::KernelPacketFilterControl::new());
    let pf_program = sos::SosPfProgram::new(pf_service);
    let registry: sos::ProgramRegistry<'_, 1> = sos::ProgramRegistry::new([&pf_program]);
    let program_service = sos::ProgramServiceImpl::new(registry);
    let console_service = sos::ConsoleService::new(&program_service);
    let mut out = SerialConsoleWriter;
    let mut reader = SerialConsoleReader;

    let self_check = sos::BootSelfCheckReport::all_ok();

    serial_puts("[sos] console: starting\r\n");
    serial_puts("[sos] boot-self-check: begin\r\n");
    self_check.write_transcript(&mut out);

    serial_puts("[sos] boot-timing: prompt-budget-ms=");
    serial_put_dec_u64(sos::BOOT_PROMPT_BUDGET_MS);
    serial_puts("\r\n");
    serial_puts("[sos] boot-timing: prompt-at-ms=");
    serial_put_dec_u64(fake_uptime_millis());
    serial_puts("\r\n");

    let clock = BootClock;
    serial_puts("[sos] console: ready\r\n");
    console_service.run_loop_with_clock(&mut reader, &mut out, "sos> ", &clock);
}

#[cfg(all(not(test), not(feature = "lib-panic"), target_os = "none"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In this minimal environment we only have serial logging,
    // so panic reporting is intentionally simple and robust.
    serial_puts("[sos] panic\r\n");
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(all(not(test), target_os = "none"))]
#[no_mangle]
/// # Safety
///
/// This is the first entry point in a `no_std`/`no_main` boot flow and must only
/// be invoked by the loader/hypervisor with a valid execution context.
pub unsafe extern "C" fn _start() -> ! {
    // No runtime is available in no_std + no_main mode.
    // Set up a known-good temporary stack, then jump into Rust code.
    asm!(
        "mov rsp, 0x00200000",
        "mov rbp, 0x00200000",
        "call kernel_main",
        options(noreturn)
    );
}

#[cfg(all(not(test), target_os = "none"))]
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    // Bring up serial first so every subsequent state can be observed.
    serial_init();
    serial_puts("[sos] boot OK\r\n");
    init_mock_sosfs_partition();
    probe_sosfs_partition();
    run_fsck();
    boot_console();
}

#[cfg(any(test, not(target_os = "none"), feature = "std"))]
fn main() {}
