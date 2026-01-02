# ACPI Subsystem Documentation

## Overview

The ACPI (Advanced Configuration and Power Interface) subsystem parses system firmware tables to discover hardware configuration, power management capabilities, and platform topology.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                        ACPI Subsystem                               │
├────────────────────────────────────────────────────────────────────┤
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────────┐   │
│  │  Table Parser  │  │  AML           │  │  Power            │   │
│  │                │  │  Interpreter   │  │  Management       │   │
│  │  - RSDP        │  │                │  │                   │   │
│  │  - RSDT/XSDT   │  │  - Namespace   │  │  - Sleep states   │   │
│  │  - FADT        │  │  - Methods     │  │  - CPU P-states   │   │
│  │  - MADT        │  │  - Objects     │  │  - Device power   │   │
│  │  - DSDT/SSDT   │  │                │  │                   │   │
│  └────────────────┘  └────────────────┘  └────────────────────┘   │
├────────────────────────────────────────────────────────────────────┤
│                    Platform Hardware                                │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  PM1a/PM1b Event Blocks  │  GPE Blocks  │  Fixed Hardware    │  │
│  └──────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
```

## Table Discovery

### RSDP (Root System Description Pointer)

The RSDP is the entry point to ACPI tables, located in:
- EBDA (Extended BIOS Data Area)
- BIOS ROM area (0xE0000-0xFFFFF)
- EFI System Table

```rust
#[repr(C, packed)]
pub struct Rsdp {
    pub signature: [u8; 8],    // "RSD PTR "
    pub checksum: u8,          // Checksum of first 20 bytes
    pub oem_id: [u8; 6],       // OEM identifier
    pub revision: u8,          // 0 = ACPI 1.0, 2 = ACPI 2.0+
    pub rsdt_address: u32,     // Physical address of RSDT
    // ACPI 2.0+ fields
    pub length: u32,           // Length of table
    pub xsdt_address: u64,     // Physical address of XSDT
    pub extended_checksum: u8, // Checksum of entire table
    pub reserved: [u8; 3],
}

impl Rsdp {
    pub fn validate(&self) -> bool {
        // Check signature
        if &self.signature != b"RSD PTR " {
            return false;
        }
        
        // Verify checksum
        let bytes = unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, 20)
        };
        let sum: u8 = bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        sum == 0
    }
}
```

### RSDT/XSDT (Root/Extended System Description Table)

Contains pointers to other ACPI tables:

```rust
#[repr(C, packed)]
pub struct SdtHeader {
    pub signature: [u8; 4],    // Table signature (e.g., "FACP")
    pub length: u32,           // Total table length
    pub revision: u8,          // Table revision
    pub checksum: u8,          // Checksum of entire table
    pub oem_id: [u8; 6],       // OEM identifier
    pub oem_table_id: [u8; 8], // OEM table identifier
    pub oem_revision: u32,     // OEM revision
    pub creator_id: u32,       // ASL compiler vendor ID
    pub creator_revision: u32, // ASL compiler revision
}

