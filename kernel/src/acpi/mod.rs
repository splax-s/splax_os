//! # ACPI Power Management
//!
//! Advanced Configuration and Power Interface (ACPI) support for Splax OS.
//!
//! ## Features
//!
//! - ACPI table parsing (RSDP, RSDT, XSDT, FADT, MADT)
//! - Power state management (S0-S5)
//! - CPU power states (C-states, P-states)
//! - Thermal management
//! - Battery status
//! - Hardware shutdown/reboot

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;
use spin::RwLock;

/// ACPI signature for RSDP.
const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

/// ACPI signature for RSDT.
const RSDT_SIGNATURE: [u8; 4] = *b"RSDT";

/// ACPI signature for XSDT.
const XSDT_SIGNATURE: [u8; 4] = *b"XSDT";

/// ACPI signature for FADT.
const FADT_SIGNATURE: [u8; 4] = *b"FACP";

/// ACPI signature for MADT.
const MADT_SIGNATURE: [u8; 4] = *b"APIC";

/// ACPI signature for DSDT.
const DSDT_SIGNATURE: [u8; 4] = *b"DSDT";

/// ACPI signature for HPET.
const HPET_SIGNATURE: [u8; 4] = *b"HPET";

/// ACPI signature for MCFG (PCIe).
const MCFG_SIGNATURE: [u8; 4] = *b"MCFG";

/// Root System Description Pointer (ACPI 1.0).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    /// Signature "RSD PTR "
    pub signature: [u8; 8],
    /// Checksum
    pub checksum: u8,
    /// OEM ID
    pub oem_id: [u8; 6],
    /// Revision (0 = ACPI 1.0, 2 = ACPI 2.0+)
    pub revision: u8,
    /// Physical address of RSDT
    pub rsdt_address: u32,
}

/// Extended RSDP (ACPI 2.0+).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct RsdpExtended {
    /// ACPI 1.0 portion
    pub rsdp: Rsdp,
    /// Length of table
    pub length: u32,
    /// Physical address of XSDT
    pub xsdt_address: u64,
    /// Extended checksum
    pub extended_checksum: u8,
    /// Reserved
    pub reserved: [u8; 3],
}

/// Common ACPI table header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiHeader {
    /// Signature (4 bytes)
    pub signature: [u8; 4],
    /// Length of table including header
    pub length: u32,
    /// Revision
    pub revision: u8,
    /// Checksum
    pub checksum: u8,
    /// OEM ID
    pub oem_id: [u8; 6],
    /// OEM table ID
    pub oem_table_id: [u8; 8],
    /// OEM revision
    pub oem_revision: u32,
    /// Creator ID
    pub creator_id: u32,
    /// Creator revision
    pub creator_revision: u32,
}

