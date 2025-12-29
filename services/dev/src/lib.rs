//! # S-DEV: Splax OS Device Driver Service
//!
//! S-DEV is the userspace device driver service that manages hardware drivers.
//! As part of Splax OS's microkernel architecture, device drivers run in
//! userspace while only interrupt handling and DMA remain in the kernel.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      USERSPACE                              │
//! │ ┌─────────────────────────────────────────────────────────┐ │
//! │ │                     S-DEV Service                       │ │
//! │ │  ┌─────────────────────────────────────────────────────┐│ │
//! │ │  │              Driver Manager                         ││ │
//! │ │  │  - Driver registration/unregistration               ││ │
//! │ │  │  - Device enumeration                               ││ │
//! │ │  │  - Hot-plug handling                                ││ │
//! │ │  └─────────────────────────────────────────────────────┘│ │
//! │ │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │ │
//! │ │  │   USB    │ │  Sound   │ │  Input   │ │  Block   │   │ │
//! │ │  │ Drivers  │ │ Drivers  │ │ Drivers  │ │ Drivers  │   │ │
//! │ │  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │ │
//! │ │  ┌─────────────────────────────────────────────────────┐│ │
//! │ │  │           Interrupt Forwarder                       ││ │
//! │ │  │  - Receives IRQs from kernel                        ││ │
//! │ │  │  - Dispatches to appropriate driver                 ││ │
//! │ │  └─────────────────────────────────────────────────────┘│ │
//! │ └─────────────────────────────────────────────────────────┘ │
//! ├─────────────────────────────────────────────────────────────┤
//! │                         S-LINK IPC                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                      KERNEL (S-CORE)                        │
//! │ ┌─────────────────────────────────────────────────────────┐ │
//! │ │            Hardware Abstraction Layer                   │ │
//! │ │  - Interrupt delivery to userspace                      │ │
//! │ │  - DMA buffer management                                │ │
//! │ │  - MMIO mapping                                         │ │
//! │ └─────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## IPC Protocol
//!
//! S-DEV communicates via S-LINK channels:
//!
//! - `dev.register` - Register a new driver
//! - `dev.unregister` - Unregister a driver
//! - `dev.enumerate` - List available devices
//! - `dev.bind` - Bind driver to device
//! - `dev.unbind` - Unbind driver from device
//! - `dev.irq` - Interrupt notification from kernel
//!
//! ## Security
//!
//! Device access requires appropriate S-CAP capabilities:
//!
//! - `cap:dev:usb` - USB device access
//! - `cap:dev:sound` - Sound device access
//! - `cap:dev:input` - Input device access
//! - `cap:dev:block` - Block device access
//! - `cap:dev:dma` - DMA buffer access

#![no_std]

extern crate alloc;

pub mod driver;
pub mod usb;
pub mod sound;
pub mod input;
pub mod irq;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// S-DEV service version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// S-DEV service name for IPC registration
pub const SERVICE_NAME: &str = "dev";

/// Device types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// USB devices
    Usb,
    /// Sound devices
    Sound,
    /// Input devices (keyboard, mouse, etc.)
    Input,
    /// Block devices
    Block,
    /// Network devices (handled by S-NET)
    Network,
    /// Display/GPU devices
    Display,
    /// Unknown/other
    Other,
}

/// Device identifier
#[derive(Debug, Clone)]
pub struct DeviceId {
    /// Bus type (pci, usb, etc.)
    pub bus: String,
    /// Vendor ID
    pub vendor: u16,
    /// Product ID
    pub product: u16,
    /// Subsystem vendor
    pub subsys_vendor: u16,
    /// Subsystem device
    pub subsys_device: u16,
    /// Device class
    pub class: u32,
}

impl DeviceId {
    /// Creates a PCI device ID
    pub fn pci(vendor: u16, product: u16, class: u32) -> Self {
        Self {
            bus: String::from("pci"),
            vendor,
            product,
            subsys_vendor: 0,
            subsys_device: 0,
            class,
        }
    }

    /// Creates a USB device ID
    pub fn usb(vendor: u16, product: u16, class: u32) -> Self {
        Self {
            bus: String::from("usb"),
            vendor,
            product,
            subsys_vendor: 0,
            subsys_device: 0,
            class,
        }
    }
}

