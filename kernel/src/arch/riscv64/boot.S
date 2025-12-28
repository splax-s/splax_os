/*
 * RISC-V 64-bit Boot Assembly for Splax OS
 *
 * This code runs on all harts (hardware threads) after OpenSBI/SBI.
 * Hart 0 performs kernel initialization while other harts wait.
 *
 * RISC-V Privilege Levels:
 *   M-mode (Machine)     - Firmware/SBI
 *   S-mode (Supervisor)  - Kernel (we run here)
 *   U-mode (User)        - Applications
 */

.section .text.boot
.global _start
.global _hart_entry

/*
 * _start - Entry point for all harts
 *
 * On entry:
 *   a0 = hartid (hardware thread ID)
 *   a1 = device tree blob physical address
 *
 * Register conventions:
 *   t0-t6  = temporaries
 *   a0-a7  = arguments/return values
 *   s0-s11 = saved registers (callee-saved)
 *   gp     = global pointer (for relaxation)
 *   tp     = thread pointer (per-hart data)
 *   sp     = stack pointer
 *   ra     = return address
 */
_start:
    /* Disable interrupts during initialization */
    csrw    sie, zero
    csrw    sip, zero

    /* Set up global pointer for linker relaxation */
.option push
.option norelax
    la      gp, __global_pointer$
.option pop

    /* Store hartid in tp (thread pointer) for per-hart identification */
    mv      tp, a0

    /* Calculate per-hart stack pointer */
    /* Stack layout: __stack_end - (hartid * 64K) */
    la      t0, __stack_end
    li      t1, 0x10000         /* 64KB per hart */
    mul     t2, a0, t1          /* hartid * 64KB */
    sub     sp, t0, t2          /* sp = __stack_end - offset */

    /* Hart 0 initializes the kernel, others wait */
    bnez    a0, _secondary_hart

    /* === Hart 0: Primary initialization === */

    /* Clear BSS section */
    la      t0, __bss_start
    la      t1, __bss_end
_clear_bss:
    bgeu    t0, t1, _bss_done
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       _clear_bss
_bss_done:

    /* Save device tree pointer for kernel */
    la      t0, _dtb_ptr
    sd      a1, 0(t0)

    /* Set up trap vector (direct mode) */
    la      t0, _trap_vector
    csrw    stvec, t0

    /* Enable Supervisor interrupts */
    li      t0, 0x222           /* SEIE | STIE | SSIE */
    csrw    sie, t0

    /* Call Rust kernel_main(hartid, dtb_ptr) */
    mv      a0, tp              /* a0 = hartid */
    ld      a1, _dtb_ptr        /* a1 = dtb_ptr */
    call    kernel_main

    /* If kernel_main returns, halt */
_halt:
    wfi
    j       _halt

/* Secondary hart entry point */
_secondary_hart:
    /* Wait for release from primary hart */
    fence

    /* Check if kernel is ready */
    la      t0, _kernel_ready
    ld      t1, 0(t0)
    beqz    t1, _secondary_wait

    /* Kernel is ready, jump to hart entry */
    j       _hart_entry

_secondary_wait:
    wfi                         /* Wait for interrupt */
    j       _secondary_hart

/* Hart entry after kernel initialization */
_hart_entry:
    /* Set up trap vector */
    la      t0, _trap_vector
    csrw    stvec, t0

    /* Enable interrupts */
    li      t0, 0x222
    csrw    sie, t0

    /* Call Rust hart_entry(hartid) */
    mv      a0, tp
    call    hart_entry

    /* Should not return */
_hart_halt:
    wfi
    j       _hart_halt

/*
 * Trap Vector (Supervisor mode)
 *
 * All traps come here:
 *   - Interrupts (timer, software, external)
 *   - Exceptions (page fault, illegal instruction, syscall)
 */
.align 4
_trap_vector:
    /* Save context on stack */
    addi    sp, sp, -256

    /* Save general purpose registers */
    sd      ra,   0(sp)
    sd      sp,   8(sp)         /* Original sp saved later */
    sd      gp,  16(sp)
    sd      tp,  24(sp)
    sd      t0,  32(sp)
    sd      t1,  40(sp)
    sd      t2,  48(sp)
    sd      s0,  56(sp)
    sd      s1,  64(sp)
    sd      a0,  72(sp)
    sd      a1,  80(sp)
    sd      a2,  88(sp)
    sd      a3,  96(sp)
    sd      a4, 104(sp)
    sd      a5, 112(sp)
    sd      a6, 120(sp)
    sd      a7, 128(sp)
    sd      s2, 136(sp)
    sd      s3, 144(sp)
    sd      s4, 152(sp)
    sd      s5, 160(sp)
    sd      s6, 168(sp)
    sd      s7, 176(sp)
    sd      s8, 184(sp)
    sd      s9, 192(sp)
    sd      s10, 200(sp)
    sd      s11, 208(sp)
    sd      t3, 216(sp)
    sd      t4, 224(sp)
    sd      t5, 232(sp)
    sd      t6, 240(sp)

    /* Read trap cause and value */
    csrr    a0, scause
    csrr    a1, stval
    csrr    a2, sepc
    mv      a3, sp              /* Context pointer */

    /* Call Rust trap handler */
    call    trap_handler

    /* Restore context */
    ld      ra,   0(sp)
    ld      gp,  16(sp)
    ld      tp,  24(sp)
    ld      t0,  32(sp)
    ld      t1,  40(sp)
    ld      t2,  48(sp)
    ld      s0,  56(sp)
    ld      s1,  64(sp)
    ld      a0,  72(sp)
    ld      a1,  80(sp)
    ld      a2,  88(sp)
    ld      a3,  96(sp)
    ld      a4, 104(sp)
    ld      a5, 112(sp)
    ld      a6, 120(sp)
    ld      a7, 128(sp)
    ld      s2, 136(sp)
    ld      s3, 144(sp)
    ld      s4, 152(sp)
    ld      s5, 160(sp)
    ld      s6, 168(sp)
    ld      s7, 176(sp)
    ld      s8, 184(sp)
    ld      s9, 192(sp)
    ld      s10, 200(sp)
    ld      s11, 208(sp)
    ld      t3, 216(sp)
    ld      t4, 224(sp)
    ld      t5, 232(sp)
    ld      t6, 240(sp)

    addi    sp, sp, 256
    sret

/* Data section */
.section .data
.align 8

/* Device tree blob pointer (set during boot) */
.global _dtb_ptr
_dtb_ptr:
    .dword 0

/* Kernel ready flag (set when primary hart finishes init) */
.global _kernel_ready
_kernel_ready:
    .dword 0

/* Global pointer anchor */
.section .sdata
.global __global_pointer$
__global_pointer$:
    .dword 0
