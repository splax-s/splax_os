/* Splax OS Kernel Linker Script */

ENTRY(_start)

SECTIONS
{
    /* Start at 1MB - standard kernel load address */
    . = 1M;

    /* Multiboot header MUST be in first 8KB of file */
    .multiboot_header : ALIGN(8)
    {
        KEEP(*(.multiboot_header))
    }

    /* Code section */
    .text : ALIGN(4K)
    {
        *(.text .text.*)
    }

    /* Read-only data */
    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    /* Read-write data */
    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    /* BSS section */
    .bss : ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Discard unnecessary sections */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
        *(.note*)
    }
}