pub fn find_table(signature: &[u8; 4]) -> Option<&'static SdtHeader> {
    let xsdt = get_xsdt();
    let entries = (xsdt.header.length as usize - size_of::<SdtHeader>()) / 8;
    
    for i in 0..entries {
        let addr = unsafe { *xsdt.entries.add(i) };
        let header = unsafe { &*(addr as *const SdtHeader) };
        
        if &header.signature == signature {
            return Some(header);
        }
    }
    None
}
```

## Key Tables

### FADT (Fixed ACPI Description Table)

Contains system power management information:

```rust
#[repr(C, packed)]
pub struct Fadt {
    pub header: SdtHeader,
    pub firmware_ctrl: u32,      // Physical address of FACS
    pub dsdt: u32,               // Physical address of DSDT
    pub reserved1: u8,
    pub preferred_pm_profile: u8, // Power management profile
    pub sci_interrupt: u16,      // System Control Interrupt
    pub smi_command: u32,        // SMI command port
    pub acpi_enable: u8,         // Value to enable ACPI
    pub acpi_disable: u8,        // Value to disable ACPI
    pub s4bios_req: u8,          // S4BIOS_REQ command
    pub pstate_control: u8,      // P-state control
    pub pm1a_event_block: u32,   // PM1a event register block
    pub pm1b_event_block: u32,   // PM1b event register block
    pub pm1a_control_block: u32, // PM1a control register block
    pub pm1b_control_block: u32, // PM1b control register block
    pub pm2_control_block: u32,  // PM2 control register block
    pub pm_timer_block: u32,     // PM timer register block
    pub gpe0_block: u32,         // GPE0 register block
    pub gpe1_block: u32,         // GPE1 register block
    // ... additional fields
    pub flags: u32,              // Feature flags
    pub reset_reg: GenericAddress, // Reset register
    pub reset_value: u8,         // Value to write for reset
    // ACPI 2.0+ extended addresses
    pub x_firmware_ctrl: u64,
    pub x_dsdt: u64,
    // ... extended PM blocks
}
```

### MADT (Multiple APIC Description Table)

Describes interrupt controllers and processors:

```rust
#[repr(C, packed)]
pub struct Madt {
    pub header: SdtHeader,
    pub local_apic_address: u32, // Local APIC address
    pub flags: u32,              // Multiple APIC flags
    // Variable-length entries follow
}
```

**Helper Functions** (`kernel/src/acpi/mod.rs`):

```rust
/// Get total number of enabled CPUs from MADT
pub fn cpu_count() -> usize;

/// Get APIC IDs of all enabled processors
pub fn get_apic_ids() -> Vec<u8>;

/// Get APIC ID of the Bootstrap Processor
pub fn bsp_apic_id() -> Option<u8>;
```

**MADT Entry Types:**

```rust
#[repr(u8)]
pub enum MadtEntryType {
    LocalApic = 0,           // Processor Local APIC
    IoApic = 1,              // I/O APIC
    InterruptOverride = 2,   // Interrupt Source Override
    NmiSource = 3,           // NMI Source
    LocalApicNmi = 4,        // Local APIC NMI
    LocalApicOverride = 5,   // Local APIC Address Override
    IoSapic = 6,             // I/O SAPIC
    LocalSapic = 7,          // Local SAPIC
    PlatformInterrupt = 8,   // Platform Interrupt Sources
    LocalX2Apic = 9,         // Processor Local x2APIC
    LocalX2ApicNmi = 10,     // Local x2APIC NMI
    GicCpu = 11,             // GIC CPU Interface
    GicDist = 12,            // GIC Distributor
    GicMsi = 13,             // GIC MSI Frame
    GicRedist = 14,          // GIC Redistributor
    GicIts = 15,             // GIC ITS
}

#[repr(C, packed)]
pub struct MadtLocalApic {
    pub entry_type: u8,      // 0
    pub length: u8,          // 8
    pub processor_id: u8,    // ACPI processor ID
    pub apic_id: u8,         // Local APIC ID
    pub flags: u32,          // Flags (bit 0 = enabled)
}

#[repr(C, packed)]
pub struct MadtIoApic {
    pub entry_type: u8,      // 1
    pub length: u8,          // 12
    pub io_apic_id: u8,      // I/O APIC ID
    pub reserved: u8,
    pub io_apic_address: u32, // Physical address
    pub gsi_base: u32,       // Global System Interrupt base
}
```

### MCFG (Memory Mapped Configuration)

PCIe enhanced configuration access:

```rust
#[repr(C, packed)]
pub struct Mcfg {
    pub header: SdtHeader,
    pub reserved: u64,
    // Variable-length allocation entries
}

