# RISC-V 64-bit Architecture Support

> Comprehensive documentation for Splax OS RISC-V port.

## Overview

Splax OS includes full RISC-V 64-bit (RV64GC) architecture support, making it the **third supported architecture** alongside x86_64 and AArch64. The port leverages RISC-V's clean, modern ISA design and integrates with OpenSBI firmware for platform abstraction.

## Architecture Features

### Supported Extensions

| Extension | Description | Status |
|-----------|-------------|--------|
| **RV64I** | Base 64-bit integer ISA | ✅ Required |
| **M** | Integer multiplication/division | ✅ Enabled |
| **A** | Atomic instructions (LR/SC, AMO) | ✅ Enabled |
| **F** | Single-precision floating-point | ✅ Enabled |
| **D** | Double-precision floating-point | ✅ Enabled |
| **C** | Compressed instructions | ✅ Enabled |
| **Zicsr** | CSR instructions | ✅ Implicit |
| **Zifencei** | Instruction-fetch fence | ✅ Implicit |

Target triple: `riscv64gc-unknown-none-elf`

---

## Privilege Levels

RISC-V defines three privilege levels. Splax OS runs at Supervisor (S-mode):

```
┌─────────────────────────────────────────────────────────────┐
│  U-mode (User)          - User applications                 │
│  Privilege Level 0                                          │
├─────────────────────────────────────────────────────────────┤
│  S-mode (Supervisor)    - Splax OS Kernel                   │
│  Privilege Level 1      ← We are here                       │
├─────────────────────────────────────────────────────────────┤
│  M-mode (Machine)       - OpenSBI Firmware                  │
│  Privilege Level 3      - Hardware abstraction              │
└─────────────────────────────────────────────────────────────┘
```

### Why S-mode?

- OpenSBI handles M-mode tasks (timer, console, power management)
- S-mode has all kernel functionality needed
- Portable across different RISC-V platforms
- Standard approach for RISC-V operating systems

---

## Memory Layout

### Physical Memory Map (QEMU virt)

```
0x0000_0000 - 0x000F_FFFF    QEMU MMIO (CLINT, PLIC, UART)
0x0C00_0000 - 0x0FFF_FFFF    PLIC (Platform Level Interrupt Controller)
0x1000_0000 - 0x1000_0FFF    UART (NS16550A compatible)
0x8000_0000 - 0x8001_FFFF    OpenSBI Firmware
0x8020_0000 - ...            Kernel (our code starts here)
```

### Virtual Address Space (Sv39)

```
┌─────────────────────────────────────────────────────────────┐
│ 0xFFFF_FFFF_FFFF_FFFF  Top of address space                 │
├─────────────────────────────────────────────────────────────┤
│                        Kernel space                         │
│                        (mapped 1:1 or higher-half)          │
├─────────────────────────────────────────────────────────────┤
│ 0x0000_003F_FFFF_FFFF  Top of user space (Sv39)            │
├─────────────────────────────────────────────────────────────┤
│                        User space                           │
│                        (per-process mappings)               │
├─────────────────────────────────────────────────────────────┤
│ 0x0000_0000_0000_0000  Bottom of address space             │
└─────────────────────────────────────────────────────────────┘
```

---

## Control and Status Registers (CSRs)

### Supervisor CSRs Used

| CSR | Name | Description |
|-----|------|-------------|
| `sstatus` | Supervisor Status | Interrupt enable, privilege mode |
| `sie` | Supervisor Interrupt Enable | Which interrupts are enabled |
| `sip` | Supervisor Interrupt Pending | Which interrupts are pending |
| `stvec` | Supervisor Trap Vector | Trap handler address |
| `sscratch` | Supervisor Scratch | Temporary storage for trap handler |
| `sepc` | Supervisor Exception PC | Faulting instruction address |
| `scause` | Supervisor Cause | Trap cause code |
| `stval` | Supervisor Trap Value | Additional trap info (address, etc.) |
| `satp` | Supervisor Address Translation | Page table base + mode |
| `time` | Timer | Current time value (read-only) |
| `cycle` | Cycle Counter | CPU cycles (read-only) |

### CSR Access Functions

```rust
// kernel/src/arch/riscv64/csr.rs

// Read CSR
pub fn read_sstatus() -> u64;
pub fn read_scause() -> u64;
pub fn read_stval() -> u64;
pub fn read_sepc() -> u64;
pub fn read_time() -> u64;

// Write CSR
pub fn write_sstatus(val: u64);
pub fn write_stvec(val: u64);
pub fn write_sscratch(val: u64);
pub fn write_satp(val: u64);

// Set/Clear bits
pub fn set_sstatus_sie();   // Enable supervisor interrupts
pub fn clear_sstatus_sie(); // Disable supervisor interrupts
```

---

