//! xHCI (eXtensible Host Controller Interface) Driver
//!
//! This module implements a driver for USB 3.x xHCI controllers.
//! xHCI is the standard host controller interface for USB 3.0 and later.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    xHCI Controller                       │
//! ├─────────────────────────────────────────────────────────┤
//! │  Capability Registers  │  Operational Registers         │
//! ├─────────────────────────────────────────────────────────┤
//! │  Runtime Registers     │  Doorbell Registers            │
//! ├─────────────────────────────────────────────────────────┤
//! │  Device Context Array  │  Command Ring                  │
//! ├─────────────────────────────────────────────────────────┤
//! │  Event Ring            │  Transfer Rings                │
//! └─────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use super::{
    Direction, SetupPacket, TransferResult, UsbHostController, UsbSpeed,
};

/// xHCI PCI vendor/device IDs
pub const XHCI_CLASS: u8 = 0x0C;
pub const XHCI_SUBCLASS: u8 = 0x03;
pub const XHCI_PROG_IF: u8 = 0x30;

/// xHCI Capability Registers offsets
mod cap_regs {
    pub const CAPLENGTH: usize = 0x00;
    pub const HCIVERSION: usize = 0x02;
    pub const HCSPARAMS1: usize = 0x04;
    pub const HCSPARAMS2: usize = 0x08;
    pub const HCSPARAMS3: usize = 0x0C;
    pub const HCCPARAMS1: usize = 0x10;
    pub const DBOFF: usize = 0x14;
    pub const RTSOFF: usize = 0x18;
    pub const HCCPARAMS2: usize = 0x1C;
}

/// xHCI Operational Registers offsets
mod op_regs {
    pub const USBCMD: usize = 0x00;
    pub const USBSTS: usize = 0x04;
    pub const PAGESIZE: usize = 0x08;
    pub const DNCTRL: usize = 0x14;
    pub const CRCR: usize = 0x18;
    pub const DCBAAP: usize = 0x30;
    pub const CONFIG: usize = 0x38;
}

/// xHCI Port Register offsets (per port)
mod port_regs {
    pub const PORTSC: usize = 0x00;
    pub const PORTPMSC: usize = 0x04;
    pub const PORTLI: usize = 0x08;
    pub const PORTHLPMC: usize = 0x0C;
}

/// USB Command register bits
mod usbcmd {
    pub const RUN: u32 = 1 << 0;
    pub const HCRST: u32 = 1 << 1;
    pub const INTE: u32 = 1 << 2;
    pub const HSEE: u32 = 1 << 3;
    pub const LHCRST: u32 = 1 << 7;
    pub const CSS: u32 = 1 << 8;
    pub const CRS: u32 = 1 << 9;
    pub const EWE: u32 = 1 << 10;
    pub const EU3S: u32 = 1 << 11;
}

/// USB Status register bits
mod usbsts {
    pub const HCH: u32 = 1 << 0;  // HCHalted
    pub const HSE: u32 = 1 << 2;  // Host System Error
    pub const EINT: u32 = 1 << 3; // Event Interrupt
    pub const PCD: u32 = 1 << 4;  // Port Change Detect
    pub const SSS: u32 = 1 << 8;  // Save State Status
    pub const RSS: u32 = 1 << 9;  // Restore State Status
    pub const SRE: u32 = 1 << 10; // Save/Restore Error
    pub const CNR: u32 = 1 << 11; // Controller Not Ready
    pub const HCE: u32 = 1 << 12; // Host Controller Error
}