/// Fixed ACPI Description Table (FADT).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    /// Header
    pub header: AcpiHeader,
    /// Physical address of FACS
    pub facs_address: u32,
    /// Physical address of DSDT
    pub dsdt_address: u32,
    /// Reserved (ACPI 1.0 model)
    pub reserved1: u8,
    /// Preferred PM profile
    pub preferred_pm_profile: u8,
    /// SCI interrupt
    pub sci_interrupt: u16,
    /// SMI command port
    pub smi_command: u32,
    /// ACPI enable value
    pub acpi_enable: u8,
    /// ACPI disable value
    pub acpi_disable: u8,
    /// S4BIOS request value
    pub s4bios_req: u8,
    /// P-state control value
    pub pstate_control: u8,
    /// PM1a event block address
    pub pm1a_event_block: u32,
    /// PM1b event block address
    pub pm1b_event_block: u32,
    /// PM1a control block address
    pub pm1a_control_block: u32,
    /// PM1b control block address
    pub pm1b_control_block: u32,
    /// PM2 control block address
    pub pm2_control_block: u32,
    /// PM timer block address
    pub pm_timer_block: u32,
    /// GPE0 block address
    pub gpe0_block: u32,
    /// GPE1 block address
    pub gpe1_block: u32,
    /// PM1 event length
    pub pm1_event_length: u8,
    /// PM1 control length
    pub pm1_control_length: u8,
    /// PM2 control length
    pub pm2_control_length: u8,
    /// PM timer length
    pub pm_timer_length: u8,
    /// GPE0 block length
    pub gpe0_length: u8,
    /// GPE1 block length
    pub gpe1_length: u8,
    /// GPE1 base
    pub gpe1_base: u8,
    /// C-state control
    pub cstate_control: u8,
    /// Worst C2 latency (µs)
    pub worst_c2_latency: u16,
    /// Worst C3 latency (µs)
    pub worst_c3_latency: u16,
    /// Flush size
    pub flush_size: u16,
    /// Flush stride
    pub flush_stride: u16,
    /// Duty offset
    pub duty_offset: u8,
    /// Duty width
    pub duty_width: u8,
    /// RTC day alarm index
    pub day_alarm: u8,
    /// RTC month alarm index
    pub month_alarm: u8,
    /// RTC century index
    pub century: u8,
    /// Boot architecture flags (ACPI 2.0+)
    pub boot_architecture_flags: u16,
    /// Reserved
    pub reserved2: u8,
    /// Flags
    pub flags: u32,
    // ACPI 2.0+ fields follow (Generic Address Structures)
}

/// FADT flags.
pub mod fadt_flags {
    pub const WBINVD: u32 = 1 << 0;
    pub const WBINVD_FLUSH: u32 = 1 << 1;
    pub const PROC_C1: u32 = 1 << 2;
    pub const P_LVL2_UP: u32 = 1 << 3;
    pub const PWR_BUTTON: u32 = 1 << 4;
    pub const SLP_BUTTON: u32 = 1 << 5;
    pub const FIX_RTC: u32 = 1 << 6;
    pub const RTC_S4: u32 = 1 << 7;
    pub const TMR_VAL_EXT: u32 = 1 << 8;
    pub const DCK_CAP: u32 = 1 << 9;
    pub const RESET_REG_SUP: u32 = 1 << 10;
    pub const SEALED_CASE: u32 = 1 << 11;
    pub const HEADLESS: u32 = 1 << 12;
    pub const CPU_SW_SLP: u32 = 1 << 13;
    pub const PCI_EXP_WAK: u32 = 1 << 14;
    pub const USE_PLATFORM_CLOCK: u32 = 1 << 15;
    pub const S4_RTC_STS_VALID: u32 = 1 << 16;
    pub const REMOTE_POWER_ON_CAP: u32 = 1 << 17;
    pub const FORCE_APIC_CLUSTER: u32 = 1 << 18;
    pub const FORCE_APIC_PHYS: u32 = 1 << 19;
    pub const HW_REDUCED_ACPI: u32 = 1 << 20;
    pub const LOW_POWER_S0_IDLE: u32 = 1 << 21;
}

/// Multiple APIC Description Table (MADT).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Madt {
    /// Header
    pub header: AcpiHeader,
    /// Local APIC address
    pub local_apic_address: u32,
    /// Flags
    pub flags: u32,
}

/// MADT entry header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtEntryHeader {
    /// Entry type
    pub entry_type: u8,
    /// Entry length
    pub length: u8,
}

/// MADT entry types.
pub mod madt_entry {
    pub const LOCAL_APIC: u8 = 0;
    pub const IO_APIC: u8 = 1;
    pub const INTERRUPT_OVERRIDE: u8 = 2;
    pub const NMI_SOURCE: u8 = 3;
    pub const LOCAL_APIC_NMI: u8 = 4;
    pub const LOCAL_APIC_OVERRIDE: u8 = 5;
    pub const IO_SAPIC: u8 = 6;
    pub const LOCAL_SAPIC: u8 = 7;
    pub const PLATFORM_INTERRUPT: u8 = 8;
    pub const LOCAL_X2APIC: u8 = 9;
    pub const LOCAL_X2APIC_NMI: u8 = 10;
    pub const GIC_CPU: u8 = 11;
    pub const GIC_DISTRIBUTOR: u8 = 12;
}

