//! # Intel E1000 Network Driver
//!
//! Driver for Intel 82540EM Gigabit Ethernet Controller (E1000).
//! This is one of the most commonly emulated NICs in virtual machines.
//!
//! Based on Intel 82540EM (E1000) PCI device specification.
//! Reference: Intel 82540EM Gigabit Ethernet Controller Software Developer's Manual

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::device::{NetworkDevice, NetworkDeviceInfo, NetworkError};
use super::ethernet::MacAddress;

/// Intel Vendor ID
pub const INTEL_VENDOR_ID: u16 = 0x8086;

/// E1000 Device IDs
pub mod device_ids {
    pub const E1000_82540EM: u16 = 0x100E;    // QEMU default
    pub const E1000_82543GC: u16 = 0x1004;
    pub const E1000_82545EM: u16 = 0x100F;
    pub const E1000_82541EI: u16 = 0x1013;
    pub const E1000_82574L: u16 = 0x10D3;     // e1000e
    pub const E1000E_82579LM: u16 = 0x1502;   // e1000e laptop
}

/// E1000 Register offsets (memory-mapped)
pub mod regs {
    pub const CTRL: u32 = 0x0000;      // Device Control
    pub const STATUS: u32 = 0x0008;    // Device Status
    pub const EECD: u32 = 0x0010;      // EEPROM/Flash Control
    pub const EERD: u32 = 0x0014;      // EEPROM Read
    pub const CTRL_EXT: u32 = 0x0018;  // Extended Device Control
    pub const ICR: u32 = 0x00C0;       // Interrupt Cause Read
    pub const ITR: u32 = 0x00C4;       // Interrupt Throttling
    pub const ICS: u32 = 0x00C8;       // Interrupt Cause Set
    pub const IMS: u32 = 0x00D0;       // Interrupt Mask Set
    pub const IMC: u32 = 0x00D8;       // Interrupt Mask Clear
    pub const RCTL: u32 = 0x0100;      // RX Control
    pub const RDBAL: u32 = 0x2800;     // RX Descriptor Base Low
    pub const RDBAH: u32 = 0x2804;     // RX Descriptor Base High
    pub const RDLEN: u32 = 0x2808;     // RX Descriptor Length
    pub const RDH: u32 = 0x2810;       // RX Descriptor Head
    pub const RDT: u32 = 0x2818;       // RX Descriptor Tail
    pub const TCTL: u32 = 0x0400;      // TX Control
    pub const TIPG: u32 = 0x0410;      // TX Inter Packet Gap
    pub const TDBAL: u32 = 0x3800;     // TX Descriptor Base Low
    pub const TDBAH: u32 = 0x3804;     // TX Descriptor Base High
    pub const TDLEN: u32 = 0x3808;     // TX Descriptor Length
    pub const TDH: u32 = 0x3810;       // TX Descriptor Head
    pub const TDT: u32 = 0x3818;       // TX Descriptor Tail
    pub const MTA: u32 = 0x5200;       // Multicast Table Array (128 entries)
    pub const RAL: u32 = 0x5400;       // Receive Address Low
    pub const RAH: u32 = 0x5404;       // Receive Address High
}

/// Control register bits
pub mod ctrl {
    pub const SLU: u32 = 1 << 6;       // Set Link Up
    pub const ASDE: u32 = 1 << 5;      // Auto-Speed Detection Enable
    pub const RST: u32 = 1 << 26;      // Device Reset
    pub const VME: u32 = 1 << 30;      // VLAN Mode Enable
    pub const PHY_RST: u32 = 1 << 31;  // PHY Reset
}

/// Status register bits
pub mod status {
    pub const LU: u32 = 1 << 1;        // Link Up
    pub const SPEED_MASK: u32 = 0b11 << 6;
    pub const SPEED_10: u32 = 0b00 << 6;
    pub const SPEED_100: u32 = 0b01 << 6;
    pub const SPEED_1000: u32 = 0b10 << 6;
}