/// Port Status and Control bits
mod portsc {
    pub const CCS: u32 = 1 << 0;   // Current Connect Status
    pub const PED: u32 = 1 << 1;   // Port Enabled/Disabled
    pub const OCA: u32 = 1 << 3;   // Over-current Active
    pub const PR: u32 = 1 << 4;    // Port Reset
    pub const PLS_MASK: u32 = 0xF << 5; // Port Link State
    pub const PP: u32 = 1 << 9;    // Port Power
    pub const SPEED_MASK: u32 = 0xF << 10; // Port Speed
    pub const PIC_MASK: u32 = 0x3 << 14;   // Port Indicator Control
    pub const LWS: u32 = 1 << 16;  // Port Link State Write Strobe
    pub const CSC: u32 = 1 << 17;  // Connect Status Change
    pub const PEC: u32 = 1 << 18;  // Port Enabled/Disabled Change
    pub const WRC: u32 = 1 << 19;  // Warm Port Reset Change
    pub const OCC: u32 = 1 << 20;  // Over-current Change
    pub const PRC: u32 = 1 << 21;  // Port Reset Change
    pub const PLC: u32 = 1 << 22;  // Port Link State Change
    pub const CEC: u32 = 1 << 23;  // Port Config Error Change
    pub const CAS: u32 = 1 << 24;  // Cold Attach Status
    pub const WCE: u32 = 1 << 25;  // Wake on Connect Enable
    pub const WDE: u32 = 1 << 26;  // Wake on Disconnect Enable
    pub const WOE: u32 = 1 << 27;  // Wake on Over-current Enable
    pub const DR: u32 = 1 << 30;   // Device Removable
    pub const WPR: u32 = 1 << 31;  // Warm Port Reset
}

/// TRB (Transfer Request Block) types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    NoOp = 8,
    EnableSlotCommand = 9,
    DisableSlotCommand = 10,
    AddressDeviceCommand = 11,
    ConfigureEndpointCommand = 12,
    EvaluateContextCommand = 13,
    ResetEndpointCommand = 14,
    StopEndpointCommand = 15,
    SetTrDequeuePointerCommand = 16,
    ResetDeviceCommand = 17,
    ForceEventCommand = 18,
    NegotiateBandwidthCommand = 19,
    SetLatencyToleranceValueCommand = 20,
    GetPortBandwidthCommand = 21,
    ForceHeaderCommand = 22,
    NoOpCommand = 23,
    GetExtendedPropertyCommand = 24,
    SetExtendedPropertyCommand = 25,
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
    BandwidthRequestEvent = 35,
    DoorbellEvent = 36,
    HostControllerEvent = 37,
    DeviceNotificationEvent = 38,
    MfindexWrapEvent = 39,
}

/// TRB Completion Codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbCompletionCode {
    Invalid = 0,
    Success = 1,
    DataBufferError = 2,
    BabbleDetectedError = 3,
    UsbTransactionError = 4,
    TrbError = 5,
    StallError = 6,
    ResourceError = 7,
    BandwidthError = 8,
    NoSlotsAvailableError = 9,
    InvalidStreamTypeError = 10,
    SlotNotEnabledError = 11,
    EndpointNotEnabledError = 12,
    ShortPacket = 13,
    RingUnderrun = 14,
    RingOverrun = 15,
    VfEventRingFullError = 16,
    ParameterError = 17,
    BandwidthOverrunError = 18,
    ContextStateError = 19,
    NoPingResponseError = 20,
    EventRingFullError = 21,
    IncompatibleDeviceError = 22,
    MissedServiceError = 23,
    CommandRingStopped = 24,
    CommandAborted = 25,
    Stopped = 26,
    StoppedLengthInvalid = 27,
    StoppedShortPacket = 28,
    MaxExitLatencyTooLargeError = 29,
    IsochBufferOverrun = 31,
    EventLostError = 32,
    UndefinedError = 33,
    InvalidStreamIdError = 34,
    SecondaryBandwidthError = 35,
    SplitTransactionError = 36,
}

/// Transfer Request Block
#[derive(Debug, Clone, Copy)]
#[repr(C, align(16))]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub const fn new() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: 0,
        }
    }

    /// Set TRB type
    pub fn set_type(&mut self, trb_type: TrbType) {
        self.control = (self.control & !0xFC00) | ((trb_type as u32) << 10);
    }

    /// Get TRB type
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    /// Set cycle bit
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 1;
        } else {
            self.control &= !1;
        }
    }

    /// Get cycle bit
    pub fn cycle(&self) -> bool {
        (self.control & 1) != 0
    }

    /// Get completion code
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xFF) as u8
    }
}

