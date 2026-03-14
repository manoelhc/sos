# SOS `src/bin` Boot Stub Documentation

This directory contains a very small `no_std` Rust binary intended to boot in a Xen-oriented environment and emit basic status logs over a serial port.

## What this code does

- Defines a Xen ELF note (`.note.Xen`) that points to `_start`.
- Provides low-level x86 port I/O helpers (`inb`/`outb`).
- Initializes the COM1 UART (16550-compatible).
- Prints a boot marker (`[sos] boot OK`) over serial.
- Enters an idle loop using `hlt`.

## High-level boot flow

```mermaid
flowchart TD
    A[Hypervisor/Loader loads ELF image] --> B[Read .note.Xen]
    B --> C[Jump to _start]
    C --> D[Set stack pointers rsp/rbp = 0x00200000]
    D --> E[Call kernel_main]
    E --> F[serial_init configures COM1]
    F --> G[serial_puts logs boot message]
    G --> H[Loop forever: hlt]
```

## Serial pipeline

```mermaid
sequenceDiagram
    participant K as kernel_main
    participant S as serial_puts
    participant C as serial_putc
    participant U as UART COM1

    K->>S: serial_puts("[sos] boot OK\\r\\n")
    loop for each byte
        S->>C: serial_putc(byte)
        C->>U: poll LSR (COM1+5) bit 5 until ready
        C->>U: write byte to THR (COM1)
    end
```

## Memory/link layout

```mermaid
flowchart TB
    A[ELF image base 0x00100000] --> B[.note]
    A --> C[.text + .rodata]\npage-aligned
    A --> D[.data]\npage-aligned
    A --> E[.bss]\npage-aligned
```

## Important implementation notes

- `#![no_std]` and `#![no_main]`: no Rust runtime, allocator, or default entry point.
- `_start` is the first Rust-visible symbol and manually sets up stack state.
- Panic handling only logs a fixed message and halts forever.
- UART output is polling-based (no interrupts), which keeps early-boot behavior deterministic.

## File guide

- `src/bin/main.rs`: entry point, serial driver primitives, panic handler, idle loop.
- `src/bin/linker.ld`: linker script that places sections and preserves ELF notes.
