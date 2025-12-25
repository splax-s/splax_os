//! # Local APIC Driver
//!
//! The Local Advanced Programmable Interrupt Controller handles:
//! - Timer interrupts
//! - Inter-Processor Interrupts (IPI)
//! - Error handling
//!
//! Each CPU has its own Local APIC.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Local APIC base address (mapped to physical 0xFEE00000).
const LAPIC_BASE: u64 = 0xFEE0_0000;

/// Local APIC register offsets.
mod regs {
    pub const ID: u64 = 0x020;
    pub const VERSION: u64 = 0x030;
    pub const TPR: u64 = 0x080;       // Task Priority Register
    pub const APR: u64 = 0x090;       // Arbitration Priority Register
    pub const PPR: u64 = 0x0A0;       // Processor Priority Register
    pub const EOI: u64 = 0x0B0;       // End of Interrupt
    pub const RRD: u64 = 0x0C0;       // Remote Read Register
    pub const LDR: u64 = 0x0D0;       // Logical Destination Register
    pub const DFR: u64 = 0x0E0;       // Destination Format Register
    pub const SVR: u64 = 0x0F0;       // Spurious Interrupt Vector Register
    pub const ISR_BASE: u64 = 0x100;  // In-Service Register (8 registers)
    pub const TMR_BASE: u64 = 0x180;  // Trigger Mode Register (8 registers)
    pub const IRR_BASE: u64 = 0x200;  // Interrupt Request Register (8 registers)
    pub const ESR: u64 = 0x280;       // Error Status Register
    pub const ICR_LOW: u64 = 0x300;   // Interrupt Command Register (low)
    pub const ICR_HIGH: u64 = 0x310;  // Interrupt Command Register (high)
    pub const TIMER_LVT: u64 = 0x320; // Timer Local Vector Table
    pub const THERMAL_LVT: u64 = 0x330;
    pub const PERF_LVT: u64 = 0x340;
    pub const LINT0_LVT: u64 = 0x350;
    pub const LINT1_LVT: u64 = 0x360;
    pub const ERROR_LVT: u64 = 0x370;
    pub const TIMER_INIT: u64 = 0x380;    // Timer Initial Count
    pub const TIMER_CURRENT: u64 = 0x390;  // Timer Current Count
    pub const TIMER_DIVIDE: u64 = 0x3E0;   // Timer Divide Configuration
}

/// ICR delivery modes.
mod delivery {
    pub const FIXED: u32 = 0b000 << 8;
    pub const LOWEST: u32 = 0b001 << 8;
    pub const SMI: u32 = 0b010 << 8;
    pub const NMI: u32 = 0b100 << 8;
    pub const INIT: u32 = 0b101 << 8;
    pub const STARTUP: u32 = 0b110 << 8;
}

/// ICR destination shorthand.
mod shorthand {
    pub const NONE: u32 = 0b00 << 18;
    pub const SELF: u32 = 0b01 << 18;
    pub const ALL: u32 = 0b10 << 18;
    pub const ALL_EXCL_SELF: u32 = 0b11 << 18;
}

/// ICR flags.
mod icr_flags {
    pub const LEVEL_ASSERT: u32 = 1 << 14;
    pub const LEVEL_DEASSERT: u32 = 0 << 14;
    pub const TRIGGER_EDGE: u32 = 0 << 15;
    pub const TRIGGER_LEVEL: u32 = 1 << 15;
}

/// Timer modes.
mod timer_mode {
    pub const ONE_SHOT: u32 = 0b00 << 17;
    pub const PERIODIC: u32 = 0b01 << 17;
    pub const TSC_DEADLINE: u32 = 0b10 << 17;
}

/// LVT mask bit.
const LVT_MASKED: u32 = 1 << 16;

/// Local APIC driver.
pub struct LocalApic {
    base: u64,
}

impl LocalApic {
    /// Creates a new Local APIC driver.
    ///
    /// # Safety
    ///
    /// The LAPIC must be present and accessible.
    pub const unsafe fn new() -> Self {
        Self { base: LAPIC_BASE }
    }