/// RCTL (Receive Control) register bits
pub mod rctl {
    pub const EN: u32 = 1 << 1;           // Receiver Enable
    pub const SBP: u32 = 1 << 2;          // Store Bad Packets
    pub const UPE: u32 = 1 << 3;          // Unicast Promiscuous Enable
    pub const MPE: u32 = 1 << 4;          // Multicast Promiscuous Enable
    pub const LPE: u32 = 1 << 5;          // Long Packet Reception Enable
    pub const LBM_NONE: u32 = 0b00 << 6;  // No Loopback
    pub const RDMTS_HALF: u32 = 0b00 << 8; // RX Desc Min Threshold Size
    pub const MO_36: u32 = 0b00 << 12;    // Multicast Offset bits [47:36]
    pub const BAM: u32 = 1 << 15;         // Broadcast Accept Mode
    pub const BSIZE_2048: u32 = 0b00 << 16;
    pub const BSIZE_1024: u32 = 0b01 << 16;
    pub const BSIZE_512: u32 = 0b10 << 16;
    pub const BSIZE_256: u32 = 0b11 << 16;
    pub const BSEX: u32 = 1 << 25;        // Buffer Size Extension
    pub const SECRC: u32 = 1 << 26;       // Strip Ethernet CRC
}

/// TCTL (Transmit Control) register bits
pub mod tctl {
    pub const EN: u32 = 1 << 1;           // Transmitter Enable
    pub const PSP: u32 = 1 << 3;          // Pad Short Packets
    pub const CT_SHIFT: u32 = 4;          // Collision Threshold
    pub const CT_DEFAULT: u32 = 0x10 << 4;
    pub const COLD_SHIFT: u32 = 12;       // Collision Distance
    pub const COLD_FD: u32 = 0x40 << 12;  // Full Duplex
    pub const COLD_HD: u32 = 0x200 << 12; // Half Duplex
    pub const RTLC: u32 = 1 << 24;        // Re-transmit on Late Collision
}

/// Interrupt bits
pub mod interrupt {
    pub const TXDW: u32 = 1 << 0;   // TX Descriptor Written Back
    pub const TXQE: u32 = 1 << 1;   // TX Queue Empty
    pub const LSC: u32 = 1 << 2;    // Link Status Change
    pub const RXSEQ: u32 = 1 << 3;  // RX Sequence Error
    pub const RXDMT0: u32 = 1 << 4; // RX Desc Min Threshold
    pub const RXO: u32 = 1 << 6;    // RX Overrun
    pub const RXT0: u32 = 1 << 7;   // RX Timer Interrupt
}

/// Number of RX/TX descriptors
const RX_DESC_COUNT: usize = 32;
const TX_DESC_COUNT: usize = 32;
const RX_BUFFER_SIZE: usize = 2048;

/// Device statistics with atomic counters for lock-free updates.
#[derive(Debug)]
pub struct DeviceStats {
    pub tx_packets: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub tx_errors: AtomicU64,
    pub tx_dropped: AtomicU64,
    pub rx_packets: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub rx_errors: AtomicU64,
    pub rx_dropped: AtomicU64,
}

impl DeviceStats {
    /// Creates a new DeviceStats with all counters initialized to zero.
    pub const fn new() -> Self {
        Self {
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
        }
    }
}

/// RX Descriptor (Legacy format)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RxDescriptor {
    pub buffer_addr: u64,   // Physical address of buffer
    pub length: u16,        // Length of received packet
    pub checksum: u16,      // Packet checksum
    pub status: u8,         // Descriptor status
    pub errors: u8,         // Errors
    pub special: u16,       // Special field (VLAN, etc.)
}

/// RX descriptor status bits
pub mod rx_status {
    pub const DD: u8 = 1 << 0;    // Descriptor Done
    pub const EOP: u8 = 1 << 1;   // End of Packet
}

/// TX Descriptor (Legacy format)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxDescriptor {
    pub buffer_addr: u64,   // Physical address of buffer
    pub length: u16,        // Length of data to send
    pub cso: u8,            // Checksum Offset
    pub cmd: u8,            // Command field
    pub status: u8,         // Status (DD bit)
    pub css: u8,            // Checksum Start
    pub special: u16,       // Special field
}

/// TX command bits
pub mod tx_cmd {
    pub const EOP: u8 = 1 << 0;   // End of Packet
    pub const IFCS: u8 = 1 << 1;  // Insert FCS (CRC)
    pub const IC: u8 = 1 << 2;    // Insert Checksum
    pub const RS: u8 = 1 << 3;    // Report Status
    pub const RPS: u8 = 1 << 4;   // Report Packet Sent
    pub const DEXT: u8 = 1 << 5;  // Descriptor Extension (not used in legacy)
    pub const VLE: u8 = 1 << 6;   // VLAN Packet Enable
    pub const IDE: u8 = 1 << 7;   // Interrupt Delay Enable
}

/// TX status bits
pub mod tx_status {
    pub const DD: u8 = 1 << 0;    // Descriptor Done
}

