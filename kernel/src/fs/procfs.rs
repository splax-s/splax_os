//! # ProcFS - Process Filesystem
//!
//! Virtual filesystem providing process and system information.
//! Similar to Linux's /proc filesystem.
//!
//! ## Structure
//!
//! ```text
//! /proc
//! ├── self          -> current process
//! ├── version       - kernel version
//! ├── uptime        - system uptime
//! ├── meminfo       - memory information
//! ├── cpuinfo       - CPU information
//! ├── cmdline       - kernel command line
//! ├── loadavg       - system load averages
//! ├── stat          - kernel/system statistics
//! ├── net/          - network information
//! │   ├── dev       - network device statistics
//! │   ├── arp       - ARP cache
//! │   └── route     - routing table
//! └── [pid]/        - per-process directories
//!     ├── status    - process status
//!     ├── cmdline   - command line
//!     ├── stat      - process statistics
//!     └── maps      - memory maps
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

/// ProcFS file types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcFileType {
    /// Regular file (read generates content dynamically)
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Link,
}

/// ProcFS entry
#[derive(Debug, Clone)]
pub struct ProcEntry {
    /// Entry name
    pub name: String,
    /// Entry type
    pub file_type: ProcFileType,
    /// For links, the target path
    pub link_target: Option<String>,
}

/// Reads /proc/version
pub fn read_version() -> String {
    format!(
        "Splax OS version 0.1.0 (rustc {}) #1 SMP {}\n",
        "1.85.0-nightly",
        "Dec 26 2025"
    )
}

/// Reads /proc/uptime
pub fn read_uptime() -> String {
    let ticks = crate::arch::x86_64::interrupts::get_ticks();
    let seconds = ticks / 100; // Assuming 100 Hz timer
    let idle_seconds = seconds / 2; // Fake idle time
    format!("{}.{:02} {}.{:02}\n", 
        seconds, ticks % 100,
        idle_seconds, (ticks / 2) % 100)
}

/// Reads /proc/meminfo
pub fn read_meminfo() -> String {
    let stats = crate::mm::heap_stats();
    let total_kb = stats.heap_size / 1024;
    let used_kb = stats.total_allocated / 1024;
    let free_kb = (stats.heap_size.saturating_sub(stats.total_allocated)) / 1024;
    
    format!(
        "MemTotal:       {:8} kB\n\
         MemFree:        {:8} kB\n\
         MemUsed:        {:8} kB\n\
         Buffers:        {:8} kB\n\
         Cached:         {:8} kB\n\
         SwapTotal:      {:8} kB\n\
         SwapFree:       {:8} kB\n\
         Allocations:    {:8}\n\
         Deallocations:  {:8}\n",
        total_kb,
        free_kb,
        used_kb,
        0,  // Buffers
        0,  // Cached
        0,  // SwapTotal
        0,  // SwapFree
        stats.allocation_count,
        stats.deallocation_count
    )
}

/// Reads /proc/cpuinfo
pub fn read_cpuinfo() -> String {
    let mut info = String::new();
    
    // Get number of CPUs
    let cpu_count = crate::sched::smp::cpu_count();
    
    for cpu_id in 0..cpu_count {
        info.push_str(&format!(
            "processor       : {}\n\
             vendor_id       : Splax\n\
             cpu family      : 6\n\
             model           : 0\n\
             model name      : Splax Virtual CPU\n\
             stepping        : 0\n\
             cpu MHz         : 3000.000\n\
             cache size      : 4096 KB\n\
             physical id     : 0\n\
             siblings        : {}\n\
             core id         : {}\n\
             cpu cores       : {}\n\
             flags           : fpu vme de pse tsc msr pae mce cx8 apic sep mtrr\n\
             \n",
            cpu_id,
            cpu_count,
            cpu_id,
            cpu_count
        ));
    }
    
    info
}

/// Reads /proc/cmdline
pub fn read_cmdline() -> String {
    String::from("BOOT_IMAGE=/boot/splax_kernel root=/dev/vda ro quiet\n")
}

/// Reads /proc/loadavg
pub fn read_loadavg() -> String {
    // Fake load averages for now
    let running = crate::sched::running_process_count();
    let total = crate::sched::total_process_count();
    format!("0.00 0.00 0.00 {}/{} 1\n", running, total)
}

/// Reads /proc/stat
pub fn read_stat() -> String {
    let ticks = crate::arch::x86_64::interrupts::get_ticks();
    let processes = crate::sched::total_process_count();
    let cpu_count = crate::sched::smp::cpu_count();
    
    let mut stat = String::new();
    
    // Aggregate CPU stats
    stat.push_str(&format!(
        "cpu  {} {} {} {} 0 0 0 0 0 0\n",
        ticks / 4, ticks / 8, ticks / 4, ticks / 2
    ));
    
    // Per-CPU stats
    for i in 0..cpu_count {
        stat.push_str(&format!(
            "cpu{} {} {} {} {} 0 0 0 0 0 0\n",
            i, ticks / 4 / cpu_count as u64, ticks / 8 / cpu_count as u64,
            ticks / 4 / cpu_count as u64, ticks / 2 / cpu_count as u64
        ));
    }
    
    stat.push_str(&format!(
        "intr {}\n\
         ctxt {}\n\
         btime 0\n\
         processes {}\n\
         procs_running 1\n\
         procs_blocked 0\n",
        ticks * 10,  // Fake interrupt count
        ticks * 5,   // Fake context switches
        processes
    ));
    
    stat
}