/// Local APIC entry.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtLocalApic {
    /// Header
    pub header: MadtEntryHeader,
    /// Processor ID
    pub acpi_processor_id: u8,
    /// APIC ID
    pub apic_id: u8,
    /// Flags (bit 0 = enabled)
    pub flags: u32,
}

/// I/O APIC entry.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIoApic {
    /// Header
    pub header: MadtEntryHeader,
    /// I/O APIC ID
    pub io_apic_id: u8,
    /// Reserved
    pub reserved: u8,
    /// I/O APIC address
    pub io_apic_address: u32,
    /// Global system interrupt base
    pub gsi_base: u32,
}

/// Interrupt source override entry.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtInterruptOverride {
    /// Header
    pub header: MadtEntryHeader,
    /// Bus (always 0 = ISA)
    pub bus: u8,
    /// Source IRQ
    pub source: u8,
    /// Global system interrupt
    pub gsi: u32,
    /// Flags
    pub flags: u16,
}

/// System power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// S0: Working state
    S0Working,
    /// S1: CPU stops, context maintained
    S1Standby,
    /// S2: CPU off, cache flushed
    S2Sleep,
    /// S3: Suspend to RAM
    S3SuspendToRam,
    /// S4: Suspend to disk (hibernate)
    S4Hibernate,
    /// S5: Soft off
    S5SoftOff,
}

impl PowerState {
    /// Returns the SLP_TYP value for this state.
    pub fn slp_typ(&self) -> u8 {
        match self {
            PowerState::S0Working => 0,
            PowerState::S1Standby => 1,
            PowerState::S2Sleep => 2,
            PowerState::S3SuspendToRam => 3,
            PowerState::S4Hibernate => 4,
            PowerState::S5SoftOff => 5,
        }
    }
}

/// CPU power state (C-state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CState {
    /// C0: Active
    C0Active,
    /// C1: Halt
    C1Halt,
    /// C2: Stop clock
    C2StopClock,
    /// C3: Deep sleep
    C3DeepSleep,
}

/// Processor information from MADT.
#[derive(Debug, Clone)]
pub struct ProcessorInfo {
    /// ACPI processor ID
    pub acpi_id: u8,
    /// Local APIC ID
    pub apic_id: u8,
    /// Is enabled
    pub enabled: bool,
    /// Is bootstrap processor
    pub is_bsp: bool,
}

/// I/O APIC information from MADT.
#[derive(Debug, Clone)]
pub struct IoApicInfo {
    /// I/O APIC ID
    pub id: u8,
    /// Physical address
    pub address: u32,
    /// Global system interrupt base
    pub gsi_base: u32,
}

/// Interrupt override information.
#[derive(Debug, Clone)]
pub struct InterruptOverride {
    /// Source IRQ
    pub source: u8,
    /// Global system interrupt
    pub gsi: u32,
    /// Is active low
    pub active_low: bool,
    /// Is level triggered
    pub level_triggered: bool,
}

/// Battery status.
#[derive(Debug, Clone)]
pub struct BatteryStatus {
    /// Is battery present
    pub present: bool,
    /// Is charging
    pub charging: bool,
    /// Is discharging
    pub discharging: bool,
    /// Charge percentage (0-100)
    pub percentage: u8,
    /// Remaining capacity (mWh)
    pub remaining_capacity: u32,
    /// Full capacity (mWh)
    pub full_capacity: u32,
    /// Design capacity (mWh)
    pub design_capacity: u32,
    /// Voltage (mV)
    pub voltage: u32,
    /// Current rate (mW)
    pub rate: i32,
    /// Time remaining (minutes, -1 if unknown)
    pub time_remaining: i32,
}