## Supervisor Binary Interface (SBI)

SBI provides a standardized interface between S-mode OS and M-mode firmware (OpenSBI).

### Supported Extensions

| Extension | EID | Description |
|-----------|-----|-------------|
| **Base** | 0x10 | SBI version, implementation info |
| **Timer** | 0x54494D45 | Set timer interrupt |
| **IPI** | 0x735049 | Inter-processor interrupts |
| **HSM** | 0x48534D | Hart State Management |
| **SRST** | 0x53525354 | System Reset |

### SBI Calls

```rust
// kernel/src/arch/riscv64/sbi.rs

/// Set timer interrupt for current hart
pub fn set_timer(stime: u64) -> SbiResult;

/// Send IPI to specified harts (bitmask)
pub fn send_ipi(hart_mask: u64) -> SbiResult;

/// System shutdown
pub fn shutdown() -> !;

/// System reboot
pub fn reboot() -> !;

/// Console output (legacy)
pub fn console_putchar(ch: u8);

/// Console input (legacy)  
pub fn console_getchar() -> Option<u8>;
```

### SBI Call Convention

```
ecall instruction:
  a7 = Extension ID (EID)
  a6 = Function ID (FID)
  a0-a5 = Arguments

Return:
  a0 = Error code (0 = success)
  a1 = Return value
```

---

## Interrupt Handling

### PLIC (Platform Level Interrupt Controller)

PLIC handles external interrupts from devices.

```
┌─────────────────────────────────────────────────────────────┐
│                         PLIC                                │
│                   Base: 0x0C00_0000                        │
├─────────────────────────────────────────────────────────────┤
│  Source 1 (UART)      ──┐                                  │
│  Source 2 (VirtIO)    ──┼──→ Priority → Hart 0 Context    │
│  Source 3 (...)       ──┘            → Hart 1 Context     │
│  ...                                  → ...                │
│  Source 1023                                               │
└─────────────────────────────────────────────────────────────┘
```

#### PLIC Registers

| Offset | Register | Description |
|--------|----------|-------------|
| 0x0000 + src*4 | Priority[src] | Interrupt priority (0-7) |
| 0x1000 + ctx*0x80 | Enable[ctx] | Enable bits for context |
| 0x200000 + ctx*0x1000 | Threshold[ctx] | Priority threshold |
| 0x200004 + ctx*0x1000 | Claim/Complete | Claim or complete interrupt |

#### PLIC API

```rust
// kernel/src/arch/riscv64/plic.rs

/// Initialize PLIC
pub fn init();

/// Initialize PLIC for current hart
pub fn hart_init(hartid: usize);

/// Set interrupt source priority (0-7)
pub fn set_priority(source: u32, priority: u8);

/// Enable interrupt source for hart
pub fn enable_source(hartid: usize, source: u32);

/// Claim pending interrupt (returns source number)
pub fn claim(hartid: usize) -> u32;

/// Complete interrupt handling
pub fn complete(hartid: usize, source: u32);
```

### Trap Handler

All traps (interrupts and exceptions) go through a single handler:

```rust
// kernel/src/arch/riscv64/trap.rs

#[no_mangle]
pub extern "C" fn trap_handler(
    scause: u64,
    stval: u64, 
    sepc: u64,
    context: &mut TrapContext
) {
    if is_interrupt(scause) {
        handle_interrupt(scause, context);
    } else {
        handle_exception(scause, stval, sepc, context);
    }
}
```

### Interrupt Types

| Cause | Description | Handler |
|-------|-------------|---------|
| 1 (S-mode software) | IPI from another hart | `handle_ipi()` |
| 5 (S-mode timer) | Timer interrupt | `timer::handle_interrupt()` |
| 9 (S-mode external) | External device | `plic::handle_interrupt()` |

### Exception Types

| Cause | Description | Handler |
|-------|-------------|---------|
| 0 | Instruction address misaligned | Panic |
| 2 | Illegal instruction | Panic |
| 5 | Load access fault | `handle_page_fault()` |
| 7 | Store access fault | `handle_page_fault()` |
| 8 | Environment call from U-mode | `handle_syscall()` |
| 12 | Instruction page fault | `handle_page_fault()` |
| 13 | Load page fault | `handle_page_fault()` |
| 15 | Store page fault | `handle_page_fault()` |

---

## Memory Management Unit (MMU)

### Paging Modes

| Mode | Levels | Virtual Address Bits | Page Size |
|------|--------|---------------------|-----------|
| Bare | 0 | N/A | No translation |
| **Sv39** | 3 | 39 bits (512 GB) | 4 KB |
| Sv48 | 4 | 48 bits (256 TB) | 4 KB |
| Sv57 | 5 | 57 bits | 4 KB |

Splax OS uses **Sv39** by default for broad compatibility.

### Sv39 Virtual Address Format