    /// Reads a register.
    #[inline]
    fn read(&self, offset: u64) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    /// Writes a register.
    #[inline]
    fn write(&self, offset: u64, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Returns the APIC ID.
    pub fn id(&self) -> u32 {
        (self.read(regs::ID) >> 24) & 0xFF
    }

    /// Returns the APIC version.
    pub fn version(&self) -> u32 {
        self.read(regs::VERSION) & 0xFF
    }

    /// Initializes the Local APIC.
    pub fn init(&self) {
        // Enable APIC in SVR (bit 8) with spurious vector 0xFF
        self.write(regs::SVR, 0x100 | 0xFF);

        // Set task priority to 0 (accept all interrupts)
        self.write(regs::TPR, 0);

        // Set logical destination (for logical addressing)
        self.write(regs::LDR, (self.id() as u32) << 24);
        self.write(regs::DFR, 0xFFFFFFFF);  // Flat model

        // Mask all LVT entries initially
        self.write(regs::TIMER_LVT, LVT_MASKED);
        self.write(regs::THERMAL_LVT, LVT_MASKED);
        self.write(regs::PERF_LVT, LVT_MASKED);
        self.write(regs::LINT0_LVT, LVT_MASKED);
        self.write(regs::LINT1_LVT, LVT_MASKED);
        self.write(regs::ERROR_LVT, LVT_MASKED);

        // Clear any pending errors
        self.write(regs::ESR, 0);
        self.write(regs::ESR, 0);
    }

    /// Sends End of Interrupt.
    pub fn eoi(&self) {
        self.write(regs::EOI, 0);
    }

    /// Initializes the APIC timer.
    ///
    /// # Arguments
    ///
    /// * `vector` - Interrupt vector for timer
    /// * `divide` - Timer divide value (0=2, 1=4, 2=8, ..., 7=128, 8+=1)
    /// * `initial_count` - Initial counter value
    /// * `periodic` - Whether to use periodic mode
    pub fn init_timer(&self, vector: u8, divide: u8, initial_count: u32, periodic: bool) {
        // Set divider
        self.write(regs::TIMER_DIVIDE, divide as u32);

        // Set timer LVT entry
        let mode = if periodic { timer_mode::PERIODIC } else { timer_mode::ONE_SHOT };
        self.write(regs::TIMER_LVT, mode | (vector as u32));

        // Set initial count (starts the timer)
        self.write(regs::TIMER_INIT, initial_count);
    }

    /// Stops the timer.
    pub fn stop_timer(&self) {
        self.write(regs::TIMER_INIT, 0);
        self.write(regs::TIMER_LVT, LVT_MASKED);
    }

    /// Returns the current timer count.
    pub fn timer_current(&self) -> u32 {
        self.read(regs::TIMER_CURRENT)
    }

    /// Sends an IPI to a specific CPU.
    ///
    /// # Arguments
    ///
    /// * `target_apic_id` - Target CPU's APIC ID
    /// * `vector` - Interrupt vector
    pub fn send_ipi(&self, target_apic_id: u8, vector: u8) {
        // Wait for previous IPI to complete
        while (self.read(regs::ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        // Write destination (high dword)
        self.write(regs::ICR_HIGH, (target_apic_id as u32) << 24);

        // Write command (low dword) - triggers the IPI
        self.write(
            regs::ICR_LOW,
            delivery::FIXED | shorthand::NONE | icr_flags::LEVEL_ASSERT | (vector as u32),
        );
    }

    /// Sends an IPI to all CPUs except self.
    pub fn send_ipi_all_excluding_self(&self, vector: u8) {
        while (self.read(regs::ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        self.write(regs::ICR_HIGH, 0);
        self.write(
            regs::ICR_LOW,
            delivery::FIXED | shorthand::ALL_EXCL_SELF | icr_flags::LEVEL_ASSERT | (vector as u32),
        );
    }

    /// Sends INIT IPI to a CPU (for AP startup).
    pub fn send_init(&self, target_apic_id: u8) {
        while (self.read(regs::ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        self.write(regs::ICR_HIGH, (target_apic_id as u32) << 24);
        self.write(
            regs::ICR_LOW,
            delivery::INIT | icr_flags::LEVEL_ASSERT | icr_flags::TRIGGER_LEVEL,
        );

        // Wait for delivery
        while (self.read(regs::ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        // De-assert INIT
        self.write(regs::ICR_HIGH, (target_apic_id as u32) << 24);
        self.write(
            regs::ICR_LOW,
            delivery::INIT | icr_flags::LEVEL_DEASSERT | icr_flags::TRIGGER_LEVEL,
        );
    }

    /// Sends STARTUP IPI to a CPU (for AP startup).
    ///
    /// # Arguments
    ///
    /// * `target_apic_id` - Target CPU's APIC ID
    /// * `vector` - Startup vector (page address >> 12)
    pub fn send_sipi(&self, target_apic_id: u8, vector: u8) {
        while (self.read(regs::ICR_LOW) & (1 << 12)) != 0 {
            core::hint::spin_loop();
        }

        self.write(regs::ICR_HIGH, (target_apic_id as u32) << 24);
        self.write(
            regs::ICR_LOW,
            delivery::STARTUP | (vector as u32),
        );
    }

    /// Reads the error status register.
    pub fn error_status(&self) -> u32 {
        // Write before read to latch the error
        self.write(regs::ESR, 0);
        self.read(regs::ESR)
    }
}

/// Global Local APIC instance for the current CPU.
static mut LAPIC: LocalApic = unsafe { LocalApic::new() };

/// Global LAPIC initialized flag.
static LAPIC_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Global timer frequency (calibrated).
static TIMER_FREQ: AtomicU64 = AtomicU64::new(0);

/// Gets the Local APIC for the current CPU.
///
/// # Safety
///
/// Must only be called after LAPIC is initialized.
pub unsafe fn lapic() -> &'static LocalApic {
    unsafe { &LAPIC }
}

/// Initializes the Local APIC.
pub fn init() {
    unsafe {
        LAPIC.init();
    }

    // Calibrate timer (simplified - assumes ~100MHz bus)
    // In a real OS, this would use PIT or HPET for calibration
    TIMER_FREQ.store(100_000_000, Ordering::Relaxed);

    LAPIC_INITIALIZED.store(true, Ordering::Release);
}

/// Starts the APIC timer with a periodic tick.
pub fn start_timer(vector: u8, frequency_hz: u32) {
    let timer_freq = TIMER_FREQ.load(Ordering::Relaxed);
    let initial_count = (timer_freq / frequency_hz as u64) as u32;

    unsafe {
        LAPIC.init_timer(vector, 0x0B, initial_count, true);  // Divide by 1
    }
}

/// Sends EOI to acknowledge interrupt.
pub fn eoi() {
    if LAPIC_INITIALIZED.load(Ordering::Acquire) {
        unsafe { LAPIC.eoi(); }
    }
}

/// IPI vector numbers.
pub mod ipi_vectors {
    pub const RESCHEDULE: u8 = 0xFC;
    pub const TLB_SHOOTDOWN: u8 = 0xFD;
    pub const STOP: u8 = 0xFE;
}

/// Sends a reschedule IPI to a specific CPU.
pub fn send_reschedule_ipi(target_apic_id: u8) {
    if LAPIC_INITIALIZED.load(Ordering::Acquire) {
        unsafe { LAPIC.send_ipi(target_apic_id, ipi_vectors::RESCHEDULE); }
    }
}

/// Sends a TLB shootdown IPI to all other CPUs.
pub fn send_tlb_shootdown() {
    if LAPIC_INITIALIZED.load(Ordering::Acquire) {
        unsafe { LAPIC.send_ipi_all_excluding_self(ipi_vectors::TLB_SHOOTDOWN); }
    }
}
