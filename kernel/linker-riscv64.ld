/*
 * RISC-V 64-bit Linker Script for Splax OS
 *
 * Memory Layout:
 *   0x80000000 - Kernel start (OpenSBI entry point)
 *   0x80200000 - Alternative kernel start (after M-mode firmware)
 */

OUTPUT_ARCH(riscv)
ENTRY(_start)

/* RISC-V typically starts at 0x80000000 or 0x80200000 */
KERNEL_BASE = 0x80200000;

SECTIONS
{
    . = KERNEL_BASE;

    /* Code section */
    .text ALIGN(4K) : AT(ADDR(.text))
    {
        __text_start = .;
        KEEP(*(.text.boot))     /* Boot code first */
        *(.text .text.*)
        __text_end = .;
    }

    /* Read-only data */
    .rodata ALIGN(4K) : AT(ADDR(.rodata))
    {
        __rodata_start = .;
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        __rodata_end = .;
    }

    /* Initialized data */
    .data ALIGN(4K) : AT(ADDR(.data))
    {
        __data_start = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        __data_end = .;
    }

    /* Global Offset Table (for PIC code) */
    .got ALIGN(8) :
    {
        *(.got)
        *(.got.plt)
    }

    /* Thread Local Storage */
    .tdata ALIGN(8) :
    {
        __tdata_start = .;
        *(.tdata .tdata.*)
        __tdata_end = .;
    }

    .tbss ALIGN(8) :
    {
        __tbss_start = .;
        *(.tbss .tbss.*)
        __tbss_end = .;
    }

    /* Uninitialized data */
    .bss ALIGN(4K) (NOLOAD) : AT(ADDR(.bss))
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Stack (per-hart) */
    . = ALIGN(4K);
    __stack_start = .;
    . += 64K;  /* 64KB stack per hart, max 4 harts = 256KB */
    __stack_end = .;

    /* Heap */
    . = ALIGN(4K);
    __heap_start = .;
    . += 16M;  /* 16MB heap */
    __heap_end = .;

    /* Kernel end marker */
    __kernel_end = .;

    /* Discard unnecessary sections */
    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}

/* Provide symbols for kernel */
PROVIDE(__kernel_start = KERNEL_BASE);
PROVIDE(__kernel_size = __kernel_end - __kernel_start);
