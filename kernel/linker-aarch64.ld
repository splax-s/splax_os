/*
 * AArch64 Linker Script for Splax OS
 *
 * Memory layout for QEMU virt machine.
 * Kernel is loaded at 0x40080000.
 */

ENTRY(_start)

SECTIONS
{
    /* Kernel load address for QEMU virt machine */
    . = 0x40080000;
    
    __kernel_start = .;

    /* Boot code must come first */
    .text.boot : {
        *(.text.boot)
    }

    /* Exception vectors (2KB aligned) */
    . = ALIGN(2048);
    .text.vectors : {
        *(.text.vectors)
    }

    /* Main code section */
    .text : {
        *(.text .text.*)
    }

    /* Read-only data */
    . = ALIGN(4096);
    .rodata : {
        *(.rodata .rodata.*)
    }

    /* Read-write data */
    . = ALIGN(4096);
    .data : {
        *(.data .data.*)
    }

    /* BSS section */
    . = ALIGN(4096);
    __bss_start = .;
    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    }
    __bss_end = .;

    /* Stack (64KB) */
    . = ALIGN(4096);
    __stack_bottom = .;
    . += 64K;
    __stack_top = .;

    /* Heap */
    . = ALIGN(4096);
    __heap_start = .;
    . += 16M;
    __heap_end = .;

    __kernel_end = .;

    /* Discard unnecessary sections */
    /DISCARD/ : {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}

/* Provide symbols for Rust code */
PROVIDE(__kernel_size = __kernel_end - __kernel_start);
PROVIDE(__bss_size = __bss_end - __bss_start);
PROVIDE(__stack_size = __stack_top - __stack_bottom);
PROVIDE(__heap_size = __heap_end - __heap_start);
