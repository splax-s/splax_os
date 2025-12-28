//! # RISC-V Timer
//!
//! Uses the SBI timer extension or memory-mapped CLINT.
//!
//! ## CLINT (Core Local Interruptor)
//!
//! - mtimecmp: Compare register for timer interrupt
//! - mtime: Current time counter
//!
//! For S-mode, we use SBI calls to set the timer.

use super::csr;
use super::sbi;

/// Timer frequency (typical QEMU value: 10 MHz)
const TIMER_FREQ: u64 = 10_000_000;

/// Ticks per millisecond
const TICKS_PER_MS: u64 = TIMER_FREQ / 1000;

/// Timer interval for scheduling (10ms)
const TIMER_INTERVAL_MS: u64 = 10;
const TIMER_INTERVAL_TICKS: u64 = TIMER_INTERVAL_MS * TICKS_PER_MS;

/// Initialize timer subsystem
pub fn init() {
    // Timer will be set up per-hart
}

/// Initialize timer for current hart
pub fn hart_init() {
    // Set first timer interrupt
    set_next_timer();
    
    // Enable timer interrupt in sie
    unsafe {
        let sie = csr::read_sie();
        csr::write_sie(sie | csr::sie::STIE);
    }
}

/// Read current time
#[inline(always)]
pub fn get_time() -> u64 {
    csr::read_time()
}

/// Get time in milliseconds
pub fn get_time_ms() -> u64 {
    get_time() / TICKS_PER_MS
}

/// Set timer for next interrupt
pub fn set_next_timer() {
    let next_time = get_time() + TIMER_INTERVAL_TICKS;
    sbi::set_timer(next_time);
}

/// Set timer for specific time
pub fn set_timer(time: u64) {
    sbi::set_timer(time);
}

/// Handle timer interrupt
pub fn handle_interrupt() {
    // Clear pending interrupt by setting next timer
    set_next_timer();
    
    // Call scheduler tick
    // TODO: crate::sched::timer_tick();
}

/// Sleep for approximately `ms` milliseconds
pub fn sleep_ms(ms: u64) {
    let target = get_time() + ms * TICKS_PER_MS;
    while get_time() < target {
        super::wfi();
    }
}

/// Busy-wait delay for `us` microseconds
pub fn delay_us(us: u64) {
    let ticks = us * TIMER_FREQ / 1_000_000;
    let target = get_time() + ticks;
    while get_time() < target {
        core::hint::spin_loop();
    }
}