/// E1000 Network Device
pub struct E1000Device {
    /// Memory-mapped I/O base address
    mmio_base: u64,
    /// MAC address
    mac: MacAddress,
    /// RX descriptors
    rx_descs: Mutex<&'static mut [RxDescriptor]>,
    /// TX descriptors
    tx_descs: Mutex<&'static mut [TxDescriptor]>,
    /// RX buffers
    rx_buffers: Mutex<Vec<Vec<u8>>>,
    /// TX buffers
    tx_buffers: Mutex<Vec<Vec<u8>>>,
    /// Current RX descriptor index
    rx_cur: AtomicU32,
    /// Current TX descriptor index
    tx_cur: AtomicU32,
    /// Link status
    link_up: AtomicBool,
    /// Initialized
    initialized: AtomicBool,
    /// Received packets queue
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// MTU
    mtu: u16,
    /// Device statistics
    stats: DeviceStats,
}

impl E1000Device {
    /// Creates a new E1000 device with the given MMIO base address.
    pub fn new(mmio_base: u64) -> Self {
        // Allocate RX descriptors (must be 16-byte aligned)
        let rx_desc_layout = core::alloc::Layout::from_size_align(
            RX_DESC_COUNT * core::mem::size_of::<RxDescriptor>(),
            16
        ).unwrap();
        let rx_desc_ptr = unsafe { alloc::alloc::alloc_zeroed(rx_desc_layout) } as *mut RxDescriptor;
        let rx_descs = unsafe { core::slice::from_raw_parts_mut(rx_desc_ptr, RX_DESC_COUNT) };
        
        // Allocate TX descriptors
        let tx_desc_layout = core::alloc::Layout::from_size_align(
            TX_DESC_COUNT * core::mem::size_of::<TxDescriptor>(),
            16
        ).unwrap();
        let tx_desc_ptr = unsafe { alloc::alloc::alloc_zeroed(tx_desc_layout) } as *mut TxDescriptor;
        let tx_descs = unsafe { core::slice::from_raw_parts_mut(tx_desc_ptr, TX_DESC_COUNT) };
        
        // Allocate RX buffers
        let mut rx_buffers = Vec::with_capacity(RX_DESC_COUNT);
        for i in 0..RX_DESC_COUNT {
            let buffer = vec![0u8; RX_BUFFER_SIZE];
            rx_descs[i].buffer_addr = buffer.as_ptr() as u64;
            rx_buffers.push(buffer);
        }
        
        // Allocate TX buffers
        let mut tx_buffers = Vec::with_capacity(TX_DESC_COUNT);
        for _ in 0..TX_DESC_COUNT {
            tx_buffers.push(vec![0u8; RX_BUFFER_SIZE]);
        }
        
        Self {
            mmio_base,
            mac: MacAddress([0; 6]),
            rx_descs: Mutex::new(rx_descs),
            tx_descs: Mutex::new(tx_descs),
            rx_buffers: Mutex::new(rx_buffers),
            tx_buffers: Mutex::new(tx_buffers),
            rx_cur: AtomicU32::new(0),
            tx_cur: AtomicU32::new(0),
            link_up: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            rx_queue: Mutex::new(VecDeque::new()),
            mtu: 1500,
            stats: DeviceStats::new(),
        }
    }
    
    /// Read a 32-bit register
    fn read_reg(&self, reg: u32) -> u32 {
        unsafe {
            core::ptr::read_volatile((self.mmio_base + reg as u64) as *const u32)
        }
    }
    
    /// Write a 32-bit register
    fn write_reg(&self, reg: u32, value: u32) {
        unsafe {
            core::ptr::write_volatile((self.mmio_base + reg as u64) as *mut u32, value);
        }
    }
    