/// Command Ring
pub struct CommandRing {
    trbs: Vec<Trb>,
    enqueue_ptr: usize,
    cycle_bit: bool,
}

impl CommandRing {
    pub fn new(size: usize) -> Self {
        let mut trbs = Vec::with_capacity(size);
        for _ in 0..size {
            trbs.push(Trb::new());
        }
        
        // Add link TRB at the end
        let last_idx = size - 1;
        trbs[last_idx].set_type(TrbType::Link);
        trbs[last_idx].control |= 1 << 1; // Toggle Cycle
        
        Self {
            trbs,
            enqueue_ptr: 0,
            cycle_bit: true,
        }
    }

    /// Enqueue a command TRB
    pub fn enqueue(&mut self, mut trb: Trb) -> usize {
        trb.set_cycle(self.cycle_bit);
        let idx = self.enqueue_ptr;
        self.trbs[idx] = trb;
        
        self.enqueue_ptr += 1;
        let ring_len = self.trbs.len();
        if self.enqueue_ptr >= ring_len - 1 {
            // Wrap around, toggle cycle bit
            let cycle = self.cycle_bit;
            self.trbs[ring_len - 1].set_cycle(cycle);
            self.cycle_bit = !self.cycle_bit;
            self.enqueue_ptr = 0;
        }
        
        idx
    }

    /// Get physical address of ring
    pub fn physical_address(&self) -> u64 {
        self.trbs.as_ptr() as u64
    }
}

/// Event Ring Segment Table Entry
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct EventRingSegmentTableEntry {
    pub ring_segment_base: u64,
    pub ring_segment_size: u16,
    pub reserved: [u8; 6],
}

/// Event Ring
pub struct EventRing {
    trbs: Vec<Trb>,
    segment_table: Vec<EventRingSegmentTableEntry>,
    dequeue_ptr: usize,
    cycle_bit: bool,
}

impl EventRing {
    pub fn new(size: usize) -> Self {
        let mut trbs = Vec::with_capacity(size);
        for _ in 0..size {
            trbs.push(Trb::new());
        }
        
        let mut segment_table = Vec::with_capacity(1);
        segment_table.push(EventRingSegmentTableEntry {
            ring_segment_base: trbs.as_ptr() as u64,
            ring_segment_size: size as u16,
            reserved: [0; 6],
        });
        
        Self {
            trbs,
            segment_table,
            dequeue_ptr: 0,
            cycle_bit: true,
        }
    }

    /// Check if there's an event to process
    pub fn has_event(&self) -> bool {
        self.trbs[self.dequeue_ptr].cycle() == self.cycle_bit
    }

    /// Dequeue an event TRB
    pub fn dequeue(&mut self) -> Option<Trb> {
        if !self.has_event() {
            return None;
        }
        
        let trb = self.trbs[self.dequeue_ptr];
        self.dequeue_ptr += 1;
        
        if self.dequeue_ptr >= self.trbs.len() {
            self.dequeue_ptr = 0;
            self.cycle_bit = !self.cycle_bit;
        }
        
        Some(trb)
    }

    /// Get segment table address
    pub fn segment_table_address(&self) -> u64 {
        self.segment_table.as_ptr() as u64
    }

    /// Get dequeue pointer
    pub fn dequeue_address(&self) -> u64 {
        (&self.trbs[self.dequeue_ptr] as *const Trb) as u64
    }
}

/// Transfer Ring
pub struct TransferRing {
    trbs: Vec<Trb>,
    enqueue_ptr: usize,
    cycle_bit: bool,
}

