/*
 * AArch64 Boot Assembly for Splax OS
 *
 * Entry point for the kernel on ARM64.
 * Assumes we're running at EL1 (typical for QEMU -kernel).
 */

.section .text.boot
.global _start
.extern kernel_main
.extern __bss_start
.extern __bss_end
.extern __stack_top
.extern __exception_vectors

_start:
    /* Disable interrupts */
    msr daifset, #0xf

    /* Check if we're on CPU 0 */
    mrs x0, mpidr_el1
    and x0, x0, #0xff
    cbnz x0, .park_secondary

    /* Set up stack pointer */
    ldr x0, =__stack_top
    mov sp, x0

    /* Clear BSS section */
    ldr x0, =__bss_start
    ldr x1, =__bss_end
.clear_bss:
    cmp x0, x1
    b.ge .bss_done
    str xzr, [x0], #8
    b .clear_bss
.bss_done:

    /* Set up exception vector table */
    ldr x0, =__exception_vectors
    msr vbar_el1, x0
    isb

    /* Jump to Rust kernel main */
    bl kernel_main

    /* If kernel returns, halt */
.halt:
    wfe
    b .halt

/* Park secondary CPUs */
.park_secondary:
    wfe
    b .park_secondary

/*
 * Exception Vector Table
 * 
 * 4 exception types × 4 contexts = 16 entries
 * Each entry is 128 bytes (32 instructions)
 */
.section .text.vectors
.balign 2048
.global __exception_vectors
__exception_vectors:

/* Current EL with SP0 */
.balign 128
curr_el_sp0_sync:
    b exception_sync_sp0
.balign 128
curr_el_sp0_irq:
    b exception_irq_sp0
.balign 128
curr_el_sp0_fiq:
    b exception_fiq_sp0
.balign 128
curr_el_sp0_serror:
    b exception_serror_sp0

/* Current EL with SPx */
.balign 128
curr_el_spx_sync:
    b exception_sync_spx
.balign 128
curr_el_spx_irq:
    b exception_irq_spx
.balign 128
curr_el_spx_fiq:
    b exception_fiq_spx
.balign 128
curr_el_spx_serror:
    b exception_serror_spx

/* Lower EL using AArch64 */
.balign 128
lower_el_aarch64_sync:
    b exception_sync_lower
.balign 128
lower_el_aarch64_irq:
    b exception_irq_lower
.balign 128
lower_el_aarch64_fiq:
    b exception_fiq_lower
.balign 128
lower_el_aarch64_serror:
    b exception_serror_lower

/* Lower EL using AArch32 */
.balign 128
lower_el_aarch32_sync:
    b exception_sync_lower32
.balign 128
lower_el_aarch32_irq:
    b exception_irq_lower32
.balign 128
lower_el_aarch32_fiq:
    b exception_fiq_lower32
.balign 128
lower_el_aarch32_serror:
    b exception_serror_lower32

/*
 * Exception handlers
 * Save context and call Rust handler
 */

.macro SAVE_CONTEXT
    /* Allocate stack space for context */
    sub sp, sp, #272

    /* Save general purpose registers x0-x30 */
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]

    /* Save special registers */
    add x0, sp, #272        /* Original SP */
    str x0, [sp, #248]
    mrs x0, elr_el1
    str x0, [sp, #256]
    mrs x0, spsr_el1
    str x0, [sp, #264]
.endm

.macro RESTORE_CONTEXT
    /* Restore special registers */
    ldr x0, [sp, #256]
    msr elr_el1, x0
    ldr x0, [sp, #264]
    msr spsr_el1, x0

    /* Restore general purpose registers */
    ldp x0, x1, [sp, #0]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]

    /* Restore stack pointer */
    add sp, sp, #272
.endm

/* Current EL with SP0 handlers */
exception_sync_sp0:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #0          /* Exception type: sync */
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_irq_sp0:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #1          /* Exception type: IRQ */
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_fiq_sp0:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #2          /* Exception type: FIQ */
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_serror_sp0:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #3          /* Exception type: SError */
    bl exception_handler
    RESTORE_CONTEXT
    eret

/* Current EL with SPx handlers */
exception_sync_spx:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #0
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_irq_spx:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #1
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_fiq_spx:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #2
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_serror_spx:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #3
    bl exception_handler
    RESTORE_CONTEXT
    eret

/* Lower EL AArch64 handlers */
exception_sync_lower:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #0
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_irq_lower:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #1
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_fiq_lower:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #2
    bl exception_handler
    RESTORE_CONTEXT
    eret

exception_serror_lower:
    SAVE_CONTEXT
    mov x0, sp
    mov x1, #3
    bl exception_handler
    RESTORE_CONTEXT
    eret

/* Lower EL AArch32 handlers (not used, just halt) */
exception_sync_lower32:
exception_irq_lower32:
exception_fiq_lower32:
exception_serror_lower32:
    wfe
    b exception_sync_lower32