    /// Initialize the E1000 device
    pub fn init(&mut self) -> Result<(), NetworkError> {
        // 1. Disable interrupts
        self.write_reg(regs::IMC, 0xFFFFFFFF);
        self.read_reg(regs::ICR); // Clear pending interrupts
        
        // 2. Reset the device
        self.write_reg(regs::CTRL, ctrl::RST);
        
        // Wait for reset to complete
        for _ in 0..1000 {
            if self.read_reg(regs::CTRL) & ctrl::RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        
        // Small delay after reset
        for _ in 0..10000 {
            core::hint::spin_loop();
        }
        
        // 3. Disable interrupts again after reset
        self.write_reg(regs::IMC, 0xFFFFFFFF);
        
        // 4. Set up link
        let ctrl = self.read_reg(regs::CTRL);
        self.write_reg(regs::CTRL, ctrl | ctrl::SLU | ctrl::ASDE);
        
        // 5. Read MAC address from EEPROM or RAL/RAH
        self.read_mac_address();
        
        // 6. Initialize Multicast Table Array (clear all)
        for i in 0..128 {
            self.write_reg(regs::MTA + i * 4, 0);
        }
        
        // 7. Set up receive descriptors
        self.init_rx()?;
        
        // 8. Set up transmit descriptors
        self.init_tx()?;
        
        // 9. Enable interrupts for RX
        self.write_reg(regs::IMS, interrupt::RXT0 | interrupt::LSC | interrupt::RXDMT0);
        
        // 10. Check link status
        let status = self.read_reg(regs::STATUS);
        self.link_up.store(status & status::LU != 0, Ordering::SeqCst);
        
        self.initialized.store(true, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Read MAC address from EEPROM
    fn read_mac_address(&mut self) {
        // Try reading from RAL/RAH first (may be pre-configured)
        let ral = self.read_reg(regs::RAL);
        let rah = self.read_reg(regs::RAH);
        
        if ral != 0 && (rah & 0xFFFF) != 0 {
            self.mac.0[0] = (ral >> 0) as u8;
            self.mac.0[1] = (ral >> 8) as u8;
            self.mac.0[2] = (ral >> 16) as u8;
            self.mac.0[3] = (ral >> 24) as u8;
            self.mac.0[4] = (rah >> 0) as u8;
            self.mac.0[5] = (rah >> 8) as u8;
        } else {
            // Read from EEPROM
            for i in 0..3 {
                let word = self.eeprom_read(i as u8);
                self.mac.0[i * 2] = (word & 0xFF) as u8;
                self.mac.0[i * 2 + 1] = (word >> 8) as u8;
            }
            
            // Write MAC to RAL/RAH
            let ral = (self.mac.0[0] as u32)
                | ((self.mac.0[1] as u32) << 8)
                | ((self.mac.0[2] as u32) << 16)
                | ((self.mac.0[3] as u32) << 24);
            let rah = (self.mac.0[4] as u32)
                | ((self.mac.0[5] as u32) << 8)
                | (1 << 31); // Address Valid
            
            self.write_reg(regs::RAL, ral);
            self.write_reg(regs::RAH, rah);
        }
    }
    
    /// Read a word from EEPROM
    fn eeprom_read(&self, addr: u8) -> u16 {
        // Write address and start read
        self.write_reg(regs::EERD, ((addr as u32) << 8) | 1);
        
        // Wait for read to complete
        loop {
            let eerd = self.read_reg(regs::EERD);
            if eerd & (1 << 4) != 0 {
                return ((eerd >> 16) & 0xFFFF) as u16;
            }
            core::hint::spin_loop();
        }
    }
    
    /// Initialize RX ring
    fn init_rx(&self) -> Result<(), NetworkError> {
        let rx_descs = self.rx_descs.lock();
        
        // Set RX descriptor base address
        let rx_base = rx_descs.as_ptr() as u64;
        self.write_reg(regs::RDBAL, rx_base as u32);
        self.write_reg(regs::RDBAH, (rx_base >> 32) as u32);
        
        // Set descriptor length
        let rx_len = (RX_DESC_COUNT * core::mem::size_of::<RxDescriptor>()) as u32;
        self.write_reg(regs::RDLEN, rx_len);
        
        // Set head and tail
        self.write_reg(regs::RDH, 0);
        self.write_reg(regs::RDT, (RX_DESC_COUNT - 1) as u32);
        
        // Enable receiver
        let rctl = rctl::EN
            | rctl::BAM           // Accept broadcast
            | rctl::BSIZE_2048    // 2048 byte buffers
            | rctl::SECRC;        // Strip CRC
        
        self.write_reg(regs::RCTL, rctl);
        
        Ok(())
    }
    
    /// Initialize TX ring
    fn init_tx(&self) -> Result<(), NetworkError> {
        let tx_descs = self.tx_descs.lock();
        
        // Set TX descriptor base address
        let tx_base = tx_descs.as_ptr() as u64;
        self.write_reg(regs::TDBAL, tx_base as u32);
        self.write_reg(regs::TDBAH, (tx_base >> 32) as u32);
        
        // Set descriptor length
        let tx_len = (TX_DESC_COUNT * core::mem::size_of::<TxDescriptor>()) as u32;
        self.write_reg(regs::TDLEN, tx_len);
        
        // Set head and tail
        self.write_reg(regs::TDH, 0);
        self.write_reg(regs::TDT, 0);
        
        // Set transmit IPG (Inter Packet Gap) for IEEE 802.3 standard
        // IPGT=10, IPGR1=8, IPGR2=6 for full duplex
        self.write_reg(regs::TIPG, 10 | (8 << 10) | (6 << 20));
        
        // Enable transmitter
        let tctl = tctl::EN
            | tctl::PSP           // Pad short packets
            | tctl::CT_DEFAULT    // Collision threshold
            | tctl::COLD_FD;      // Full duplex collision distance
        
        self.write_reg(regs::TCTL, tctl);
        
        Ok(())
    }
    
    /// Transmit a packet
    pub fn transmit(&self, data: &[u8]) -> Result<(), NetworkError> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(NetworkError::NotReady);
        }
        
        if data.len() > self.mtu as usize + 14 {
            return Err(NetworkError::InvalidPacket);
        }
        
        let mut tx_descs = self.tx_descs.lock();
        let mut tx_buffers = self.tx_buffers.lock();
        
        let cur = self.tx_cur.load(Ordering::SeqCst) as usize;
        let desc = &mut tx_descs[cur];
        
        // Wait for descriptor to be available
        if desc.status & tx_status::DD == 0 && desc.cmd != 0 {
            return Err(NetworkError::NoBuffer);
        }
        
        // Copy data to buffer
        let buffer = &mut tx_buffers[cur];
        buffer[..data.len()].copy_from_slice(data);
        
        // Set up descriptor
        desc.buffer_addr = buffer.as_ptr() as u64;
        desc.length = data.len() as u16;
        desc.cmd = tx_cmd::EOP | tx_cmd::IFCS | tx_cmd::RS;
        desc.status = 0;
        
        // Update tail pointer
        let next = ((cur + 1) % TX_DESC_COUNT) as u32;
        self.tx_cur.store(next, Ordering::SeqCst);
        self.write_reg(regs::TDT, next);
        
        Ok(())
    }
    
    /// Poll for received packets
    pub fn poll_rx(&self) {
        if !self.initialized.load(Ordering::SeqCst) {
            return;
        }
        
        let mut rx_descs = self.rx_descs.lock();
        let rx_buffers = self.rx_buffers.lock();
        let mut rx_queue = self.rx_queue.lock();
        
        loop {
            let cur = self.rx_cur.load(Ordering::SeqCst) as usize;
            let desc = &mut rx_descs[cur];
            
            // Check if descriptor is done
            if desc.status & rx_status::DD == 0 {
                break;
            }
            
            // Check for errors
            if desc.errors == 0 && desc.status & rx_status::EOP != 0 {
                let len = desc.length as usize;
                if len > 0 && len <= RX_BUFFER_SIZE {
                    let packet = rx_buffers[cur][..len].to_vec();
                    rx_queue.push_back(packet);
                }
            }
            
            // Reset descriptor for reuse
            desc.status = 0;
            
            // Update current and tail
            let next = ((cur + 1) % RX_DESC_COUNT) as u32;
            self.rx_cur.store(next, Ordering::SeqCst);
            self.write_reg(regs::RDT, cur as u32);
        }
    }
    
    /// Get a received packet
    pub fn get_packet(&self) -> Option<Vec<u8>> {
        self.poll_rx();
        self.rx_queue.lock().pop_front()
    }
    
    /// Handle interrupt
    pub fn handle_interrupt(&self) {
        let icr = self.read_reg(regs::ICR);
        
        if icr & interrupt::LSC != 0 {
            // Link status changed
            let status = self.read_reg(regs::STATUS);
            self.link_up.store(status & status::LU != 0, Ordering::SeqCst);
        }
        
        if icr & (interrupt::RXT0 | interrupt::RXDMT0) != 0 {
            // Received packets
            self.poll_rx();
        }
    }
}

impl NetworkDevice for E1000Device {
    fn info(&self) -> NetworkDeviceInfo {
        let speed = match self.read_reg(regs::STATUS) & status::SPEED_MASK {
            status::SPEED_1000 => 1000,
            status::SPEED_100 => 100,
            _ => 10,
        };
        
        NetworkDeviceInfo {
            name: "e1000",
            mac: self.mac,
            mtu: self.mtu,
            link_speed: speed,
            link_up: self.link_up.load(Ordering::SeqCst),
            tx_packets: self.stats.tx_packets.load(Ordering::Relaxed),
            tx_bytes: self.stats.tx_bytes.load(Ordering::Relaxed),
            tx_errors: self.stats.tx_errors.load(Ordering::Relaxed),
            tx_dropped: self.stats.tx_dropped.load(Ordering::Relaxed),
            rx_packets: self.stats.rx_packets.load(Ordering::Relaxed),
            rx_bytes: self.stats.rx_bytes.load(Ordering::Relaxed),
            rx_errors: self.stats.rx_errors.load(Ordering::Relaxed),
            rx_dropped: self.stats.rx_dropped.load(Ordering::Relaxed),
        }
    }
    