impl TransferRing {
    pub fn new(size: usize) -> Self {
        let mut trbs = Vec::with_capacity(size);
        for _ in 0..size {
            trbs.push(Trb::new());
        }
        
        // Add link TRB at the end
        let last_idx = size - 1;
        trbs[last_idx].set_type(TrbType::Link);
        trbs[last_idx].control |= 1 << 1; // Toggle Cycle
        
        Self {
            trbs,
            enqueue_ptr: 0,
            cycle_bit: true,
        }
    }

    /// Enqueue a transfer TRB
    pub fn enqueue(&mut self, mut trb: Trb) -> usize {
        trb.set_cycle(self.cycle_bit);
        let idx = self.enqueue_ptr;
        self.trbs[idx] = trb;
        
        self.enqueue_ptr += 1;
        let ring_len = self.trbs.len();
        if self.enqueue_ptr >= ring_len - 1 {
            let cycle = self.cycle_bit;
            self.trbs[ring_len - 1].set_cycle(cycle);
            self.cycle_bit = !self.cycle_bit;
            self.enqueue_ptr = 0;
        }
        
        idx
    }

    /// Get physical address
    pub fn physical_address(&self) -> u64 {
        self.trbs.as_ptr() as u64
    }
}

/// Device Slot
pub struct DeviceSlot {
    /// Slot ID (1-255)
    pub slot_id: u8,
    /// Device address
    pub device_address: u8,
    /// Device speed
    pub speed: UsbSpeed,
    /// Root hub port
    pub root_port: u8,
    /// Transfer rings for each endpoint
    pub transfer_rings: [Option<TransferRing>; 32],
    /// Is slot enabled
    pub enabled: bool,
}

impl DeviceSlot {
    pub fn new(slot_id: u8) -> Self {
        const NONE: Option<TransferRing> = None;
        Self {
            slot_id,
            device_address: 0,
            speed: UsbSpeed::Full,
            root_port: 0,
            transfer_rings: [NONE; 32],
            enabled: false,
        }
    }
}

/// Device Context Base Address Array
#[repr(C, align(64))]
pub struct DeviceContextBaseAddressArray {
    entries: [u64; 256],
}

impl DeviceContextBaseAddressArray {
    pub fn new() -> Self {
        Self { entries: [0; 256] }
    }
}

/// xHCI Host Controller
pub struct XhciController {
    /// Base address (MMIO)
    base_addr: usize,
    /// Capability register length
    cap_length: u8,
    /// Number of ports
    pub num_ports: u8,
    /// Number of device slots
    pub num_slots: u8,
    /// Command ring
    command_ring: CommandRing,
    /// Event ring
    event_ring: EventRing,
    /// Device context base address array
    dcbaa: Box<DeviceContextBaseAddressArray>,
    /// Device slots
    slots: Vec<DeviceSlot>,
    /// Address allocation bitmap
    address_bitmap: [u8; 16],
    /// Is controller running
    running: AtomicBool,
    /// Next available slot ID
    next_slot: AtomicU8,
}

