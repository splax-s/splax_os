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
        // Register with S-LINK
        // In full implementation: IPC registration

        // Enumerate PCI devices
        self.enumerate_pci()?;

        // Enumerate USB devices
        self.enumerate_usb()?;

        Ok(())
    }

    /// Enumerates PCI devices
    fn enumerate_pci(&mut self) -> Result<(), DevError> {
        // In full implementation: scan PCI bus
        // For now, return empty list
        Ok(())
    }

    /// Enumerates USB devices
    fn enumerate_usb(&mut self) -> Result<(), DevError> {
        // In full implementation: scan USB bus
        // For now, return empty list
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
            // In full implementation:
            // 1. Wait for IPC messages
            // 2. Process device events
            // 3. Handle hotplug
            // 4. Forward interrupts

            // Placeholder: yield CPU
            core::hint::spin_loop();
        }
    }
}

impl Default for DevService {
    fn default() -> Self {
        Self::new(DevConfig::default())
    }
}
