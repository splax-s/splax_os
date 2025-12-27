//! # Realtek RTL8139 Network Driver
//!
//! Driver for Realtek RTL8139 Fast Ethernet Controller.
//! This is one of the simplest and most commonly emulated NICs.
//!
//! The RTL8139 uses a simple ring buffer for RX and a single TX buffer.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use super::device::{NetworkDevice, NetworkDeviceInfo, NetworkError};
use super::ethernet::MacAddress;

/// Realtek Vendor ID
pub const REALTEK_VENDOR_ID: u16 = 0x10EC;

/// RTL8139 Device IDs
pub mod device_ids {
    pub const RTL8139: u16 = 0x8139;
    pub const RTL8139_K: u16 = 0x8138;
}

/// RTL8139 Register offsets (I/O space)
pub mod regs {
    pub const MAC0: u16 = 0x00;        // MAC address bytes 0-3
    pub const MAC4: u16 = 0x04;        // MAC address bytes 4-5
    pub const MAR0: u16 = 0x08;        // Multicast filter 0-3
    pub const MAR4: u16 = 0x0C;        // Multicast filter 4-7
    pub const TSD0: u16 = 0x10;        // TX status descriptor 0
    pub const TSD1: u16 = 0x14;        // TX status descriptor 1
    pub const TSD2: u16 = 0x18;        // TX status descriptor 2
    pub const TSD3: u16 = 0x1C;        // TX status descriptor 3
    pub const TSAD0: u16 = 0x20;       // TX start address 0
    pub const TSAD1: u16 = 0x24;       // TX start address 1
    pub const TSAD2: u16 = 0x28;       // TX start address 2
    pub const TSAD3: u16 = 0x2C;       // TX start address 3
    pub const RBSTART: u16 = 0x30;     // RX buffer start address
    pub const ERBCR: u16 = 0x34;       // Early RX byte count
    pub const ERSR: u16 = 0x36;        // Early RX status
    pub const CMD: u16 = 0x37;         // Command register
    pub const CAPR: u16 = 0x38;        // Current address of packet read
    pub const CBR: u16 = 0x3A;         // Current buffer address
    pub const IMR: u16 = 0x3C;         // Interrupt mask
    pub const ISR: u16 = 0x3E;         // Interrupt status
    pub const TCR: u16 = 0x40;         // TX configuration
    pub const RCR: u16 = 0x44;         // RX configuration
    pub const TCTR: u16 = 0x48;        // Timer count
    pub const MPC: u16 = 0x4C;         // Missed packet counter
    pub const CR9346: u16 = 0x50;      // 93C46 command register
    pub const CONFIG0: u16 = 0x51;     // Configuration 0
    pub const CONFIG1: u16 = 0x52;     // Configuration 1
    pub const MSR: u16 = 0x58;         // Media status
    pub const CONFIG3: u16 = 0x59;     // Configuration 3
    pub const CONFIG4: u16 = 0x5A;     // Configuration 4
    pub const BMCR: u16 = 0x62;        // Basic mode control
    pub const BMSR: u16 = 0x64;        // Basic mode status
}

/// Command register bits
pub mod cmd {
    pub const BUFE: u8 = 1 << 0;       // Buffer empty
    pub const TE: u8 = 1 << 2;         // Transmitter enable
    pub const RE: u8 = 1 << 3;         // Receiver enable
    pub const RST: u8 = 1 << 4;        // Reset
}

/// TX status bits
pub mod tx_status {
    pub const OWN: u32 = 1 << 13;      // DMA completed
    pub const TUN: u32 = 1 << 14;      // TX underrun
    pub const TOK: u32 = 1 << 15;      // TX OK
    pub const OWC: u32 = 1 << 29;      // Out of window collision
    pub const TABT: u32 = 1 << 30;     // TX aborted
    pub const CRS: u32 = 1 << 31;      // Carrier sense lost
}

/// RX configuration bits
pub mod rx_config {
    pub const AAP: u32 = 1 << 0;       // Accept all packets
    pub const APM: u32 = 1 << 1;       // Accept physical match
    pub const AM: u32 = 1 << 2;        // Accept multicast
    pub const AB: u32 = 1 << 3;        // Accept broadcast
    pub const AR: u32 = 1 << 4;        // Accept runt
    pub const AER: u32 = 1 << 5;       // Accept error packet
    pub const WRAP: u32 = 1 << 7;      // Wrap mode (continuous buffer)
    // Buffer length: 00 = 8K+16, 01 = 16K+16, 10 = 32K+16, 11 = 64K+16
    pub const RBLEN_8K: u32 = 0b00 << 11;
    pub const RBLEN_16K: u32 = 0b01 << 11;
    pub const RBLEN_32K: u32 = 0b10 << 11;
    pub const RBLEN_64K: u32 = 0b11 << 11;
    // RX FIFO threshold (none = copy directly)
    pub const RXFTH_NONE: u32 = 0b111 << 13;
    // Max DMA burst size
    pub const MXDMA_UNLIM: u32 = 0b111 << 8;
}