impl Default for BatteryStatus {
    fn default() -> Self {
        Self {
            present: false,
            charging: false,
            discharging: false,
            percentage: 0,
            remaining_capacity: 0,
            full_capacity: 0,
            design_capacity: 0,
            voltage: 0,
            rate: 0,
            time_remaining: -1,
        }
    }
}

/// Thermal zone information.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    /// Zone name
    pub name: String,
    /// Current temperature (deci-Kelvin)
    pub temperature: u32,
    /// Critical temperature (deci-Kelvin)
    pub critical: u32,
    /// Passive cooling threshold (deci-Kelvin)
    pub passive: u32,
    /// Active cooling thresholds (deci-Kelvin)
    pub active: Vec<u32>,
}

impl ThermalZone {
    /// Returns temperature in Celsius.
    pub fn celsius(&self) -> i32 {
        (self.temperature as i32 - 2732) / 10
    }
    
    /// Returns critical temperature in Celsius.
    pub fn critical_celsius(&self) -> i32 {
        (self.critical as i32 - 2732) / 10
    }
}

/// ACPI subsystem.
pub struct AcpiSubsystem {
    /// Is initialized
    initialized: spin::Mutex<bool>,
    /// ACPI revision
    revision: spin::Mutex<u8>,
    /// FADT address
    fadt: RwLock<Option<Fadt>>,
    /// Processors
    processors: RwLock<Vec<ProcessorInfo>>,
    /// I/O APICs
    io_apics: RwLock<Vec<IoApicInfo>>,
    /// Interrupt overrides
    interrupt_overrides: RwLock<Vec<InterruptOverride>>,
    /// PM1a control block
    pm1a_control: spin::Mutex<u16>,
    /// PM1b control block
    pm1b_control: spin::Mutex<u16>,
    /// SLP_TYPa values for each S-state
    slp_typa: spin::Mutex<[u8; 6]>,
    /// SLP_TYPb values for each S-state
    slp_typb: spin::Mutex<[u8; 6]>,
}

impl AcpiSubsystem {
    /// Creates a new ACPI subsystem.
    pub const fn new() -> Self {
        Self {
            initialized: spin::Mutex::new(false),
            revision: spin::Mutex::new(0),
            fadt: RwLock::new(None),
            processors: RwLock::new(Vec::new()),
            io_apics: RwLock::new(Vec::new()),
            interrupt_overrides: RwLock::new(Vec::new()),
            pm1a_control: spin::Mutex::new(0),
            pm1b_control: spin::Mutex::new(0),
            slp_typa: spin::Mutex::new([0; 6]),
            slp_typb: spin::Mutex::new([0; 6]),
        }
    }
    
    /// Searches for RSDP in BIOS memory regions.
    fn find_rsdp(&self) -> Option<*const Rsdp> {
        // Search EBDA (Extended BIOS Data Area)
        let ebda_ptr = unsafe { *(0x40E as *const u16) as usize } << 4;
        if ebda_ptr != 0 {
            if let Some(rsdp) = self.search_rsdp(ebda_ptr, 1024) {
                return Some(rsdp);
            }
        }
        
        // Search main BIOS area (0xE0000 - 0xFFFFF)
        self.search_rsdp(0xE0000, 0x20000)
    }
    
    /// Searches for RSDP signature in a memory region.
    fn search_rsdp(&self, start: usize, length: usize) -> Option<*const Rsdp> {
        let mut addr = start;
        let end = start + length;
        
        while addr < end {
            let sig = unsafe { core::slice::from_raw_parts(addr as *const u8, 8) };
            if sig == RSDP_SIGNATURE {
                let rsdp = addr as *const Rsdp;
                if self.validate_rsdp(rsdp) {
                    return Some(rsdp);
                }
            }
            addr += 16; // RSDP is 16-byte aligned
        }
        
        None
    }
    
    /// Validates RSDP checksum.
    fn validate_rsdp(&self, rsdp: *const Rsdp) -> bool {
        let bytes = unsafe { core::slice::from_raw_parts(rsdp as *const u8, 20) };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        sum == 0
    }
    