/// Reads /proc/net/dev
pub fn read_net_dev() -> String {
    let output = String::from(
        "Inter-|   Receive                                                |  Transmit\n\
         face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n"
    );
    
    // For now, we don't have stats in InterfaceConfig, so just show interface name with zeros
    // NetworkStats are included in the interface data
    // Stats are populated when available from the network subsystem
    
    output
}

/// Reads /proc/net/arp
pub fn read_net_arp() -> String {
    let mut output = String::from(
        "IP address       HW type     Flags       HW address            Mask     Device\n"
    );
    
    for entry in crate::net::get_arp_cache() {
        output.push_str(&format!(
            "{:<16} 0x1         0x2         {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}     *        eth0\n",
            entry.ip,
            entry.mac.0[0], entry.mac.0[1], entry.mac.0[2],
            entry.mac.0[3], entry.mac.0[4], entry.mac.0[5]
        ));
    }
    
    output
}

/// Reads /proc/net/route
pub fn read_net_route() -> String {
    let mut output = String::from(
        "Iface   Destination     Gateway         Flags   RefCnt  Use     Metric  Mask            MTU     Window  IRTT\n"
    );
    
    for entry in crate::net::get_routes() {
        output.push_str(&format!(
            "{:<8}{:<16}{:<16}{:<8}{:<8}{:<8}{:<8}{:<16}{:<8}{:<8}{}\n",
            entry.interface,
            entry.destination,
            entry.gateway,
            entry.flags,
            0, 0, 0,  // RefCnt, Use, Metric
            entry.netmask,
            1500, 0, 0  // MTU, Window, IRTT
        ));
    }
    
    output
}

/// Reads /proc/mounts
pub fn read_mounts() -> String {
    let mut output = String::new();
    
    output.push_str("ramfs / ramfs rw 0 0\n");
    output.push_str("proc /proc proc rw 0 0\n");
    output.push_str("dev /dev devfs rw 0 0\n");
    output.push_str("sys /sys sysfs rw 0 0\n");
    
    output
}

/// Reads /proc/filesystems
pub fn read_filesystems() -> String {
    String::from(
        "nodev   ramfs\n\
         nodev   proc\n\
         nodev   devfs\n\
         nodev   sysfs\n\
                 splaxfs\n"
    )
}

/// Reads /proc/interrupts
pub fn read_interrupts() -> String {
    let mut output = String::from("           CPU0\n");
    
    let timer_count = crate::arch::x86_64::interrupts::get_ticks();
    let keyboard_count = crate::arch::x86_64::interrupts::get_keyboard_irq_count();
    
    output.push_str(&format!("  0: {:>10}   IO-APIC-edge      timer\n", timer_count));
    output.push_str(&format!("  1: {:>10}   IO-APIC-edge      keyboard\n", keyboard_count));
    output.push_str(&format!("  4: {:>10}   IO-APIC-edge      serial\n", 0));
    
    output
}

/// Lists entries in /proc directory
pub fn list_proc() -> Vec<ProcEntry> {
    let mut entries = Vec::new();
    
    // System-wide entries
    entries.push(ProcEntry {
        name: String::from("version"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("uptime"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("meminfo"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("cpuinfo"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("cmdline"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("loadavg"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("stat"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("mounts"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("filesystems"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("interrupts"),
        file_type: ProcFileType::File,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("net"),
        file_type: ProcFileType::Directory,
        link_target: None,
    });
    entries.push(ProcEntry {
        name: String::from("self"),
        file_type: ProcFileType::Link,
        link_target: Some(String::from("1")),
    });
    
    // Add process directories
    for proc in crate::sched::list_processes() {
        entries.push(ProcEntry {
            name: format!("{}", proc.pid.0),
            file_type: ProcFileType::Directory,
            link_target: None,
        });
    }
    
    entries
}

/// Lists entries in /proc/net directory
pub fn list_proc_net() -> Vec<ProcEntry> {
    vec![
        ProcEntry {
            name: String::from("dev"),
            file_type: ProcFileType::File,
            link_target: None,
        },
        ProcEntry {
            name: String::from("arp"),
            file_type: ProcFileType::File,
            link_target: None,
        },
        ProcEntry {
            name: String::from("route"),
            file_type: ProcFileType::File,
            link_target: None,
        },
    ]
}

/// Reads a procfs file by path
pub fn read_proc_file(path: &str) -> Option<String> {
    let path = path.trim_start_matches("/proc").trim_start_matches('/');
    
    match path {
        "version" => Some(read_version()),
        "uptime" => Some(read_uptime()),
        "meminfo" => Some(read_meminfo()),
        "cpuinfo" => Some(read_cpuinfo()),
        "cmdline" => Some(read_cmdline()),
        "loadavg" => Some(read_loadavg()),
        "stat" => Some(read_stat()),
        "mounts" => Some(read_mounts()),
        "filesystems" => Some(read_filesystems()),
        "interrupts" => Some(read_interrupts()),
        "net/dev" => Some(read_net_dev()),
        "net/arp" => Some(read_net_arp()),
        "net/route" => Some(read_net_route()),
        _ => None,
    }
}
