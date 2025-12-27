//! Real-Time Clock (RTC) Driver
//!
//! Provides access to the CMOS RTC for reading real-world time.
//! Works on both physical hardware and virtual machines.
//!
//! The RTC maintains time even when the system is powered off (battery-backed).

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

/// CMOS/RTC port addresses
const CMOS_ADDRESS: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

/// RTC register addresses
mod registers {
    pub const SECONDS: u8 = 0x00;
    pub const MINUTES: u8 = 0x02;
    pub const HOURS: u8 = 0x04;
    pub const DAY_OF_WEEK: u8 = 0x06;
    pub const DAY_OF_MONTH: u8 = 0x07;
    pub const MONTH: u8 = 0x08;
    pub const YEAR: u8 = 0x09;
    pub const CENTURY: u8 = 0x32; // May not exist on all systems
    pub const STATUS_A: u8 = 0x0A;
    pub const STATUS_B: u8 = 0x0B;
}

/// Boot timestamp (Unix timestamp when kernel started)
static BOOT_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

/// Represents a date and time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub day_of_week: u8, // 1 = Sunday, 7 = Saturday
}

impl DateTime {
    /// Format as ISO 8601: YYYY-MM-DD HH:MM:SS
    pub fn format_iso(&self) -> [u8; 19] {
        let mut buf = [b' '; 19];
        
        // Year
        buf[0] = b'0' + ((self.year / 1000) % 10) as u8;
        buf[1] = b'0' + ((self.year / 100) % 10) as u8;
        buf[2] = b'0' + ((self.year / 10) % 10) as u8;
        buf[3] = b'0' + (self.year % 10) as u8;
        buf[4] = b'-';
        
        // Month
        buf[5] = b'0' + (self.month / 10);
        buf[6] = b'0' + (self.month % 10);
        buf[7] = b'-';
        
        // Day
        buf[8] = b'0' + (self.day / 10);
        buf[9] = b'0' + (self.day % 10);
        buf[10] = b' ';
        
        // Hour
        buf[11] = b'0' + (self.hour / 10);
        buf[12] = b'0' + (self.hour % 10);
        buf[13] = b':';
        
        // Minute
        buf[14] = b'0' + (self.minute / 10);
        buf[15] = b'0' + (self.minute % 10);
        buf[16] = b':';
        
        // Second
        buf[17] = b'0' + (self.second / 10);
        buf[18] = b'0' + (self.second % 10);
        
        buf
    }
    
    /// Get day name
    pub fn day_name(&self) -> &'static str {
        match self.day_of_week {
            1 => "Sunday",
            2 => "Monday",
            3 => "Tuesday",
            4 => "Wednesday",
            5 => "Thursday",
            6 => "Friday",
            7 => "Saturday",
            _ => "Unknown",
        }
    }
    
    /// Get month name
    pub fn month_name(&self) -> &'static str {
        match self.month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "Unknown",
        }
    }
    
    /// Convert to Unix timestamp (seconds since 1970-01-01 00:00:00 UTC)
    pub fn to_unix_timestamp(&self) -> u64 {
        // Days from year 1970 to this year
        let mut days: u64 = 0;
        for y in 1970..self.year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }
        
        // Days from months in current year
        let month_days = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for m in 1..self.month {
            days += month_days[m as usize] as u64;
            if m == 2 && is_leap_year(self.year) {
                days += 1;
            }
        }
        
        // Add days in current month
        days += (self.day - 1) as u64;
        
        // Convert to seconds and add time
        days * 86400 + (self.hour as u64) * 3600 + (self.minute as u64) * 60 + (self.second as u64)
    }
}

/// Check if a year is a leap year
fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Read a byte from CMOS
fn read_cmos(register: u8) -> u8 {
    unsafe {
        // Select register (with NMI disable bit clear)
        asm!("out dx, al", in("dx") CMOS_ADDRESS, in("al") register);
        // Small delay for CMOS
        asm!("nop");
        asm!("nop");
        // Read data
        let value: u8;
        asm!("in al, dx", out("al") value, in("dx") CMOS_DATA);
        value
    }
}

/// Write a byte to CMOS
#[allow(dead_code)]
fn write_cmos(register: u8, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") CMOS_ADDRESS, in("al") register);
        asm!("nop");
        asm!("nop");
        asm!("out dx, al", in("dx") CMOS_DATA, in("al") value);
    }
}

/// Check if RTC update is in progress
fn is_update_in_progress() -> bool {
    (read_cmos(registers::STATUS_A) & 0x80) != 0
}

/// Convert BCD to binary
fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd >> 4) * 10)
}

/// Read the current date and time from the RTC
/// 
/// This function handles both BCD and binary modes,
/// and waits for the RTC update to complete for accuracy.
pub fn read_rtc() -> DateTime {
    // Wait for any update in progress to complete
    while is_update_in_progress() {
        core::hint::spin_loop();
    }
    
    // Read all registers
    let mut second = read_cmos(registers::SECONDS);
    let mut minute = read_cmos(registers::MINUTES);
    let mut hour = read_cmos(registers::HOURS);
    let day_of_week = read_cmos(registers::DAY_OF_WEEK);
    let mut day = read_cmos(registers::DAY_OF_MONTH);
    let mut month = read_cmos(registers::MONTH);
    let mut year = read_cmos(registers::YEAR);
    
    // Try to read century register (may not exist)
    let century = read_cmos(registers::CENTURY);
    
    // Read status register B to check format
    let status_b = read_cmos(registers::STATUS_B);
    let is_bcd = (status_b & 0x04) == 0;
    let is_12_hour = (status_b & 0x02) == 0;
    
    // Convert from BCD if necessary
    if is_bcd {
        second = bcd_to_binary(second);
        minute = bcd_to_binary(minute);
        hour = bcd_to_binary(hour & 0x7F) | (hour & 0x80); // Preserve PM bit
        day = bcd_to_binary(day);
        month = bcd_to_binary(month);
        year = bcd_to_binary(year);
    }
    
    // Handle 12-hour format
    if is_12_hour && (hour & 0x80) != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }
    
    // Calculate full year
    let full_year = if century > 0 && century < 100 {
        (bcd_to_binary(century) as u16) * 100 + (year as u16)
    } else if year < 70 {
        2000 + (year as u16)
    } else {
        1900 + (year as u16)
    };
    
    DateTime {
        year: full_year,
        month,
        day,
        hour,
        minute,
        second,
        day_of_week,
    }
}

/// Initialize the RTC and record boot time
pub fn init() {
    let now = read_rtc();
    let timestamp = now.to_unix_timestamp();
    BOOT_TIMESTAMP.store(timestamp, Ordering::SeqCst);
}

/// Get the boot timestamp
pub fn boot_timestamp() -> u64 {
    BOOT_TIMESTAMP.load(Ordering::Relaxed)
}

/// Get current Unix timestamp
pub fn unix_timestamp() -> u64 {
    read_rtc().to_unix_timestamp()
}

/// Get system uptime in seconds
pub fn uptime_seconds() -> u64 {
    let boot = BOOT_TIMESTAMP.load(Ordering::Relaxed);
    let now = unix_timestamp();
    now.saturating_sub(boot)
}

/// Format uptime as human-readable string
pub fn format_uptime() -> alloc::string::String {
    use alloc::format;
    
    let total_seconds = uptime_seconds();
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if days > 0 {
        format!("{} days, {:02}:{:02}:{:02}", days, hours, minutes, seconds)
    } else if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

extern crate alloc;
