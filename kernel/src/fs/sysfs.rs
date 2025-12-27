//! # SysFS - System Filesystem
//!
//! Virtual filesystem providing kernel object and device information.
//! Similar to Linux's /sys filesystem.
//!
//! ## Structure
//!
//! ```text
//! /sys
//! ├── kernel/
//! │   ├── version
//! │   ├── hostname
//! │   └── osrelease
//! ├── devices/
//! │   ├── block/
//! │   │   └── vda/
//! │   │       ├── size
//! │   │       ├── model
//! │   │       └── stat
//! │   └── net/
//! │       └── eth0/
//! │           ├── address
//! │           ├── mtu
//! │           └── statistics/
//! ├── block/
//! │   └── vda -> ../devices/block/vda
//! ├── class/
//! │   ├── block/
//! │   └── net/
//! └── fs/
//!     ├── ramfs/
//!     └── splaxfs/
//! ```

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

/// SysFS entry types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysEntryType {
    /// Directory
    Directory,
    /// Regular file (attribute)
    File,
    /// Symbolic link
    Link,
}

/// SysFS entry
#[derive(Debug, Clone)]
pub struct SysEntry {
    /// Entry name
    pub name: String,
    /// Entry type
    pub entry_type: SysEntryType,
    /// For links, the target path
    pub link_target: Option<String>,
}

/// Lists entries in /sys directory
pub fn list_sys() -> Vec<SysEntry> {
    vec![
        SysEntry {
            name: String::from("kernel"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("devices"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("block"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("class"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("fs"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("module"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
    ]
}

/// Lists entries in /sys/kernel
pub fn list_sys_kernel() -> Vec<SysEntry> {
    vec![
        SysEntry {
            name: String::from("version"),
            entry_type: SysEntryType::File,
            link_target: None,
        },
        SysEntry {
            name: String::from("hostname"),
            entry_type: SysEntryType::File,
            link_target: None,
        },
        SysEntry {
            name: String::from("osrelease"),
            entry_type: SysEntryType::File,
            link_target: None,
        },
        SysEntry {
            name: String::from("ostype"),
            entry_type: SysEntryType::File,
            link_target: None,
        },
    ]
}

/// Lists entries in /sys/devices
pub fn list_sys_devices() -> Vec<SysEntry> {
    vec![
        SysEntry {
            name: String::from("block"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("net"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
        SysEntry {
            name: String::from("system"),
            entry_type: SysEntryType::Directory,
            link_target: None,
        },
    ]
}

/// Lists entries in /sys/devices/block
pub fn list_sys_devices_block() -> Vec<SysEntry> {
    let mut entries = Vec::new();
    
    for dev in crate::block::list_devices() {
        entries.push(SysEntry {
            name: dev.name.clone(),
            entry_type: SysEntryType::Directory,
            link_target: None,
        });
    }
    
    entries
}

/// Lists entries in /sys/devices/net
pub fn list_sys_devices_net() -> Vec<SysEntry> {
    let mut entries = Vec::new();
    
    let stack = crate::net::network_stack().lock();
    if let Some(iface) = stack.primary_interface() {
        entries.push(SysEntry {
            name: iface.config.name.to_string(),
            entry_type: SysEntryType::Directory,
            link_target: None,
        });
    }
    drop(stack);
    
    entries
}

/// Lists block device attributes
pub fn list_sys_block_device(dev_name: &str) -> Vec<SysEntry> {
    if crate::block::with_device(dev_name, |_| ()).is_ok() {
        vec![
            SysEntry {
                name: String::from("size"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("model"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("stat"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("ro"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
        ]
    } else {
        Vec::new()
    }
}

/// Lists network device attributes
pub fn list_sys_net_device(dev_name: &str) -> Vec<SysEntry> {
    let stack = crate::net::network_stack().lock();
    let found = stack.primary_interface()
        .map(|i| i.config.name == dev_name)
        .unwrap_or(false);
    drop(stack);
    
    if found {
        vec![
            SysEntry {
                name: String::from("address"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("mtu"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("operstate"),
                entry_type: SysEntryType::File,
                link_target: None,
            },
            SysEntry {
                name: String::from("statistics"),
                entry_type: SysEntryType::Directory,
                link_target: None,
            },
        ]
    } else {
        Vec::new()
    }
}

/// Reads a sysfs file
pub fn read_sys_file(path: &str) -> Option<String> {
    let path = path.trim_start_matches("/sys").trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    
    match parts.as_slice() {
        ["kernel", "version"] => Some(String::from("0.1.0\n")),
        ["kernel", "hostname"] => Some(String::from("splax\n")),
        ["kernel", "osrelease"] => Some(String::from("0.1.0-splax\n")),
        ["kernel", "ostype"] => Some(String::from("SplaxOS\n")),
        
        ["devices", "block", dev_name, "size"] => {
            crate::block::with_device(dev_name, |dev| {
                format!("{}\n", dev.info().total_sectors * 512)
            }).ok()
        }
        ["devices", "block", dev_name, "model"] => {
            crate::block::with_device(dev_name, |dev| {
                format!("{}\n", dev.info().model)
            }).ok()
        }
        ["devices", "block", dev_name, "ro"] => {
            crate::block::with_device(dev_name, |dev| {
                format!("{}\n", if dev.info().read_only { "1" } else { "0" })
            }).ok()
        }
        ["devices", "block", dev_name, "stat"] => {
            crate::block::with_device(dev_name, |dev| {
                let info = dev.info();
                format!("       0        0        0        0        0        0        0        0        0        0        0        0        0        0        0        0        0\n")
            }).ok()
        }
        
        ["devices", "net", dev_name, "address"] => {
            let stack = crate::net::network_stack().lock();
            let result = stack.primary_interface()
                .filter(|i| i.config.name == *dev_name)
                .map(|i| {
                    let mac = i.config.mac;
                    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                        mac.0[0], mac.0[1], mac.0[2],
                        mac.0[3], mac.0[4], mac.0[5])
                });
            drop(stack);
            result
        }
        ["devices", "net", dev_name, "mtu"] => {
            let stack = crate::net::network_stack().lock();
            let result = stack.primary_interface()
                .filter(|i| i.config.name == *dev_name)
                .map(|i| format!("{}\n", i.config.mtu));
            drop(stack);
            result
        }
        ["devices", "net", dev_name, "operstate"] => {
            let stack = crate::net::network_stack().lock();
            let result = stack.primary_interface()
                .filter(|i| i.config.name == *dev_name)
                .map(|_| String::from("up\n"));
            drop(stack);
            result
        }
        ["devices", "net", dev_name, "statistics", stat] => {
            // TODO: Add stats to NetworkInterface when available
            let stack = crate::net::network_stack().lock();
            let result = stack.primary_interface()
                .filter(|i| i.config.name == *dev_name)
                .map(|_| {
                    // Return placeholder stats for now
                    match *stat {
                        "rx_bytes" | "tx_bytes" | "rx_packets" | "tx_packets" |
                        "rx_errors" | "tx_errors" => String::from("0\n"),
                        _ => String::from("0\n"),
                    }
                });
            drop(stack);
            result
        }
        
        _ => None,
    }
}