impl XhciController {
    /// Create a new xHCI controller
    pub fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            cap_length: 0,
            num_ports: 0,
            num_slots: 0,
            command_ring: CommandRing::new(256),
            event_ring: EventRing::new(256),
            dcbaa: Box::new(DeviceContextBaseAddressArray::new()),
            slots: Vec::new(),
            address_bitmap: [0; 16],
            running: AtomicBool::new(false),
            next_slot: AtomicU8::new(1),
        }
    }

    /// Read capability register
    fn read_cap_reg(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base_addr + offset) as *const u32) }
    }

    /// Read operational register
    fn read_op_reg(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile((self.base_addr + self.cap_length as usize + offset) as *const u32)
        }
    }

    /// Write operational register
    fn write_op_reg(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(
                (self.base_addr + self.cap_length as usize + offset) as *mut u32,
                value,
            )
        }
    }

    /// Read port register
    fn read_port_reg(&self, port: u8, offset: usize) -> u32 {
        let port_offset = 0x400 + (port as usize * 0x10);
        unsafe {
            read_volatile(
                (self.base_addr + self.cap_length as usize + port_offset + offset) as *const u32,
            )
        }
    }

    /// Write port register
    fn write_port_reg(&self, port: u8, offset: usize, value: u32) {
        let port_offset = 0x400 + (port as usize * 0x10);
        unsafe {
            write_volatile(
                (self.base_addr + self.cap_length as usize + port_offset + offset) as *mut u32,
                value,
            )
        }
    }

    /// Wait for controller not ready to clear
    fn wait_cnr_clear(&self) -> Result<(), &'static str> {
        for _ in 0..1000 {
            if (self.read_op_reg(op_regs::USBSTS) & usbsts::CNR) == 0 {
                return Ok(());
            }
            // Small delay - in real implementation use proper timing
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        Err("xHCI: Controller not ready timeout")
    }

    /// Wait for halt
    fn wait_halt(&self) -> Result<(), &'static str> {
        for _ in 0..1000 {
            if (self.read_op_reg(op_regs::USBSTS) & usbsts::HCH) != 0 {
                return Ok(());
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        Err("xHCI: Halt timeout")
    }

    /// Ring doorbell
    fn ring_doorbell(&self, slot: u8, target: u8) {
        let db_offset = self.read_cap_reg(cap_regs::DBOFF);
        let doorbell_addr = self.base_addr + db_offset as usize + (slot as usize * 4);
        unsafe {
            write_volatile(doorbell_addr as *mut u32, target as u32);
        }
    }

    /// Send a command and wait for completion
    fn send_command(&mut self, trb: Trb) -> Result<Trb, &'static str> {
        self.command_ring.enqueue(trb);
        
        // Ring the command doorbell (slot 0, target 0)
        self.ring_doorbell(0, 0);
        
        // Wait for completion event
        for _ in 0..10000 {
            if let Some(event) = self.event_ring.dequeue() {
                if event.trb_type() == TrbType::CommandCompletionEvent as u8 {
                    return Ok(event);
                }
            }
            for _ in 0..100 {
                core::hint::spin_loop();
            }
        }
        
        Err("xHCI: Command timeout")
    }

    /// Perform a control transfer on the default endpoint
    fn control_transfer_slot(
        &mut self,
        slot_id: u8,
        setup: SetupPacket,
        data: Option<&mut [u8]>,
    ) -> TransferResult {
        // Get transfer ring for endpoint 0
        if slot_id == 0 || slot_id as usize > self.slots.len() {
            return TransferResult::HostError;
        }
        
        // For now, return a placeholder
        // Full implementation would queue TRBs to the endpoint's transfer ring
        TransferResult::Success(0)
    }
}

