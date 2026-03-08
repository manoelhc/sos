OUTPUT_FORMAT("elf64-x86-64")
ENTRY(_start)

SECTIONS
{
    . = 0x00100000;
    
    .note : {
        *(.note)
    }
    
    .text : {
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
}