    /// Validates a table checksum.
    fn validate_table(&self, header: *const AcpiHeader) -> bool {
        let length = unsafe { (*header).length } as usize;
        let bytes = unsafe { core::slice::from_raw_parts(header as *const u8, length) };
        let sum: u8 = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        sum == 0
    }
    
    /// Initializes the ACPI subsystem.
    pub fn init(&self) -> bool {
        if *self.initialized.lock() {
            return true;
        }
        
        // Find RSDP
        let rsdp_ptr = match self.find_rsdp() {
            Some(ptr) => ptr,
            None => {
                crate::serial_println!("[acpi] RSDP not found");
                return false;
            }
        };
        
        let rsdp = unsafe { *rsdp_ptr };
        *self.revision.lock() = rsdp.revision;
        
        crate::serial_println!(
            "[acpi] Found RSDP revision {} at {:#x}",
            rsdp.revision,
            rsdp_ptr as usize
        );
        
        // Parse RSDT or XSDT
        if rsdp.revision >= 2 {
            let xrsdp = unsafe { *(rsdp_ptr as *const RsdpExtended) };
            self.parse_xsdt(xrsdp.xsdt_address as usize);
        } else {
            self.parse_rsdt(rsdp.rsdt_address as usize);
        }
        
        *self.initialized.lock() = true;
        true
    }
    
    /// Parses the RSDT.
    fn parse_rsdt(&self, addr: usize) {
        let header = addr as *const AcpiHeader;
        if !self.validate_table(header) {
            crate::serial_println!("[acpi] Invalid RSDT checksum");
            return;
        }
        
        let length = unsafe { (*header).length } as usize;
        let entry_count = (length - core::mem::size_of::<AcpiHeader>()) / 4;
        let entries = (addr + core::mem::size_of::<AcpiHeader>()) as *const u32;
        
        crate::serial_println!("[acpi] RSDT has {} entries", entry_count);
        
        for i in 0..entry_count {
            let table_addr = unsafe { *entries.add(i) } as usize;
            self.parse_table(table_addr);
        }
    }
    
    /// Parses the XSDT.
    fn parse_xsdt(&self, addr: usize) {
        let header = addr as *const AcpiHeader;
        if !self.validate_table(header) {
            crate::serial_println!("[acpi] Invalid XSDT checksum");
            return;
        }
        
        let length = unsafe { (*header).length } as usize;
        let entry_count = (length - core::mem::size_of::<AcpiHeader>()) / 8;
        let entries = (addr + core::mem::size_of::<AcpiHeader>()) as *const u64;
        
        crate::serial_println!("[acpi] XSDT has {} entries", entry_count);
        
        for i in 0..entry_count {
            let table_addr = unsafe { *entries.add(i) } as usize;
            self.parse_table(table_addr);
        }
    }
    
    /// Parses an individual ACPI table.
    fn parse_table(&self, addr: usize) {
        let header = unsafe { *(addr as *const AcpiHeader) };
        let sig = header.signature;
        
        match sig {
            FADT_SIGNATURE => {
                crate::serial_println!("[acpi] Parsing FADT");
                self.parse_fadt(addr);
            }
            MADT_SIGNATURE => {
                crate::serial_println!("[acpi] Parsing MADT");
                self.parse_madt(addr);
            }
            HPET_SIGNATURE => {
                crate::serial_println!("[acpi] Found HPET table");
            }
            MCFG_SIGNATURE => {
                crate::serial_println!("[acpi] Found MCFG (PCIe) table");
            }
            _ => {
                let sig_str = core::str::from_utf8(&sig).unwrap_or("????");
                crate::serial_println!("[acpi] Found table: {}", sig_str);
            }
        }
    }
    