    fn send(&self, data: &[u8]) -> Result<(), NetworkError> {
        self.transmit(data)
    }
    
    fn receive(&self) -> Result<Vec<u8>, NetworkError> {
        match self.get_packet() {
            Some(packet) => Ok(packet),
            None => Err(NetworkError::WouldBlock),
        }
    }
    
    fn is_ready(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }
    
    fn link_up(&self) -> bool {
        self.link_up.load(Ordering::SeqCst)
    }
}

/// Probe for E1000 devices on the PCI bus
#[cfg(target_arch = "x86_64")]
pub fn probe_e1000() -> Option<Arc<Mutex<E1000Device>>> {
    use core::fmt::Write;
    
    for bus in 0..8u8 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let addr = pci_address(bus, device, function, 0);
                let vendor_device = unsafe { pci_config_read(addr) };
                
                let vendor_id = (vendor_device & 0xFFFF) as u16;
                let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
                
                if vendor_id == 0xFFFF {
                    continue;
                }
                
                // Check for any E1000 variant
                if vendor_id == INTEL_VENDOR_ID {
                    let is_e1000 = matches!(
                        device_id,
                        device_ids::E1000_82540EM |
                        device_ids::E1000_82543GC |
                        device_ids::E1000_82545EM |
                        device_ids::E1000_82541EI |
                        device_ids::E1000_82574L |
                        device_ids::E1000E_82579LM
                    );
                    
                    if is_e1000 {
                        // Read BAR0 (Memory mapped)
                        let bar0 = unsafe { pci_config_read(pci_address(bus, device, function, 0x10)) };
                        
                        if bar0 & 1 == 0 {
                            // Memory-mapped BAR
                            let mmio_base = (bar0 & 0xFFFFFFF0) as u64;
                            
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial,
                                    "[e1000] Found E1000 at {:02}:{:02}.{} mmio=0x{:08x}",
                                    bus, device, function, mmio_base);
                            }
                            
                            // Map the MMIO region before accessing it
                            // E1000 needs about 128KB of MMIO space
                            let mmio_virt = crate::arch::x86_64::paging::map_mmio(mmio_base, 0x20000);
                            
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial, "[e1000] MMIO mapped at 0x{:08x}", mmio_virt);
                            }
                            
