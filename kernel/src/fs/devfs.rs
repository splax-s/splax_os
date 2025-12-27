//! # DevFS - Device Filesystem
//!
//! Virtual filesystem providing device nodes.
//! Similar to Linux's /dev filesystem.
//!
//! ## Structure
//!
//! ```text
//! /dev
//! ├── null        - Null device (discards writes, EOF on read)
//! ├── zero        - Zero device (infinite zeros on read)
//! ├── random      - Random number generator
//! ├── urandom     - Non-blocking random
//! ├── console     - System console
//! ├── tty         - Current TTY
//! ├── tty0        - Virtual console 0
//! ├── vda         - VirtIO disk a
//! ├── vdb         - VirtIO disk b
//! └── eth0        - Network interface
//! ```

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::format;

/// Device types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// Character device
    Char,
    /// Block device
    Block,
    /// Network device
    Net,
}

/// Device node entry
#[derive(Debug, Clone)]
pub struct DevEntry {
    /// Device name
    pub name: String,
    /// Device type
    pub device_type: DeviceType,
    /// Major number
    pub major: u16,
    /// Minor number
    pub minor: u16,
}

/// Well-known device major numbers
pub mod major {
    pub const MEM: u16 = 1;      // Memory devices (null, zero, random)
    pub const TTY: u16 = 4;      // TTY devices
    pub const CONSOLE: u16 = 5; // Console
    pub const VIRTIO_BLK: u16 = 253; // VirtIO block devices
}

/// Lists all device entries in /dev
pub fn list_dev() -> Vec<DevEntry> {
    let mut entries = Vec::new();
    
    // Character devices
    entries.push(DevEntry {
        name: String::from("null"),
        device_type: DeviceType::Char,
        major: major::MEM,
        minor: 3,
    });
    entries.push(DevEntry {
        name: String::from("zero"),
        device_type: DeviceType::Char,
        major: major::MEM,
        minor: 5,
    });
    entries.push(DevEntry {
        name: String::from("random"),
        device_type: DeviceType::Char,
        major: major::MEM,
        minor: 8,
    });
    entries.push(DevEntry {
        name: String::from("urandom"),
        device_type: DeviceType::Char,
        major: major::MEM,
        minor: 9,
    });
    entries.push(DevEntry {
        name: String::from("console"),
        device_type: DeviceType::Char,
        major: major::CONSOLE,
        minor: 1,
    });
    entries.push(DevEntry {
        name: String::from("tty"),
        device_type: DeviceType::Char,
        major: major::TTY,
        minor: 0,
    });
    entries.push(DevEntry {
        name: String::from("tty0"),
        device_type: DeviceType::Char,
        major: major::TTY,
        minor: 1,
    });
    
    // Block devices from block subsystem
    for (i, dev) in crate::block::list_devices().iter().enumerate() {
        entries.push(DevEntry {
            name: dev.name.clone(),
            device_type: DeviceType::Block,
            major: major::VIRTIO_BLK,
            minor: i as u16,
        });
    }
    
    // Network devices
    let stack = crate::net::network_stack().lock();
    if let Some(iface) = stack.primary_interface() {
        entries.push(DevEntry {
            name: iface.config.name.to_string(),
            device_type: DeviceType::Net,
            major: 0,
            minor: 0,
        });
    }
    drop(stack);
    
    entries
}

/// Read from a device
pub fn read_device(name: &str, buffer: &mut [u8]) -> Result<usize, DevError> {
    match name {
        "null" => {
            // /dev/null returns EOF immediately
            Ok(0)
        }
        "zero" => {
            // /dev/zero returns all zeros
            for byte in buffer.iter_mut() {
                *byte = 0;
            }
            Ok(buffer.len())
        }
        "random" | "urandom" => {
            // Simple PRNG for random data
            let seed = crate::arch::x86_64::interrupts::get_ticks();
            let mut state = seed;
            for byte in buffer.iter_mut() {
                // LCG random number generator
                state = state.wrapping_mul(1103515245).wrapping_add(12345);
                *byte = (state >> 16) as u8;
            }
            Ok(buffer.len())
        }
        _ => Err(DevError::NotFound),
    }
}

/// Write to a device
pub fn write_device(name: &str, data: &[u8]) -> Result<usize, DevError> {
    match name {
        "null" => {
            // /dev/null discards all data
            Ok(data.len())
        }
        "zero" => {
            // Writing to /dev/zero is ignored
            Ok(data.len())
        }
        "console" | "tty" | "tty0" => {
            // Write to console (VGA)
            for &byte in data {
                if byte >= 0x20 && byte < 0x7f {
                    crate::vga_print!("{}", byte as char);
                } else if byte == b'\n' {
                    crate::vga_println!();
                }
            }
            Ok(data.len())
        }
        _ => Err(DevError::NotFound),
    }
}

/// Device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevError {
    /// Device not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Device busy
    Busy,
    /// Invalid operation
    InvalidOperation,
}

/// Get device info for a specific device
pub fn get_device_info(name: &str) -> Option<DevEntry> {
    list_dev().into_iter().find(|e| e.name == name)
}

/// Read a pseudo-file for device info
pub fn read_dev_file(path: &str) -> Option<String> {
    let path = path.trim_start_matches("/dev").trim_start_matches('/');
    
    if let Some(entry) = get_device_info(path) {
        Some(format!(
            "Device: {}\n\
             Type:   {:?}\n\
             Major:  {}\n\
             Minor:  {}\n",
            entry.name,
            entry.device_type,
            entry.major,
            entry.minor
        ))
    } else {
        None
    }
}