/// Device instance
#[derive(Debug, Clone)]
pub struct Device {
    /// Unique device handle
    pub handle: u64,
    /// Device type
    pub dev_type: DeviceType,
    /// Device identifier
    pub id: DeviceId,
    /// Human-readable name
    pub name: String,
    /// Device path (e.g., /dev/usb0)
    pub path: String,
    /// Bound driver name (if any)
    pub driver: Option<String>,
    /// Device is enabled
    pub enabled: bool,
    /// IRQ number (if applicable)
    pub irq: Option<u32>,
    /// MMIO base address
    pub mmio_base: Option<u64>,
    /// MMIO size
    pub mmio_size: Option<u64>,
}

/// IPC message for device operations
#[derive(Debug)]
pub enum DevMessage {
    /// Register a driver
    RegisterDriver {
        name: String,
        device_type: DeviceType,
        supported_ids: Vec<DeviceId>,
    },
    /// Unregister a driver
    UnregisterDriver { name: String },
    /// Enumerate devices
    EnumerateDevices { dev_type: Option<DeviceType> },
    /// Bind driver to device
    BindDevice { driver: String, device: u64 },
    /// Unbind driver from device
    UnbindDevice { device: u64 },
    /// Enable device
    EnableDevice { device: u64 },
    /// Disable device
    DisableDevice { device: u64 },
    /// Read device register
    ReadRegister { device: u64, offset: u64 },
    /// Write device register
    WriteRegister { device: u64, offset: u64, value: u32 },
    /// Allocate DMA buffer
    AllocDma { device: u64, size: usize },
    /// Free DMA buffer
    FreeDma { device: u64, addr: u64 },
    /// IRQ notification
    IrqNotify { irq: u32 },
}

/// Device service configuration
#[derive(Debug, Clone)]
pub struct DevConfig {
    /// Maximum number of devices
    pub max_devices: usize,
    /// Maximum number of drivers
    pub max_drivers: usize,
    /// Enable hotplug detection
    pub hotplug: bool,
    /// DMA buffer pool size
    pub dma_pool_size: usize,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            max_devices: 256,
            max_drivers: 64,
            hotplug: true,
            dma_pool_size: 16 * 1024 * 1024, // 16 MB
        }
    }
}

/// Device service error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevError {
    /// Device not found
    DeviceNotFound,
    /// Driver not found
    DriverNotFound,
    /// Device already bound
    AlreadyBound,
    /// Device not bound
    NotBound,
    /// No matching driver
    NoMatchingDriver,
    /// Permission denied
    PermissionDenied,
    /// Invalid argument
    InvalidArgument,
    /// Out of memory
    OutOfMemory,
    /// Hardware error
    HardwareError,
    /// IPC error
    IpcError,
    /// Internal error
    Internal,
}

/// Device service
pub struct DevService {
    /// Configuration
    config: DevConfig,
    /// Registered drivers
    drivers: BTreeMap<String, DriverInfo>,
    /// Known devices
    devices: BTreeMap<u64, Device>,
    /// Next device handle
    next_handle: u64,
    /// IRQ handlers (irq -> driver name)
    irq_handlers: BTreeMap<u32, String>,
}

/// Driver information
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// Driver name
    pub name: String,
    /// Device type this driver handles
    pub dev_type: DeviceType,
    /// Supported device IDs
    pub supported_ids: Vec<DeviceId>,
    /// Number of bound devices
    pub bound_count: usize,
}

/// PCI device information (internal use during enumeration)
#[derive(Debug, Clone, Copy)]
struct PciDeviceInfo {
    bus: u8,
    device: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    class_code: u8,
    subclass: u8,
    prog_if: u8,
    revision: u8,
    header_type: u8,
    subsys_vendor: u16,
    subsys_device: u16,
    irq_line: u8,
    irq_pin: u8,
    mmio_base: Option<u64>,
    mmio_size: Option<u64>,
}

/// USB controller types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsbControllerType {
    /// Universal Host Controller Interface (USB 1.x)
    Uhci,
    /// Open Host Controller Interface (USB 1.x)
    Ohci,
    /// Enhanced Host Controller Interface (USB 2.0)
    Ehci,
    /// eXtensible Host Controller Interface (USB 3.x)
    Xhci,
}

impl DevService {
    /// Creates a new device service
    pub fn new(config: DevConfig) -> Self {
        Self {
            config,
            drivers: BTreeMap::new(),
            devices: BTreeMap::new(),
            next_handle: 1,
            irq_handlers: BTreeMap::new(),
        }
    }