/// TX configuration bits
pub mod tx_config {
    // Max DMA burst size
    pub const MXDMA_2048: u32 = 0b111 << 8;
    // Interframe gap
    pub const IFG_NORMAL: u32 = 0b11 << 24;
}

/// Interrupt bits
pub mod interrupt {
    pub const ROK: u16 = 1 << 0;       // RX OK
    pub const RER: u16 = 1 << 1;       // RX error
    pub const TOK: u16 = 1 << 2;       // TX OK
    pub const TER: u16 = 1 << 3;       // TX error
    pub const RXOVW: u16 = 1 << 4;     // RX buffer overflow
    pub const PUN: u16 = 1 << 5;       // Packet underrun
    pub const FOVW: u16 = 1 << 6;      // RX FIFO overflow
    pub const LENCHG: u16 = 1 << 13;   // Cable length change
    pub const TIMEOUT: u16 = 1 << 14;  // Timeout
    pub const SERR: u16 = 1 << 15;     // System error
}

/// RX packet header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct RxPacketHeader {
    pub status: u16,
    pub length: u16,
}

/// RX header status bits
pub mod rx_status {
    pub const ROK: u16 = 1 << 0;       // Receive OK
    pub const FAE: u16 = 1 << 1;       // Frame alignment error
    pub const CRC: u16 = 1 << 2;       // CRC error
    pub const LONG: u16 = 1 << 3;      // Long packet
    pub const RUNT: u16 = 1 << 4;      // Runt packet
    pub const ISE: u16 = 1 << 5;       // Invalid symbol error
    pub const BAR: u16 = 1 << 13;      // Broadcast address
    pub const PAM: u16 = 1 << 14;      // Physical address matched
    pub const MAR: u16 = 1 << 15;      // Multicast address
}

/// RX buffer size (8K + 16 bytes + 1.5K for wrap)
const RX_BUFFER_SIZE: usize = 8192 + 16 + 1536;

/// TX buffer size
const TX_BUFFER_SIZE: usize = 1792;

/// Number of TX descriptors
const TX_DESC_COUNT: usize = 4;

/// RTL8139 Network Device
pub struct Rtl8139Device {
    /// I/O base address
    io_base: u16,
    /// MAC address
    mac: MacAddress,
    /// RX buffer
    rx_buffer: Mutex<Vec<u8>>,
    /// Current RX offset
    rx_offset: AtomicU32,
    /// TX buffers
    tx_buffers: Mutex<[Vec<u8>; TX_DESC_COUNT]>,
    /// Current TX descriptor
    tx_cur: AtomicU32,
    /// Link status
    link_up: AtomicBool,
    /// Initialized
    initialized: AtomicBool,
    /// Received packets queue
    rx_queue: Mutex<VecDeque<Vec<u8>>>,
    /// MTU
    mtu: u16,
}

impl Rtl8139Device {
    /// Creates a new RTL8139 device
    pub fn new(io_base: u16) -> Self {
        // Allocate RX buffer (must be contiguous and aligned)
        let rx_buffer = vec![0u8; RX_BUFFER_SIZE];
        
        // Allocate TX buffers
        let tx_buffers = [
            vec![0u8; TX_BUFFER_SIZE],
            vec![0u8; TX_BUFFER_SIZE],
            vec![0u8; TX_BUFFER_SIZE],
            vec![0u8; TX_BUFFER_SIZE],
        ];
        
        Self {
            io_base,
            mac: MacAddress([0; 6]),
            rx_buffer: Mutex::new(rx_buffer),
            rx_offset: AtomicU32::new(0),
            tx_buffers: Mutex::new(tx_buffers),
            tx_cur: AtomicU32::new(0),
            link_up: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            rx_queue: Mutex::new(VecDeque::new()),
            mtu: 1500,
        }
    }
    
    /// Read an 8-bit register
    fn read8(&self, reg: u16) -> u8 {
        unsafe { port_read_u8(self.io_base + reg) }
    }
    
    /// Write an 8-bit register
    fn write8(&self, reg: u16, value: u8) {
        unsafe { port_write_u8(self.io_base + reg, value) }
    }
    
    /// Read a 16-bit register
    fn read16(&self, reg: u16) -> u16 {
        unsafe { port_read_u16(self.io_base + reg) }
    }
    
    /// Write a 16-bit register
    fn write16(&self, reg: u16, value: u16) {
        unsafe { port_write_u16(self.io_base + reg, value) }
    }
    
