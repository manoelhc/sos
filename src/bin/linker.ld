OUTPUT_FORMAT("elf64-x86-64")
ENTRY(_start)

phys = 0x00100000;
kernel_size = 0x200000;

SECTIONS
{
    .text phys : AT(phys) {
        code = .;
        *(.text .text.*)
        *(.rodata .rodata.*)
        . = ALIGN(4096);
    }

    .data : {
        *(.data .data.*)
        . = ALIGN(4096);
    }

    .bss : {
        *(.bss .bss.*)
        . = ALIGN(4096);
    }

    kernel_end = .;
}
