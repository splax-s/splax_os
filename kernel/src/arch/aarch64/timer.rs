//! # ARM Generic Timer
//!
//! Driver for the ARM Generic Timer used for system timing.
//!
//! Uses the EL1 physical timer (CNTP) for kernel scheduling.

use core::arch::asm;

/// Timer frequency in Hz (typically 62.5MHz on QEMU).
static mut TIMER_FREQ: u64 = 0;

/// Timer tick interval in timer counts.
static mut TICK_INTERVAL: u64 = 0;

/// Current tick count.
static mut TICK_COUNT: u64 = 0;

/// Timer configuration.
pub struct TimerConfig {
    /// Timer frequency in Hz
    pub frequency_hz: u64,
    /// Tick interval in milliseconds
    pub tick_ms: u64,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            frequency_hz: 0, // Will be read from CNTFRQ_EL0
            tick_ms: 10,     // 10ms ticks (100Hz)
        }
    }
}

/// Read the timer frequency.
#[inline(always)]
pub fn read_freq() -> u64 {
    let freq: u64;
    unsafe {
        asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    freq
}

/// Read the current counter value.
#[inline(always)]
pub fn read_counter() -> u64 {
    let cnt: u64;
    unsafe {
        asm!("mrs {}, cntpct_el0", out(reg) cnt, options(nomem, nostack));
    }
    cnt
}

/// Read the compare value.
#[inline(always)]
pub fn read_compare() -> u64 {
    let cval: u64;
    unsafe {
        asm!("mrs {}, cntp_cval_el0", out(reg) cval, options(nomem, nostack));
    }
    cval
}

/// Write the compare value.
#[inline(always)]
pub fn write_compare(cval: u64) {
    unsafe {
        asm!("msr cntp_cval_el0, {}", in(reg) cval, options(nomem, nostack));
    }
}

/// Read the timer control register.
#[inline(always)]
pub fn read_ctl() -> u64 {
    let ctl: u64;
    unsafe {
        asm!("mrs {}, cntp_ctl_el0", out(reg) ctl, options(nomem, nostack));
    }
    ctl
}

/// Write the timer control register.
#[inline(always)]
pub fn write_ctl(ctl: u64) {
    unsafe {
        asm!("msr cntp_ctl_el0, {}", in(reg) ctl, options(nomem, nostack));
        asm!("isb");
    }
}

/// Timer control register bits.
pub mod ctl {
    /// Timer enable
    pub const ENABLE: u64 = 1 << 0;
    /// Interrupt mask (1 = masked)
    pub const IMASK: u64 = 1 << 1;
    /// Interrupt status (1 = pending)
    pub const ISTATUS: u64 = 1 << 2;
}

/// Initialize the timer.
///
/// # Safety
///
/// Must only be called once during boot.
pub unsafe fn init() {
    unsafe { init_with_config(TimerConfig::default()); }
}

/// Initialize with custom configuration.
///
/// # Safety
///
/// Must only be called once during boot.
pub unsafe fn init_with_config(config: TimerConfig) {
    // Read timer frequency
    let freq = if config.frequency_hz > 0 {
        config.frequency_hz
    } else {
        read_freq()
    };
    
    unsafe {
        TIMER_FREQ = freq;
        
        // Calculate tick interval
        TICK_INTERVAL = (freq * config.tick_ms) / 1000;
    }
    
    // Disable timer while configuring
    write_ctl(0);
    
    // Set first compare value
    let current = read_counter();
    unsafe { write_compare(current + TICK_INTERVAL); }
    
    // Enable timer, unmask interrupt
    write_ctl(ctl::ENABLE);
    
    unsafe { TICK_COUNT = 0; }
}

/// Handle timer interrupt.
pub fn handle_timer_irq() {
    unsafe {
        TICK_COUNT += 1;
        
        // Set next compare value
        let current = read_counter();
        write_compare(current + TICK_INTERVAL);
        
        // Trigger scheduler tick - check if we should preempt
        // Every 10 ticks (100ms at 100Hz), try to schedule another process
        if TICK_COUNT % 10 == 0 {
            // Get next process from scheduler
            if let Some(next_pid) = crate::sched::scheduler().schedule() {
                // Switch to the new process
                crate::sched::scheduler().switch_to(next_pid);
            }
        }
    }
}

/// Get current tick count.
pub fn ticks() -> u64 {
    unsafe { TICK_COUNT }
}

/// Get timer frequency in Hz.
pub fn frequency() -> u64 {
    unsafe { TIMER_FREQ }
}

/// Get uptime in milliseconds.
pub fn uptime_ms() -> u64 {
    let freq = frequency();
    if freq == 0 {
        return 0;
    }
    (read_counter() * 1000) / freq
}

/// Get uptime in microseconds.
pub fn uptime_us() -> u64 {
    let freq = frequency();
    if freq == 0 {
        return 0;
    }
    (read_counter() * 1_000_000) / freq
}

/// Busy-wait for a number of microseconds.
pub fn delay_us(us: u64) {
    let freq = frequency();
    if freq == 0 {
        return;
    }
    
    let start = read_counter();
    let target = start + (us * freq) / 1_000_000;
    
    while read_counter() < target {
        core::hint::spin_loop();
    }
}

/// Busy-wait for a number of milliseconds.
pub fn delay_ms(ms: u64) {
    delay_us(ms * 1000);
}
