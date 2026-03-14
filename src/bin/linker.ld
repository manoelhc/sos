OUTPUT_FORMAT("elf64-x86-64")
ENTRY(_start)

/*
 * Linker layout for a tiny 64-bit kernel-like binary.
 *
 * The image is linked at 0x0010_0000 (1 MiB), a conventional low-memory
 * load address often used by bootloaders/hypervisors in simple setups.
 */
SECTIONS
{
    . = 0x00100000;

    /*
     * GRUB scans the first part of the kernel image for this header.
     * Keep it early and prevent linker garbage collection.
     */
    .multiboot2 : {
        KEEP(*(.multiboot2))
    }
    
    /*
     * Keep ELF notes (including .note.Xen) available to the loader.
     */
    .note : {
        *(.note)
        *(.note.*)
    }
    
    /*
     * Executable code and read-only data.
     * Align to page size to keep section boundaries page-friendly.
     */
    .text : {
        *(.text .text.*)
        *(.rodata .rodata.*)
        . = ALIGN(4096);
    }

    /* Writable initialized data. */
    .data : {
        *(.data .data.*)
        . = ALIGN(4096);
    }

    /* Zero-initialized data. */
    .bss : {
        *(.bss .bss.*)
        . = ALIGN(4096);
    }
}