#[repr(C, packed)]
pub struct McfgAllocation {
    pub base_address: u64,   // Base address of ECAM
    pub segment_group: u16,  // PCI segment group
    pub start_bus: u8,       // Start bus number
    pub end_bus: u8,         // End bus number
    pub reserved: u32,
}
```

### HPET (High Precision Event Timer)

```rust
#[repr(C, packed)]
pub struct Hpet {
    pub header: SdtHeader,
    pub event_timer_block_id: u32,
    pub base_address: GenericAddress,
    pub hpet_number: u8,
    pub min_tick: u16,
    pub page_protection: u8,
}
```

## CPU Enumeration

```rust
pub fn enumerate_cpus() -> Vec<CpuInfo> {
    let madt = find_table(b"APIC").unwrap();
    let madt = unsafe { &*(madt as *const _ as *const Madt) };
    
    let mut cpus = Vec::new();
    let mut offset = size_of::<Madt>();
    let end = madt.header.length as usize;
    
    while offset < end {
        let entry_type = unsafe {
            *((madt as *const _ as *const u8).add(offset))
        };
        let entry_len = unsafe {
            *((madt as *const _ as *const u8).add(offset + 1))
        };
        
        match entry_type {
            0 => {
                // Local APIC
                let lapic = unsafe {
                    &*((madt as *const _ as *const u8).add(offset) as *const MadtLocalApic)
                };
                if lapic.flags & 1 != 0 {
                    cpus.push(CpuInfo {
                        processor_id: lapic.processor_id,
                        apic_id: lapic.apic_id,
                        enabled: true,
                    });
                }
            }
            9 => {
                // x2APIC
                // Parse x2APIC entry...
            }
            _ => {}
        }
        
        offset += entry_len as usize;
    }
    
    cpus
}
```

## Power Management

### Sleep States

| State | Name | Description |
|-------|------|-------------|
| S0 | Working | System fully operational |
| S1 | Standby | CPU stops, RAM refreshed |
| S2 | Standby | CPU powered off |
| S3 | Suspend | RAM only powered |
| S4 | Hibernate | State saved to disk |
| S5 | Soft Off | System powered off |

### Entering Sleep State

```rust
pub fn enter_sleep_state(state: u8) -> Result<(), AcpiError> {
    if state > 5 {
        return Err(AcpiError::InvalidState);
    }
    
    let fadt = get_fadt();
    
    // Get sleep type values from DSDT
    let (slp_typa, slp_typb) = get_sleep_type_values(state)?;
    
    // Write sleep type to PM1a/PM1b control registers
    let pm1a = fadt.pm1a_control_block as u16;
    let pm1b = fadt.pm1b_control_block as u16;
    
    // Set SLP_TYP and SLP_EN bits
    let value = (slp_typa << 10) | (1 << 13);
    
    unsafe {
        outw(pm1a, value);
        if pm1b != 0 {
            let value = (slp_typb << 10) | (1 << 13);
            outw(pm1b, value);
        }
    }
    
    Ok(())
}
```

### System Reset

```rust
pub fn system_reset() -> ! {
    let fadt = get_fadt();
    
    if fadt.flags & (1 << 10) != 0 {
        // Reset register supported
        let addr = fadt.reset_reg.address;
        let value = fadt.reset_value;
        
        match fadt.reset_reg.address_space {
            0 => unsafe { // System Memory
                *(addr as *mut u8) = value;
            },
            1 => unsafe { // System I/O
                outb(addr as u16, value);
            },
            _ => {}
        }
    }
    
    // Fallback: keyboard controller reset
    unsafe {
        outb(0x64, 0xFE);
    }
    
    loop { core::hint::spin_loop(); }
}
```

### System Shutdown

```rust
pub fn system_shutdown() -> ! {
    // Enter S5 state
    let _ = enter_sleep_state(5);
    
    // Fallback for QEMU
    unsafe {
        outw(0x604, 0x2000); // QEMU exit
        outw(0xB004, 0x2000); // Bochs exit
    }
    
    loop { core::hint::spin_loop(); }
}
```

## Generic Address Structure

Used throughout ACPI for hardware access:

```rust
#[repr(C, packed)]
pub struct GenericAddress {
    pub address_space: u8,    // Address space ID
    pub bit_width: u8,        // Register bit width
    pub bit_offset: u8,       // Bit offset within register
    pub access_size: u8,      // Access size (1=byte, 2=word, etc.)
    pub address: u64,         // Register address
}