```
63        39 38       30 29       21 20       12 11        0
┌──────────┬───────────┬───────────┬───────────┬───────────┐
│ Sign Ext │   VPN[2]  │   VPN[1]  │   VPN[0]  │  Offset   │
│  25 bits │   9 bits  │   9 bits  │   9 bits  │  12 bits  │
└──────────┴───────────┴───────────┴───────────┴───────────┘
```

### Page Table Entry Format

```
63    54 53         10 9  8 7 6 5 4 3 2 1 0
┌───────┬─────────────┬────┬─┬─┬─┬─┬─┬─┬─┬─┐
│Reserved│    PPN     │RSW │D│A│G│U│X│W│R│V│
│10 bits │   44 bits  │2 b │1│1│1│1│1│1│1│1│
└───────┴─────────────┴────┴─┴─┴─┴─┴─┴─┴─┴─┘

V = Valid
R = Readable
W = Writable
X = Executable
U = User-accessible
G = Global
A = Accessed
D = Dirty
RSW = Reserved for software
PPN = Physical Page Number
```

### SATP Register

```
63    60 59           44 43                 0
┌───────┬───────────────┬───────────────────┐
│ MODE  │     ASID      │       PPN         │
│4 bits │    16 bits    │     44 bits       │
└───────┴───────────────┴───────────────────┘

MODE values:
  0 = Bare (no translation)
  8 = Sv39
  9 = Sv48
  10 = Sv57
```

### MMU API

```rust
// kernel/src/arch/riscv64/mmu.rs

/// Enable Sv39 paging
pub unsafe fn enable_sv39(root_table_phys: u64);

/// Enable Sv48 paging
pub unsafe fn enable_sv48(root_table_phys: u64);

/// Disable paging (bare mode)
pub unsafe fn disable_paging();

/// Map a page
pub fn map_page(
    root: &mut PageTable,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    alloc_page: impl Fn() -> Option<u64>,
) -> Result<(), &'static str>;

/// Unmap a page
pub fn unmap_page(root: &mut PageTable, vaddr: u64) -> Result<u64, &'static str>;

/// Translate virtual to physical address
pub fn translate(root: &PageTable, vaddr: u64) -> Option<u64>;
```

---

## Timer

RISC-V timer is accessed via SBI (M-mode handles the actual timer hardware).

### Timer API

```rust
// kernel/src/arch/riscv64/timer.rs

/// Initialize timer subsystem
pub fn init();

/// Initialize timer for current hart
pub fn hart_init();

/// Get current time value
pub fn get_time() -> u64;

/// Set next timer interrupt
pub fn set_next_timer(ticks_from_now: u64);

/// Handle timer interrupt
pub fn handle_interrupt();
```

### Timer Configuration

```rust
const TIMER_FREQUENCY: u64 = 10_000_000;  // 10 MHz (QEMU)
const TIMER_INTERVAL: u64 = TIMER_FREQUENCY / 100;  // 10ms ticks
```

---

## UART Console

NS16550A-compatible UART at 0x1000_0000 (QEMU virt).

### UART Registers

| Offset | Register | Access | Description |
|--------|----------|--------|-------------|
| 0x00 | RBR/THR | R/W | Receive/Transmit buffer |
| 0x01 | IER | R/W | Interrupt Enable |
| 0x02 | IIR/FCR | R/W | Interrupt ID / FIFO Control |
| 0x03 | LCR | R/W | Line Control |
| 0x04 | MCR | R/W | Modem Control |
| 0x05 | LSR | R | Line Status |
| 0x06 | MSR | R | Modem Status |

### UART API

```rust
// kernel/src/arch/riscv64/uart.rs

/// Initialize UART (8N1, FIFO enabled)
pub fn init();

/// Output character
pub fn putchar(ch: u8);

/// Read character (non-blocking)
pub fn getchar() -> Option<u8>;

/// Output string
pub fn puts(s: &str);

/// Handle UART interrupt
pub fn handle_interrupt();
```

---

## Multi-Hart (SMP) Support

### Boot Sequence

```
┌─────────────────────────────────────────────────────────────┐
│  1. OpenSBI starts all harts                                │
│  2. All harts jump to _start                                │
│  3. Hart 0 (BSP) initializes:                              │
│     - Clear BSS                                             │
│     - Set up trap vector                                    │
│     - Initialize kernel                                     │
│  4. Secondary harts wait in WFI loop                       │
│  5. BSP signals secondaries to start                        │
│  6. Secondaries initialize per-hart state                   │
└─────────────────────────────────────────────────────────────┘
```

### Hart Identification

```rust
/// Get current hart ID (stored in tp register)
pub fn hartid() -> usize {
    let id: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) id);
    }
    id
}
```

### Inter-Processor Interrupts (IPI)

