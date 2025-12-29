/* Splax Kernel Linker Script for x86_64 */
ENTRY(_start)

SECTIONS
{
    /* Start at 1MB - where multiboot loaders typically load the kernel */
    . = 1M;

    /* Multiboot header must be in the first 8KB - place it FIRST */
    .multiboot_header : ALIGN(4)
    {
        KEEP(*(.multiboot_header))
    }

    /* GOT section after multiboot */
    .got : ALIGN(8)
    {
        *(.got .got.*)
    }

    /* Text section */
    .text : ALIGN(4K)
    {
        *(.text .text.*)
    }

    /* Read-only data */
    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    /* Initialized data */
    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    /* Uninitialized data (BSS) */
    .bss : ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Stack */
    . = ALIGN(4K);
    __stack_bottom = .;
    . += 64K;
    __stack_top = .;

    /* Kernel end marker */
    __kernel_end = .;

    /* Discard unnecessary sections */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.note .note.*)
        *(.comment)
    }
}