    /// Parses the FADT.
    fn parse_fadt(&self, addr: usize) {
        let fadt = unsafe { *(addr as *const Fadt) };
        
        // Copy fields from packed struct to avoid alignment issues
        let pm1a = fadt.pm1a_control_block;
        let pm1b = fadt.pm1b_control_block;
        let sci = fadt.sci_interrupt;
        let smi = fadt.smi_command;
        let flags = fadt.flags;
        
        *self.pm1a_control.lock() = pm1a as u16;
        *self.pm1b_control.lock() = pm1b as u16;
        
        crate::serial_println!(
            "[acpi] FADT: PM1a={:#x}, PM1b={:#x}, SCI={}, SMI={:#x}",
            pm1a, pm1b, sci, smi
        );
        
        if (flags & fadt_flags::RESET_REG_SUP) != 0 {
            crate::serial_println!("[acpi] FADT: Reset register supported");
        }
        
        if (flags & fadt_flags::HW_REDUCED_ACPI) != 0 {
            crate::serial_println!("[acpi] FADT: Hardware-reduced ACPI");
        }
        
        *self.fadt.write() = Some(fadt);
    }
    
    /// Parses the MADT.
    fn parse_madt(&self, addr: usize) {
        let madt = unsafe { *(addr as *const Madt) };
        let length = { madt.header.length } as usize;
        
        // Copy fields from packed struct
        let local_apic_addr = madt.local_apic_address;
        let madt_flags = madt.flags;
        
        crate::serial_println!(
            "[acpi] MADT: Local APIC at {:#x}, flags={:#x}",
            local_apic_addr, madt_flags
        );
        
        let mut offset = core::mem::size_of::<Madt>();
        let mut processors = Vec::new();
        let mut io_apics = Vec::new();
        let mut overrides = Vec::new();
        
        while offset < length {
            let entry_ptr = (addr + offset) as *const MadtEntryHeader;
            let entry = unsafe { *entry_ptr };
            
            match entry.entry_type {
                madt_entry::LOCAL_APIC => {
                    let lapic = unsafe { *(entry_ptr as *const MadtLocalApic) };
                    let enabled = (lapic.flags & 1) != 0;
                    processors.push(ProcessorInfo {
                        acpi_id: lapic.acpi_processor_id,
                        apic_id: lapic.apic_id,
                        enabled,
                        is_bsp: processors.is_empty(),
                    });
                    crate::serial_println!(
                        "[acpi] MADT: Local APIC {} (ACPI ID {}), enabled={}",
                        lapic.apic_id,
                        lapic.acpi_processor_id,
                        enabled
                    );
                }
                madt_entry::IO_APIC => {
                    let ioapic = unsafe { *(entry_ptr as *const MadtIoApic) };
                    // Copy fields from packed struct
                    let id = ioapic.io_apic_id;
                    let address = { ioapic.io_apic_address };
                    let gsi = { ioapic.gsi_base };
                    io_apics.push(IoApicInfo {
                        id,
                        address,
                        gsi_base: gsi,
                    });
                    crate::serial_println!(
                        "[acpi] MADT: I/O APIC {} at {:#x}, GSI base {}",
                        id, address, gsi
                    );
                }
                madt_entry::INTERRUPT_OVERRIDE => {
                    let over = unsafe { *(entry_ptr as *const MadtInterruptOverride) };
                    let flags = { over.flags };
                    let source_irq = over.source;
                    let gsi_num = { over.gsi };
                    let active_low = (flags & 0x02) != 0;
                    let level_triggered = (flags & 0x08) != 0;
                    overrides.push(InterruptOverride {
                        source: source_irq,
                        gsi: gsi_num,
                        active_low,
                        level_triggered,
                    });
                    crate::serial_println!(
                        "[acpi] MADT: IRQ {} -> GSI {}",
                        source_irq, gsi_num
                    );
                }
                _ => {}
            }
            
            offset += entry.length as usize;
        }
        
        *self.processors.write() = processors;
        *self.io_apics.write() = io_apics;
        *self.interrupt_overrides.write() = overrides;
    }
    
    /// Returns the list of processors.
    pub fn processors(&self) -> Vec<ProcessorInfo> {
        self.processors.read().clone()
    }
    