    /// Read a 32-bit register
    fn read32(&self, reg: u16) -> u32 {
        unsafe { port_read_u32(self.io_base + reg) }
    }
    
    /// Write a 32-bit register
    fn write32(&self, reg: u16, value: u32) {
        unsafe { port_write_u32(self.io_base + reg, value) }
    }
    
    /// Initialize the device
    pub fn init(&mut self) -> Result<(), NetworkError> {
        // 1. Power on (turn on Config1)
        self.write8(regs::CONFIG1, 0);
        
        // 2. Reset the device
        self.write8(regs::CMD, cmd::RST);
        
        // Wait for reset to complete
        for _ in 0..1000 {
            if self.read8(regs::CMD) & cmd::RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        
        // 3. Read MAC address
        let mac0 = self.read32(regs::MAC0);
        let mac4 = self.read16(regs::MAC4);
        self.mac.0[0] = (mac0 >> 0) as u8;
        self.mac.0[1] = (mac0 >> 8) as u8;
        self.mac.0[2] = (mac0 >> 16) as u8;
        self.mac.0[3] = (mac0 >> 24) as u8;
        self.mac.0[4] = (mac4 >> 0) as u8;
        self.mac.0[5] = (mac4 >> 8) as u8;
        
        // 4. Set RX buffer address
        {
            let rx_buffer = self.rx_buffer.lock();
            self.write32(regs::RBSTART, rx_buffer.as_ptr() as u32);
        }
        
        // 5. Set TX buffer addresses
        {
            let tx_buffers = self.tx_buffers.lock();
            self.write32(regs::TSAD0, tx_buffers[0].as_ptr() as u32);
            self.write32(regs::TSAD1, tx_buffers[1].as_ptr() as u32);
            self.write32(regs::TSAD2, tx_buffers[2].as_ptr() as u32);
            self.write32(regs::TSAD3, tx_buffers[3].as_ptr() as u32);
        }
        
        // 6. Enable interrupts
        self.write16(regs::IMR, interrupt::ROK | interrupt::TOK | interrupt::RER | interrupt::TER);
        
        // 7. Configure RX: Accept broadcast + matching + multicast, 8K buffer, no WRAP
        let rcr = rx_config::APM      // Accept physical match
            | rx_config::AM           // Accept multicast
            | rx_config::AB           // Accept broadcast
            | rx_config::RBLEN_8K     // 8K + 16 buffer
            | rx_config::MXDMA_UNLIM  // Unlimited DMA burst
            | rx_config::RXFTH_NONE;  // No RX threshold
        self.write32(regs::RCR, rcr);
        
        // 8. Configure TX
        let tcr = tx_config::MXDMA_2048 | tx_config::IFG_NORMAL;
        self.write32(regs::TCR, tcr);
        
        // 9. Reset RX counters
        self.write16(regs::CAPR, 0xFFF0);
        self.rx_offset.store(0, Ordering::SeqCst);
        
        // 10. Enable RX and TX
        self.write8(regs::CMD, cmd::RE | cmd::TE);
        
        // 11. Check link status
        self.link_up.store(true, Ordering::SeqCst); // Assume up for now
        
        self.initialized.store(true, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Transmit a packet
    pub fn transmit(&self, data: &[u8]) -> Result<(), NetworkError> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(NetworkError::NotReady);
        }
        
        if data.len() > TX_BUFFER_SIZE {
            return Err(NetworkError::InvalidPacket);
        }
        
        let cur = self.tx_cur.load(Ordering::SeqCst) as usize;
        
        // Wait for previous TX to complete
        let tsd_reg = match cur {
            0 => regs::TSD0,
            1 => regs::TSD1,
            2 => regs::TSD2,
            3 => regs::TSD3,
            _ => unreachable!(),
        };
        
        // Check if descriptor is available
        let status = self.read32(tsd_reg);
        if status & tx_status::OWN == 0 && status != 0 {
            // Previous TX still in progress
            return Err(NetworkError::NoBuffer);
        }
        
        // Copy data to TX buffer
        {
            let mut tx_buffers = self.tx_buffers.lock();
            tx_buffers[cur][..data.len()].copy_from_slice(data);
        }
        
        // Start transmission
        // Bit 0-12: size, bit 13: OWN (cleared), we write size only
        let size = data.len() as u32;
        self.write32(tsd_reg, size);
        
        // Move to next descriptor
        self.tx_cur.store(((cur + 1) % TX_DESC_COUNT) as u32, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Poll for received packets
    pub fn poll_rx(&self) {
        if !self.initialized.load(Ordering::SeqCst) {
            return;
        }
        
        let rx_buffer = self.rx_buffer.lock();
        let mut rx_queue = self.rx_queue.lock();
        
        loop {
            // Check if buffer is empty
            let cmd = self.read8(regs::CMD);
            if cmd & cmd::BUFE != 0 {
                break;
            }
            
            let offset = self.rx_offset.load(Ordering::SeqCst) as usize;
            
            // Read packet header
            let header = unsafe {
                let ptr = rx_buffer.as_ptr().add(offset) as *const RxPacketHeader;
                core::ptr::read_unaligned(ptr)
            };
            
            // Check for valid packet
            if header.status & rx_status::ROK != 0 {
                let len = (header.length - 4) as usize; // Subtract CRC
                
                if len > 0 && len <= self.mtu as usize + 14 {
                    // Copy packet data (starts after header)
                    let data_offset = offset + 4;
                    let mut packet = vec![0u8; len];
                    
                    // Handle wrap around
                    if data_offset + len <= RX_BUFFER_SIZE - 16 {
                        packet.copy_from_slice(&rx_buffer[data_offset..data_offset + len]);
                    } else {
                        // Packet wraps around
                        let first_part = RX_BUFFER_SIZE - 16 - data_offset;
                        packet[..first_part].copy_from_slice(&rx_buffer[data_offset..RX_BUFFER_SIZE - 16]);
                        packet[first_part..].copy_from_slice(&rx_buffer[..len - first_part]);
                    }
                    
                    rx_queue.push_back(packet);
                }
            }
            
            // Calculate next offset (4-byte aligned)
            let packet_len = header.length as usize;
            let next_offset = (offset + packet_len + 4 + 3) & !3;
            let next_offset = next_offset % (RX_BUFFER_SIZE - 16);
            
            self.rx_offset.store(next_offset as u32, Ordering::SeqCst);
            
            // Update CAPR (Current Address of Packet Read)
            // CAPR must be 16 bytes behind the actual read offset
            let capr = if next_offset >= 16 {
                (next_offset - 16) as u16
            } else {
                ((RX_BUFFER_SIZE - 16 - 16) + next_offset) as u16
            };
            self.write16(regs::CAPR, capr);
        }
    }
    
    /// Get a received packet
    pub fn get_packet(&self) -> Option<Vec<u8>> {
        self.poll_rx();
        self.rx_queue.lock().pop_front()
    }
    
    /// Handle interrupt
    pub fn handle_interrupt(&self) {
        let isr = self.read16(regs::ISR);
        
        // Acknowledge all interrupts
        self.write16(regs::ISR, isr);
        
        if isr & interrupt::ROK != 0 {
            self.poll_rx();
        }
    }
}

impl NetworkDevice for Rtl8139Device {
    fn info(&self) -> NetworkDeviceInfo {
        NetworkDeviceInfo {
            name: "rtl8139",
            mac: self.mac,
            mtu: self.mtu,
            link_speed: 100, // 100 Mbps
            link_up: self.link_up.load(Ordering::SeqCst),
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

// === Port I/O functions ===

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u8(port: u16) -> u8 {
    use core::arch::asm;
    let result: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, lateout("al") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u8(port: u16, value: u8) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nostack));
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u16(port: u16) -> u16 {
    use core::arch::asm;
    let result: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, lateout("ax") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u16(port: u16, value: u16) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value, options(nostack));
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u32(port: u16) -> u32 {
    use core::arch::asm;
    let result: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, lateout("eax") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u32(port: u16, value: u32) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nostack));
    }
}

/// Probe for RTL8139 devices on the PCI bus
#[cfg(target_arch = "x86_64")]
pub fn probe_rtl8139() -> Option<Arc<Mutex<Rtl8139Device>>> {
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
                
                // Check for RTL8139
                if vendor_id == REALTEK_VENDOR_ID &&
                   (device_id == device_ids::RTL8139 || device_id == device_ids::RTL8139_K) {
                    
                    // Read BAR0 (I/O space)
                    let bar0 = unsafe { pci_config_read(pci_address(bus, device, function, 0x10)) };
                    
                    if bar0 & 1 != 0 {
                        // I/O space BAR
                        let io_base = (bar0 & 0xFFFC) as u16;
                        
                        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                            let _ = writeln!(serial,
                                "[rtl8139] Found RTL8139 at {:02}:{:02}.{} io=0x{:04x}",
                                bus, device, function, io_base);
                        }
                        
                        // Enable bus mastering and I/O space
                        let cmd = unsafe { pci_config_read(pci_address(bus, device, function, 0x04)) };
                        unsafe { pci_config_write(pci_address(bus, device, function, 0x04), cmd | 0x07) };
                        
                        let mut dev = Rtl8139Device::new(io_base);
                        if dev.init().is_ok() {
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial,
                                    "[rtl8139] Initialized: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
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
pub fn probe_rtl8139() -> Option<Arc<Mutex<Rtl8139Device>>> {
    None
}