                            // Enable bus mastering and memory space
                            let cmd = unsafe { pci_config_read(pci_address(bus, device, function, 0x04)) };
                            unsafe { pci_config_write(pci_address(bus, device, function, 0x04), cmd | 0x06) };
                            
                            let mut dev = E1000Device::new(mmio_virt);
                            if dev.init().is_ok() {
                                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                    let _ = writeln!(serial,
                                        "[e1000] Initialized: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                        dev.mac.0[0], dev.mac.0[1], dev.mac.0[2],
                                        dev.mac.0[3], dev.mac.0[4], dev.mac.0[5]);
                                }
                                return Some(Arc::new(Mutex::new(dev)));
                            }
                        }
                    }
                }
            }
        }
    }
    
    None
}

#[cfg(target_arch = "x86_64")]
fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x80000000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

#[cfg(target_arch = "x86_64")]
unsafe fn pci_config_read(address: u32) -> u32 {
    use core::arch::asm;
    let mut result: u32;
    unsafe {
        asm!(
            "mov dx, 0xCF8",
            "out dx, eax",
            "mov dx, 0xCFC",
            "in eax, dx",
            in("eax") address,
            lateout("eax") result,
            out("dx") _,
        );
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn pci_config_write(address: u32, value: u32) {
    use core::arch::asm;
    unsafe {
        asm!(
            "mov dx, 0xCF8",
            "out dx, eax",
            "mov dx, 0xCFC",
            "mov eax, {value:e}",
            "out dx, eax",
            in("eax") address,
            value = in(reg) value,
            out("dx") _,
        );
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn probe_e1000() -> Option<Arc<Mutex<E1000Device>>> {
    None
}