    /// Returns the number of enabled processors.
    pub fn processor_count(&self) -> usize {
        self.processors.read().iter().filter(|p| p.enabled).count()
    }
    
    /// Returns the list of I/O APICs.
    pub fn io_apics(&self) -> Vec<IoApicInfo> {
        self.io_apics.read().clone()
    }
    
    /// Returns the interrupt override for an IRQ.
    pub fn get_irq_override(&self, irq: u8) -> Option<InterruptOverride> {
        self.interrupt_overrides.read()
            .iter()
            .find(|o| o.source == irq)
            .cloned()
    }
    
    /// Enters a power state.
    pub fn enter_power_state(&self, state: PowerState) {
        if !*self.initialized.lock() {
            return;
        }
        
        let pm1a = *self.pm1a_control.lock();
        let pm1b = *self.pm1b_control.lock();
        
        if pm1a == 0 {
            crate::serial_println!("[acpi] No PM1a control block");
            return;
        }
        
        // Get SLP_TYP values (normally from DSDT parsing)
        // For now, use common default values
        let slp_typ = match state {
            PowerState::S0Working => 0,
            PowerState::S1Standby => 1,
            PowerState::S2Sleep => 2,
            PowerState::S3SuspendToRam => 1, // Often same as S1
            PowerState::S4Hibernate => 2,
            PowerState::S5SoftOff => {
                // Try common S5 values
                // QEMU typically uses SLP_TYP = 0
                0
            }
        };
        
        crate::serial_println!("[acpi] Entering power state {:?}", state);
        
        // Write to PM1a control block
        // SLP_TYP is bits 10-12, SLP_EN is bit 13
        let value = (slp_typ as u16) << 10 | (1 << 13);
        
        unsafe {
            core::arch::asm!(
                "out dx, ax",
                in("dx") pm1a,
                in("ax") value,
                options(nomem, nostack)
            );
        }
        
        // Write to PM1b if present
        if pm1b != 0 {
            unsafe {
                core::arch::asm!(
                    "out dx, ax",
                    in("dx") pm1b,
                    in("ax") value,
                    options(nomem, nostack)
                );
            }
        }
        
        // If we're still here, the state transition didn't work
        // This is expected for states other than S5 sometimes
    }
    
    /// Performs an ACPI shutdown.
    pub fn shutdown(&self) -> ! {
        self.enter_power_state(PowerState::S5SoftOff);
        
        // Fallback: use x86 power module
        crate::arch::x86_64::power::shutdown()
    }
    
    /// Performs an ACPI reboot.
    pub fn reboot(&self) -> ! {
        // Check if reset register is supported
        if let Some(fadt) = self.fadt.read().as_ref() {
            if (fadt.flags & fadt_flags::RESET_REG_SUP) != 0 {
                // Would need to parse Generic Address Structure for reset register
                // For now, fall through to x86 power module
            }
        }
        
        // Fallback: use x86 power module
        crate::arch::x86_64::power::reboot()
    }
    
    /// Checks if ACPI is initialized.
    pub fn is_initialized(&self) -> bool {
        *self.initialized.lock()
    }
    
    /// Returns the ACPI revision.
    pub fn revision(&self) -> u8 {
        *self.revision.lock()
    }
}

/// Global ACPI subsystem.
static ACPI: spin::Once<AcpiSubsystem> = spin::Once::new();

/// Gets the global ACPI subsystem.
pub fn acpi() -> &'static AcpiSubsystem {
    ACPI.call_once(|| AcpiSubsystem::new())
}

/// Initializes the ACPI subsystem.
pub fn init() -> bool {
    let result = acpi().init();
    if result {
        let cpu_count = acpi().processor_count();
        crate::serial_println!("[acpi] Initialized, {} processor(s) found", cpu_count);
    }
    result
}

/// Performs an ACPI shutdown.
pub fn shutdown() -> ! {
    acpi().shutdown()
}

/// Performs an ACPI reboot.
pub fn reboot() -> ! {
    acpi().reboot()
}