impl UsbHostController for XhciController {
    fn name(&self) -> &'static str {
        "xHCI USB 3.x Controller"
    }

    fn init(&mut self) -> Result<(), &'static str> {
        // Read capability length
        self.cap_length = (self.read_cap_reg(cap_regs::CAPLENGTH) & 0xFF) as u8;
        
        // Read HCI version
        let version = self.read_cap_reg(cap_regs::HCIVERSION) >> 16;
        
        // Read structural parameters
        let hcsparams1 = self.read_cap_reg(cap_regs::HCSPARAMS1);
        self.num_slots = (hcsparams1 & 0xFF) as u8;
        self.num_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
        
        // Wait for controller to be ready
        self.wait_cnr_clear()?;
        
        // Stop the controller if running
        let usbcmd = self.read_op_reg(op_regs::USBCMD);
        if (usbcmd & usbcmd::RUN) != 0 {
            self.write_op_reg(op_regs::USBCMD, usbcmd & !usbcmd::RUN);
            self.wait_halt()?;
        }
        
        // Reset the controller
        self.write_op_reg(op_regs::USBCMD, usbcmd::HCRST);
        
        // Wait for reset to complete
        for _ in 0..1000 {
            if (self.read_op_reg(op_regs::USBCMD) & usbcmd::HCRST) == 0 {
                break;
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        
        self.wait_cnr_clear()?;
        
        // Set number of device slots
        self.write_op_reg(op_regs::CONFIG, self.num_slots as u32);
        
        // Set up Device Context Base Address Array
        let dcbaa_addr = (&*self.dcbaa as *const DeviceContextBaseAddressArray) as u64;
        self.write_op_reg(op_regs::DCBAAP, dcbaa_addr as u32);
        self.write_op_reg(op_regs::DCBAAP + 4, (dcbaa_addr >> 32) as u32);
        
        // Set up Command Ring
        let crcr = self.command_ring.physical_address() | 1; // RCS = 1
        self.write_op_reg(op_regs::CRCR, crcr as u32);
        self.write_op_reg(op_regs::CRCR + 4, (crcr >> 32) as u32);
        
        // Set up Event Ring (via Runtime Registers)
        let rtsoff = self.read_cap_reg(cap_regs::RTSOFF);
        let interrupter_base = self.base_addr + rtsoff as usize + 0x20;
        
        // Event Ring Segment Table Size
        unsafe {
            write_volatile((interrupter_base + 0x08) as *mut u32, 1); // ERSTSZ
        }
        
        // Event Ring Dequeue Pointer
        let erdp = self.event_ring.dequeue_address();
        unsafe {
            write_volatile((interrupter_base + 0x18) as *mut u32, erdp as u32);
            write_volatile((interrupter_base + 0x1C) as *mut u32, (erdp >> 32) as u32);
        }
        
        // Event Ring Segment Table Base Address
        let erstba = self.event_ring.segment_table_address();
        unsafe {
            write_volatile((interrupter_base + 0x10) as *mut u32, erstba as u32);
            write_volatile((interrupter_base + 0x14) as *mut u32, (erstba >> 32) as u32);
        }
        
        // Enable interrupts for interrupter 0
        unsafe {
            let iman = read_volatile(interrupter_base as *const u32);
            write_volatile(interrupter_base as *mut u32, iman | 0x02); // IE
        }
        
        // Start the controller
        let usbcmd = self.read_op_reg(op_regs::USBCMD);
        self.write_op_reg(op_regs::USBCMD, usbcmd | usbcmd::RUN | usbcmd::INTE);
        
        // Wait for running
        for _ in 0..1000 {
            if (self.read_op_reg(op_regs::USBSTS) & usbsts::HCH) == 0 {
                self.running.store(true, Ordering::SeqCst);
                return Ok(());
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        
        Err("xHCI: Failed to start controller")
    }

    fn reset(&mut self) -> Result<(), &'static str> {
        // Stop and reset
        let usbcmd = self.read_op_reg(op_regs::USBCMD);
        self.write_op_reg(op_regs::USBCMD, usbcmd & !usbcmd::RUN);
        self.wait_halt()?;
        
        self.write_op_reg(op_regs::USBCMD, usbcmd::HCRST);
        
        for _ in 0..1000 {
            if (self.read_op_reg(op_regs::USBCMD) & usbcmd::HCRST) == 0 {
                self.running.store(false, Ordering::SeqCst);
                return Ok(());
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        
        Err("xHCI: Reset timeout")
    }

    fn port_count(&self) -> u8 {
        self.num_ports
    }

    fn port_connected(&self, port: u8) -> bool {
        if port >= self.num_ports {
            return false;
        }
        let portsc = self.read_port_reg(port, port_regs::PORTSC);
        (portsc & portsc::CCS) != 0
    }

    fn port_speed(&self, port: u8) -> Option<UsbSpeed> {
        if port >= self.num_ports {
            return None;
        }
        let portsc = self.read_port_reg(port, port_regs::PORTSC);
        if (portsc & portsc::CCS) == 0 {
            return None;
        }
        
        let speed = (portsc & portsc::SPEED_MASK) >> 10;
        Some(match speed {
            1 => UsbSpeed::Full,
            2 => UsbSpeed::Low,
            3 => UsbSpeed::High,
            4 => UsbSpeed::Super,
            5 => UsbSpeed::SuperPlus,
            _ => UsbSpeed::Full,
        })
    }

    fn port_reset(&mut self, port: u8) -> Result<(), &'static str> {
        if port >= self.num_ports {
            return Err("Invalid port");
        }
        
        let portsc = self.read_port_reg(port, port_regs::PORTSC);
        // Clear status bits, set reset
        let new_portsc = (portsc & !0x00FE0000) | portsc::PR;
        self.write_port_reg(port, port_regs::PORTSC, new_portsc);
        
        // Wait for reset to complete
        for _ in 0..1000 {
            let portsc = self.read_port_reg(port, port_regs::PORTSC);
            if (portsc & portsc::PRC) != 0 {
                // Clear the change bit
                self.write_port_reg(port, port_regs::PORTSC, portsc | portsc::PRC);
                return Ok(());
            }
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        
        Err("Port reset timeout")
    }

    fn port_enable(&mut self, port: u8) -> Result<(), &'static str> {
        if port >= self.num_ports {
            return Err("Invalid port");
        }
        // Port is automatically enabled after successful reset
        Ok(())
    }

    fn port_disable(&mut self, port: u8) -> Result<(), &'static str> {
        if port >= self.num_ports {
            return Err("Invalid port");
        }
        
        let portsc = self.read_port_reg(port, port_regs::PORTSC);
        // Clear PED to disable
        self.write_port_reg(port, port_regs::PORTSC, portsc & !portsc::PED);
        Ok(())
    }

    fn control_transfer(
        &mut self,
        device: u8,
        setup: SetupPacket,
        data: Option<&mut [u8]>,
    ) -> TransferResult {
        self.control_transfer_slot(device, setup, data)
    }

    fn bulk_transfer(
        &mut self,
        _device: u8,
        _endpoint: u8,
        _data: &mut [u8],
        _direction: Direction,
    ) -> TransferResult {
        // Stub implementation
        TransferResult::Success(0)
    }

    fn interrupt_transfer(
        &mut self,
        _device: u8,
        _endpoint: u8,
        _data: &mut [u8],
        _direction: Direction,
    ) -> TransferResult {
        // Stub implementation
        TransferResult::Success(0)
    }

    fn allocate_address(&mut self) -> Option<u8> {
        for i in 1..=127u8 {
            let byte_idx = (i / 8) as usize;
            let bit_idx = i % 8;
            if (self.address_bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                self.address_bitmap[byte_idx] |= 1 << bit_idx;
                return Some(i);
            }
        }
        None
    }

    fn free_address(&mut self, address: u8) {
        if address >= 1 && address <= 127 {
            let byte_idx = (address / 8) as usize;
            let bit_idx = address % 8;
            self.address_bitmap[byte_idx] &= !(1 << bit_idx);
        }
    }
}

/// Probe for xHCI controllers via PCI
/// 
/// Scans the PCI bus for USB 3.0 xHCI controllers and returns the first one found.
/// xHCI controllers are identified by:
/// - Class: 0x0C (Serial Bus Controller)
/// - Subclass: 0x03 (USB Controller)  
/// - Programming Interface: 0x30 (xHCI)
pub fn probe_xhci() -> Option<Box<dyn UsbHostController>> {
    use crate::pci::{self, class, serial_subclass};
    
    crate::serial_println!("[xhci] Probing for xHCI controllers...");
    
    // Find all USB controllers (class 0x0C, subclass 0x03)
    let usb_controllers = pci::pci().find_by_class(class::SERIAL_BUS, serial_subclass::USB);
    
    for device in usb_controllers {
        // Check if this is an xHCI controller (prog_if = 0x30)
        if device.prog_if == XHCI_PROG_IF {
            crate::serial_println!(
                "[xhci] Found xHCI controller at {} - {:04x}:{:04x}",
                device.address,
                device.vendor_id,
                device.device_id
            );
            
            // Get the MMIO base address from BAR0
            // xHCI uses a single 64-bit or 32-bit memory BAR
            if let Some(bar) = device.bars.first() {
                let base_address = bar.address as usize;
                let size = bar.size as usize;
                
                crate::serial_println!(
                    "[xhci] BAR0: base=0x{:016x}, size=0x{:x}, type={:?}",
                    base_address,
                    size,
                    bar.bar_type
                );
                
                // Validate the BAR
                if base_address == 0 {
                    crate::serial_println!("[xhci] Invalid BAR0 address, skipping");
                    continue;
                }
                
                // Enable bus mastering and memory space access
                device.enable_bus_master();
                device.enable_memory();
                
                // Disable legacy interrupts (we'll use MSI/MSI-X if available)
                device.disable_interrupts();
                
                // Try to enable MSI-X or MSI for better interrupt handling
                if let Some(ref msix) = device.msix {
                    crate::serial_println!(
                        "[xhci] MSI-X supported with {} vectors",
                        msix.table_size
                    );
                    // MSI-X setup would go here
                } else if let Some(ref msi) = device.msi {
                    crate::serial_println!(
                        "[xhci] MSI supported with up to {} vectors",
                        msi.max_vectors
                    );
                    // MSI setup would go here
                }
                
                // Create the xHCI controller instance
                let mut controller = XhciController::new(base_address);
                
                // Initialize the controller (reads capability registers, resets, etc.)
                match controller.init() {
                    Ok(()) => {
                        crate::serial_println!("[xhci] Controller initialized successfully");
                        
                        // Log controller capabilities
                        crate::serial_println!(
                            "[xhci] Ports: {}, Device Slots: {}",
                            controller.num_ports,
                            controller.num_slots
                        );
                        
                        return Some(Box::new(controller));
                    }
                    Err(e) => {
                        crate::serial_println!("[xhci] Failed to initialize controller: {}", e);
                        continue;
                    }
                }
            } else {
                crate::serial_println!("[xhci] No BAR found for controller, skipping");
            }
        }
    }
    
    crate::serial_println!("[xhci] No xHCI controllers found");
    None
}

/// Probe for all xHCI controllers on the system
/// 
/// Returns a vector of all xHCI controllers found on the PCI bus.
pub fn probe_all_xhci() -> alloc::vec::Vec<Box<dyn UsbHostController>> {
    use crate::pci::{self, class, serial_subclass};
    
    let mut controllers: alloc::vec::Vec<Box<dyn UsbHostController>> = alloc::vec::Vec::new();
    
    crate::serial_println!("[xhci] Probing for all xHCI controllers...");
    
    // Find all USB controllers (class 0x0C, subclass 0x03)
    let usb_controllers = pci::pci().find_by_class(class::SERIAL_BUS, serial_subclass::USB);
    
    for device in usb_controllers {
        // Check if this is an xHCI controller (prog_if = 0x30)
        if device.prog_if == XHCI_PROG_IF {
            crate::serial_println!(
                "[xhci] Found xHCI controller at {} - {:04x}:{:04x}",
                device.address,
                device.vendor_id,
                device.device_id
            );
            
            if let Some(bar) = device.bars.first() {
                let base_address = bar.address as usize;
                
                if base_address == 0 {
                    continue;
                }
                
                device.enable_bus_master();
                device.enable_memory();
                device.disable_interrupts();
                
                let mut controller = XhciController::new(base_address);
                match controller.init() {
                    Ok(()) => {
                        crate::serial_println!(
                            "[xhci] Initialized controller at {} with {} ports",
                            device.address,
                            controller.num_ports
                        );
                        controllers.push(Box::new(controller));
                    }
                    Err(e) => {
                        crate::serial_println!(
                            "[xhci] Failed to initialize controller at {}: {}",
                            device.address,
                            e
                        );
                    }
                }
            }
        }
    }
    
    crate::serial_println!("[xhci] Found {} xHCI controller(s)", controllers.len());
    controllers
}