    /// Initializes the service
    pub fn init(&mut self) -> Result<(), DevError> {
        // Register with S-LINK IPC system
        // Create IPC channel for device service communication
        self.register_ipc_channel()?;

        // Enumerate PCI devices
        self.enumerate_pci()?;

        // Enumerate USB devices
        self.enumerate_usb()?;

        Ok(())
    }

    /// Registers IPC channel with S-LINK for inter-service communication
    fn register_ipc_channel(&self) -> Result<(), DevError> {
        // IPC channel registration for device service
        // This enables communication with other services (S-INIT, S-STORAGE, etc.)
        //
        // Protocol channels:
        // - "dev.register"   : Driver registration requests
        // - "dev.unregister" : Driver unregistration
        // - "dev.enumerate"  : Device enumeration queries
        // - "dev.bind"       : Bind driver to device
        // - "dev.unbind"     : Unbind driver from device
        // - "dev.irq"        : Interrupt notifications from kernel
        //
        // In a full microkernel, this would use the S-LINK Channel API:
        // let router = LinkRouter::new(ChannelConfig::default());
        // let cap_token = CapabilityToken::for_service(SERVICE_NAME);
        // router.create_channel(SERVICE_NAME, "init", &cap_token)?;
        // router.create_channel(SERVICE_NAME, "storage", &cap_token)?;
        //
        // For now, IPC is handled through shared memory message queues
        // that the kernel sets up during service initialization
        Ok(())
    }

    /// Enumerates PCI devices
    fn enumerate_pci(&mut self) -> Result<(), DevError> {
        // PCI bus enumeration using Configuration Space Access Mechanism
        // Scans all buses (0-255), devices (0-31), and functions (0-7)
        
        for bus in 0u8..=255 {
            for device in 0u8..32 {
                self.probe_pci_device(bus, device)?;
            }
        }
        Ok(())
    }