// Address Space IDs
pub const ADDR_SPACE_MEMORY: u8 = 0;
pub const ADDR_SPACE_IO: u8 = 1;
pub const ADDR_SPACE_PCI_CONFIG: u8 = 2;
pub const ADDR_SPACE_EMBEDDED_CONTROLLER: u8 = 3;
pub const ADDR_SPACE_SMBUS: u8 = 4;
pub const ADDR_SPACE_PCC: u8 = 0x0A;
pub const ADDR_SPACE_FFH: u8 = 0x7F;
```

## AML Interpreter (Planned)

ACPI Machine Language interpreter for executing _STA, _INI, etc.:

```rust
pub struct AmlInterpreter {
    namespace: BTreeMap<String, AmlObject>,
    current_scope: String,
}

pub enum AmlObject {
    Integer(u64),
    String(String),
    Buffer(Vec<u8>),
    Package(Vec<AmlObject>),
    Method {
        args: u8,
        serialized: bool,
        code: Vec<u8>,
    },
    Device {
        hid: Option<String>,
        uid: Option<u64>,
        adr: Option<u64>,
    },
    // ...
}
```

## Shell Commands

### acpi

Displays ACPI table information:

```
splax> acpi
ACPI Tables:
  RSDP: 0x000F6A90 (ACPI 2.0)
  XSDT: 0x7FFE0080 (6 entries)
  FACP: 0x7FFE0200 (rev 5, FADT)
  APIC: 0x7FFE0400 (MADT)
  HPET: 0x7FFE0500
  MCFG: 0x7FFE0600
  BGRT: 0x7FFE0700 (Boot Graphics)

splax> acpi -madt
MADT: Local APIC at 0xFEE00000
  CPU 0: APIC ID 0, enabled
  CPU 1: APIC ID 1, enabled
  CPU 2: APIC ID 2, enabled
  CPU 3: APIC ID 3, enabled
  I/O APIC: ID 0, address 0xFEC00000, GSI base 0
```

### poweroff

Uses ACPI to power off the system:

```
splax> poweroff
Entering ACPI S5 state...
```

### reboot

Uses ACPI reset register:

```
splax> reboot
Performing ACPI reset...
```

## Error Handling

```rust
pub enum AcpiError {
    RsdpNotFound,
    InvalidChecksum,
    TableNotFound(&'static str),
    InvalidTable,
    UnsupportedRevision,
    InvalidState,
    PowerManagementFailed,
}
```

## Initialization Sequence

```rust
pub fn init() -> Result<(), AcpiError> {
    // 1. Find RSDP
    let rsdp = find_rsdp()?;
    serial_println!("[ACPI] Found RSDP at {:p}", rsdp);
    
    // 2. Validate and store
    if !rsdp.validate() {
        return Err(AcpiError::InvalidChecksum);
    }
    
    // 3. Get XSDT or RSDT
    let xsdt = if rsdp.revision >= 2 {
        unsafe { &*(rsdp.xsdt_address as *const Xsdt) }
    } else {
        // Use RSDT for ACPI 1.0
        return Err(AcpiError::UnsupportedRevision);
    };
    
    // 4. Parse MADT for CPU/APIC info
    if let Some(madt) = find_table(b"APIC") {
        parse_madt(madt);
    }
    
    // 5. Parse FADT for power management
    if let Some(fadt) = find_table(b"FACP") {
        parse_fadt(fadt);
    }
    
    // 6. Parse MCFG for PCIe ECAM
    if let Some(mcfg) = find_table(b"MCFG") {
        parse_mcfg(mcfg);
    }
    
    serial_println!("[ACPI] Initialized successfully");
    Ok(())
}
```

## Future Enhancements

1. **Full AML Interpreter** - Execute DSDT/SSDT methods
2. **CPU Frequency Scaling** - P-states and C-states
3. **Thermal Management** - Temperature monitoring and throttling
4. **Battery Support** - For laptop platforms
5. **Embedded Controller** - EC communication protocol
6. **NUMA Topology** - SRAT/SLIT parsing for memory topology