```rust
// Send IPI via SBI
sbi::send_ipi(hart_mask);

// Handle IPI in trap handler
fn handle_ipi() {
    // Clear IPI pending bit
    csr::clear_sip_ssip();
    
    // Handle IPI (e.g., TLB shootdown, reschedule)
}
```

---

## Context Switching

### CPU Context Structure

```rust
#[repr(C)]
pub struct CpuContext {
    pub ra: u64,        // Return address
    pub sp: u64,        // Stack pointer
    pub gp: u64,        // Global pointer
    pub tp: u64,        // Thread pointer (hart ID)
    pub s: [u64; 12],   // Saved registers s0-s11
    pub sstatus: u64,   // Supervisor status
    pub sepc: u64,      // Exception program counter
}
```

### Context Switch Assembly

```asm
.global context_switch
context_switch:
    # Save current context (a0 = old context ptr)
    sd ra, 0(a0)
    sd sp, 8(a0)
    sd gp, 16(a0)
    sd tp, 24(a0)
    sd s0, 32(a0)
    sd s1, 40(a0)
    # ... save s2-s11 ...
    csrr t0, sstatus
    sd t0, 128(a0)
    csrr t0, sepc
    sd t0, 136(a0)
    
    # Load new context (a1 = new context ptr)
    ld ra, 0(a1)
    ld sp, 8(a1)
    ld gp, 16(a1)
    ld tp, 24(a1)
    ld s0, 32(a1)
    ld s1, 40(a1)
    # ... load s2-s11 ...
    ld t0, 128(a1)
    csrw sstatus, t0
    ld t0, 136(a1)
    csrw sepc, t0
    
    ret
```

---

## Building for RISC-V

### Prerequisites

```bash
# Install Rust RISC-V target
rustup target add riscv64gc-unknown-none-elf

# Install QEMU
brew install qemu  # macOS
apt install qemu-system-riscv64  # Linux
```

### Build Commands

```bash
# Build kernel for RISC-V
cargo build --package splax_kernel \
    --target splax_kernel_riscv64.json \
    -Z build-std=core,alloc \
    --release

# The kernel ELF is at:
# target/riscv64gc-unknown-none-elf/release/splax_kernel
```

### Running in QEMU

```bash
qemu-system-riscv64 \
    -machine virt \
    -cpu rv64 \
    -smp 4 \
    -m 512M \
    -bios default \
    -kernel target/riscv64gc-unknown-none-elf/release/splax_kernel \
    -nographic
```

---

## Target Specification

`splax_kernel_riscv64.json`:

```json
{
    "llvm-target": "riscv64",
    "data-layout": "e-m:e-p:64:64-i64:64-i128:128-n32:64-S128",
    "arch": "riscv64",
    "target-endian": "little",
    "target-pointer-width": "64",
    "target-c-int-width": "32",
    "os": "none",
    "executables": true,
    "linker-flavor": "ld.lld",
    "linker": "rust-lld",
    "panic-strategy": "abort",
    "disable-redzone": true,
    "features": "+m,+a,+f,+d,+c",
    "code-model": "medium",
    "pre-link-args": {
        "ld.lld": ["-Tkernel/linker-riscv64.ld"]
    }
}
```

---

## File Structure

```
kernel/src/arch/riscv64/
├── mod.rs      # Module root, CpuContext, init()
├── boot.S      # Boot assembly, hart management
├── csr.rs      # CSR read/write functions
├── sbi.rs      # SBI interface calls
├── plic.rs     # PLIC interrupt controller
├── timer.rs    # Timer subsystem
├── uart.rs     # UART console driver
├── trap.rs     # Trap handler
└── mmu.rs      # MMU/paging (Sv39/Sv48)
```

---

## Comparison with Other Architectures

| Feature | x86_64 | AArch64 | RISC-V |
|---------|--------|---------|--------|
| Privilege levels | Ring 0-3 | EL0-EL3 | U/S/M modes |
| Interrupt controller | APIC/IOAPIC | GIC | PLIC |
| Timer | LAPIC Timer | Generic Timer | SBI Timer |
| Paging | 4-level (PML4) | 4-level (4K pages) | 3-level (Sv39) |
| Syscall instruction | `syscall` | `svc` | `ecall` |
| Firmware | UEFI/BIOS | UEFI | OpenSBI |
| Boot protocol | Multiboot2 | Device Tree | Device Tree |

---

## References

- [RISC-V Privileged Specification](https://github.com/riscv/riscv-isa-manual)
- [SBI Specification](https://github.com/riscv-non-isa/riscv-sbi-doc)
- [RISC-V Platform Level Interrupt Controller Spec](https://github.com/riscv/riscv-plic-spec)
- [QEMU RISC-V virt Machine](https://www.qemu.org/docs/master/system/riscv/virt.html)