    /// Probes a single PCI device slot
    fn probe_pci_device(&mut self, bus: u8, device: u8) -> Result<(), DevError> {
        // Check function 0 first
        if let Some(pci_dev) = self.read_pci_device(bus, device, 0)? {
            self.register_pci_device(pci_dev)?;
            
            // Check if multi-function device (bit 7 of header type)
            if pci_dev.header_type & 0x80 != 0 {
                for function in 1u8..8 {
                    if let Some(fn_dev) = self.read_pci_device(bus, device, function)? {
                        self.register_pci_device(fn_dev)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Reads PCI device configuration space
    fn read_pci_device(&self, bus: u8, device: u8, function: u8) -> Result<Option<PciDeviceInfo>, DevError> {
        // Build configuration address
        // Bit 31: Enable bit
        // Bits 23-16: Bus number
        // Bits 15-11: Device number  
        // Bits 10-8: Function number
        // Bits 7-0: Register offset (must be aligned to 4 bytes)
        
        let config_addr = (1u32 << 31)
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8);

        // Read vendor/device ID from offset 0x00
        let vendor_device = self.pci_config_read(config_addr | 0x00);
        let vendor_id = (vendor_device & 0xFFFF) as u16;
        let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;

        // 0xFFFF means no device present
        if vendor_id == 0xFFFF {
            return Ok(None);
        }

        // Read class/revision from offset 0x08
        let class_reg = self.pci_config_read(config_addr | 0x08);
        let class_code = (class_reg >> 24) as u8;
        let subclass = ((class_reg >> 16) & 0xFF) as u8;
        let prog_if = ((class_reg >> 8) & 0xFF) as u8;
        let revision = (class_reg & 0xFF) as u8;

        // Read header type from offset 0x0C
        let header_reg = self.pci_config_read(config_addr | 0x0C);
        let header_type = ((header_reg >> 16) & 0xFF) as u8;

        // Read subsystem IDs from offset 0x2C (for type 0 headers)
        let subsys_reg = self.pci_config_read(config_addr | 0x2C);
        let subsys_vendor = (subsys_reg & 0xFFFF) as u16;
        let subsys_device = ((subsys_reg >> 16) & 0xFFFF) as u16;

        // Read interrupt info from offset 0x3C
        let int_reg = self.pci_config_read(config_addr | 0x3C);
        let irq_line = (int_reg & 0xFF) as u8;
        let irq_pin = ((int_reg >> 8) & 0xFF) as u8;

        // Read BAR0 for MMIO base
        let bar0 = self.pci_config_read(config_addr | 0x10);
        let (mmio_base, mmio_size) = if bar0 & 0x01 == 0 && bar0 != 0 {
            // Memory BAR - mask low bits to get base address
            let base = (bar0 & 0xFFFFFFF0) as u64;
            // Size detection would require writing 0xFFFFFFFF and reading back
            (Some(base), Some(0x1000u64)) // Default 4KB size estimate
        } else {
            (None, None)
        };

        Ok(Some(PciDeviceInfo {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision,
            header_type,
            subsys_vendor,
            subsys_device,
            irq_line,
            irq_pin,
            mmio_base,
            mmio_size,
        }))
    }

    /// Performs PCI configuration space read
    /// Uses I/O ports 0xCF8 (address) and 0xCFC (data) on x86
    #[cfg(target_arch = "x86_64")]
    fn pci_config_read(&self, address: u32) -> u32 {
        use core::arch::asm;
        const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
        const PCI_CONFIG_DATA: u16 = 0xCFC;
        
        unsafe {
            // Write address to CONFIG_ADDRESS port
            asm!("out dx, eax", in("dx") PCI_CONFIG_ADDRESS, in("eax") address, options(nomem, nostack, preserves_flags));
            // Read data from CONFIG_DATA port
            let value: u32;
            asm!("in eax, dx", out("eax") value, in("dx") PCI_CONFIG_DATA, options(nomem, nostack, preserves_flags));
            value
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn pci_config_read(&self, _address: u32) -> u32 {
        // On non-x86 architectures (ARM, RISC-V), PCI uses ECAM (memory-mapped)
        // This would require MMIO access through the kernel's memory mapping
        0xFFFFFFFF
    }

    /// Registers a discovered PCI device
    fn register_pci_device(&mut self, info: PciDeviceInfo) -> Result<(), DevError> {
        if self.devices.len() >= self.config.max_devices {
            return Err(DevError::OutOfMemory);
        }

        let handle = self.next_handle;
        self.next_handle += 1;

        let dev_type = Self::pci_class_to_device_type(info.class_code, info.subclass);
        let name = alloc::format!(
            "pci:{:04x}:{:04x} [{:02x}{:02x}]",
            info.vendor_id, info.device_id, info.class_code, info.subclass
        );
        let path = alloc::format!(
            "/dev/pci/{:02x}:{:02x}.{}",
            info.bus, info.device, info.function
        );

        let device = Device {
            handle,
            dev_type,
            id: DeviceId::pci(
                info.vendor_id,
                info.device_id,
                ((info.class_code as u32) << 16) | ((info.subclass as u32) << 8) | (info.prog_if as u32),
            ),
            name,
            path,
            driver: None,
            enabled: true,
            irq: if info.irq_line != 0 && info.irq_line != 255 {
                Some(info.irq_line as u32)
            } else {
                None
            },
            mmio_base: info.mmio_base,
            mmio_size: info.mmio_size,
        };

        self.devices.insert(handle, device);
        Ok(())
    }

    /// Maps PCI class codes to device types
    fn pci_class_to_device_type(class: u8, subclass: u8) -> DeviceType {
        match (class, subclass) {
            (0x01, _) => DeviceType::Block,      // Mass storage
            (0x02, _) => DeviceType::Network,    // Network controller
            (0x03, _) => DeviceType::Display,    // Display controller
            (0x04, 0x01) => DeviceType::Sound,   // Audio device
            (0x04, 0x03) => DeviceType::Sound,   // Audio device
            (0x09, _) => DeviceType::Input,      // Input devices
            (0x0C, 0x03) => DeviceType::Usb,     // USB controller
            _ => DeviceType::Other,
        }
    }

    /// Enumerates USB devices
    fn enumerate_usb(&mut self) -> Result<(), DevError> {
        // USB enumeration requires a USB Host Controller
        // Common controllers: UHCI, OHCI (USB 1.x), EHCI (USB 2.0), XHCI (USB 3.x)
        //
        // USB enumeration process:
        // 1. Find USB host controllers (already discovered via PCI enumeration)
        // 2. Initialize the host controller
        // 3. For each root hub port:
        //    a. Check port status for device connection
        //    b. Reset the port
        //    c. Enumerate the device (assign address, read descriptors)
        //    d. If hub, recursively enumerate downstream ports

        // Find USB host controllers from discovered PCI devices
        let usb_controllers: Vec<u64> = self.devices
            .iter()
            .filter(|(_, d)| d.dev_type == DeviceType::Usb)
            .map(|(h, _)| *h)
            .collect();

        for controller_handle in usb_controllers {
            self.enumerate_usb_controller(controller_handle)?;
        }

        Ok(())
    }

    /// Enumerates devices on a USB host controller
    fn enumerate_usb_controller(&mut self, controller_handle: u64) -> Result<(), DevError> {
        let controller = self.devices.get(&controller_handle).ok_or(DevError::DeviceNotFound)?;
        
        // Determine controller type from PCI class code
        // USB controllers are class 0x0C, subclass 0x03
        // prog_if: 0x00 = UHCI, 0x10 = OHCI, 0x20 = EHCI, 0x30 = XHCI
        let prog_if = (controller.id.class & 0xFF) as u8;
        
        let _controller_type = match prog_if {
            0x00 => UsbControllerType::Uhci,
            0x10 => UsbControllerType::Ohci,
            0x20 => UsbControllerType::Ehci,
            0x30 => UsbControllerType::Xhci,
            _ => return Ok(()), // Unknown controller type
        };

        // In a full implementation, we would:
        // 1. Map the controller's MMIO region
        // 2. Reset and initialize the controller
        // 3. Enable root hub ports
        // 4. Poll for connected devices
        // 5. For each connected device:
        //    - Reset port
        //    - Assign address (SET_ADDRESS request)
        //    - Read device descriptor
        //    - Read configuration descriptors
        //    - Register device with S-DEV
        //
        // This requires kernel support for:
        // - MMIO mapping
        // - DMA buffer allocation
        // - Interrupt handling

        // For now, USB enumeration is deferred until controller drivers are loaded
        // The USB manager (usb.rs) handles runtime enumeration
        Ok(())
    }

    /// Registers a driver
    pub fn register_driver(
        &mut self,
        name: &str,
        dev_type: DeviceType,
        supported_ids: Vec<DeviceId>,
    ) -> Result<(), DevError> {
        if self.drivers.len() >= self.config.max_drivers {
            return Err(DevError::OutOfMemory);
        }

        let info = DriverInfo {
            name: String::from(name),
            dev_type,
            supported_ids,
            bound_count: 0,
        };

        self.drivers.insert(String::from(name), info);

        // Try to bind to existing unbound devices
        self.autobind_driver(name)?;

        Ok(())
    }

    /// Unregisters a driver
    pub fn unregister_driver(&mut self, name: &str) -> Result<(), DevError> {
        let driver = self.drivers.get(name).ok_or(DevError::DriverNotFound)?;

        if driver.bound_count > 0 {
            // Unbind from all devices first
            let bound_devices: Vec<u64> = self
                .devices
                .iter()
                .filter(|(_, d)| d.driver.as_deref() == Some(name))
                .map(|(h, _)| *h)
                .collect();

            for handle in bound_devices {
                self.unbind_device(handle)?;
            }
        }

        self.drivers.remove(name);
        Ok(())
    }

    /// Attempts to autobind a driver to matching devices
    fn autobind_driver(&mut self, driver_name: &str) -> Result<(), DevError> {
        let driver = self.drivers.get(driver_name).ok_or(DevError::DriverNotFound)?;
        let supported_ids = driver.supported_ids.clone();

        for (handle, device) in &mut self.devices {
            if device.driver.is_some() {
                continue;
            }

            for id in &supported_ids {
                if device.id.vendor == id.vendor && device.id.product == id.product {
                    device.driver = Some(String::from(driver_name));
                    if let Some(d) = self.drivers.get_mut(driver_name) {
                        d.bound_count += 1;
                    }
                    let _ = handle;
                    break;
                }
            }
        }

        Ok(())
    }

    /// Binds a driver to a device
    pub fn bind_device(&mut self, driver_name: &str, handle: u64) -> Result<(), DevError> {
        if !self.drivers.contains_key(driver_name) {
            return Err(DevError::DriverNotFound);
        }

        let device = self.devices.get_mut(&handle).ok_or(DevError::DeviceNotFound)?;

        if device.driver.is_some() {
            return Err(DevError::AlreadyBound);
        }

        device.driver = Some(String::from(driver_name));

        if let Some(driver) = self.drivers.get_mut(driver_name) {
            driver.bound_count += 1;
        }

        Ok(())
    }

    /// Unbinds a driver from a device
    pub fn unbind_device(&mut self, handle: u64) -> Result<(), DevError> {
        let device = self.devices.get_mut(&handle).ok_or(DevError::DeviceNotFound)?;

        let driver_name = device.driver.take().ok_or(DevError::NotBound)?;

        if let Some(driver) = self.drivers.get_mut(&driver_name) {
            driver.bound_count = driver.bound_count.saturating_sub(1);
        }

        Ok(())
    }

    /// Gets a device by handle
    pub fn get_device(&self, handle: u64) -> Option<&Device> {
        self.devices.get(&handle)
    }

    /// Lists all devices
    pub fn list_devices(&self, dev_type: Option<DeviceType>) -> Vec<&Device> {
        self.devices
            .values()
            .filter(|d| dev_type.map_or(true, |t| d.dev_type == t))
            .collect()
    }

    /// Lists all drivers
    pub fn list_drivers(&self) -> Vec<&DriverInfo> {
        self.drivers.values().collect()
    }

    /// Handles an IRQ
    pub fn handle_irq(&mut self, irq: u32) -> Option<&str> {
        self.irq_handlers.get(&irq).map(|s| s.as_str())
    }

    /// Registers an IRQ handler
    pub fn register_irq(&mut self, irq: u32, driver: &str) -> Result<(), DevError> {
        if !self.drivers.contains_key(driver) {
            return Err(DevError::DriverNotFound);
        }

        self.irq_handlers.insert(irq, String::from(driver));
        Ok(())
    }

    /// Runs the service main loop
    pub fn run(&mut self) -> ! {
        loop {
            // Main service loop for S-DEV
            
            // 1. Poll for IPC messages from S-LINK
            //    Messages arrive on registered channels from other services
            if let Some(message) = self.poll_ipc_message() {
                self.handle_ipc_message(message);
            }

            // 2. Process device events (power state changes, errors)
            self.process_device_events();

            // 3. Handle hotplug detection
            //    USB: Poll root hub port status change bits
            //    PCI: Check for hot-plug capable slots (PCIe)
            self.check_hotplug();

            // 4. Forward pending interrupts to appropriate drivers
            //    IRQs are delivered by kernel and need to be dispatched
            self.dispatch_pending_irqs();

            // Yield CPU to prevent busy-spinning
            // On x86, this emits PAUSE instruction which:
            // - Improves spin-wait loop performance
            // - Reduces power consumption  
            // - Avoids memory order violations on some CPUs
            core::hint::spin_loop();
        }
    }

    /// Polls for incoming IPC messages
    fn poll_ipc_message(&self) -> Option<DevMessage> {
        // In a full implementation, this would read from S-LINK channels
        // The channel receive operation would be non-blocking:
        //
        // let channel = self.ipc_channel.as_ref()?;
        // match channel.try_receive() {
        //     Ok(msg) => Some(decode_message(msg.payload)),
        //     Err(LinkError::NoMessage) => None,
        //     Err(_) => None,
        // }
        None
    }

    /// Handles an incoming IPC message
    fn handle_ipc_message(&mut self, message: DevMessage) {
        match message {
            DevMessage::RegisterDriver { name, device_type, supported_ids } => {
                let _ = self.register_driver(&name, device_type, supported_ids);
            }
            DevMessage::UnregisterDriver { name } => {
                let _ = self.unregister_driver(&name);
            }
            DevMessage::EnumerateDevices { dev_type } => {
                let _devices = self.list_devices(dev_type);
                // Send response via IPC
            }
            DevMessage::BindDevice { driver, device } => {
                let _ = self.bind_device(&driver, device);
            }
            DevMessage::UnbindDevice { device } => {
                let _ = self.unbind_device(device);
            }
            DevMessage::IrqNotify { irq } => {
                let _ = self.handle_irq(irq);
            }
            _ => {}
        }
    }

    /// Processes pending device events
    fn process_device_events(&mut self) {
        // Check for device state changes, errors, etc.
        // This would involve checking device status registers
        // and updating internal state accordingly
    }

    /// Checks for hotplug events
    fn check_hotplug(&mut self) {
        if !self.config.hotplug {
            return;
        }
        // USB hotplug: Check root hub port status change bits
        // PCIe hotplug: Check slot status registers
        // This is typically interrupt-driven in practice
    }

    /// Dispatches pending IRQs to registered handlers
    fn dispatch_pending_irqs(&mut self) {
        // IRQs would be queued by the kernel IRQ forwarder
        // Each IRQ is looked up in irq_handlers map and
        // forwarded to the appropriate driver
    }
}

impl Default for DevService {
    fn default() -> Self {
        Self::new(DevConfig::default())
    }
}
