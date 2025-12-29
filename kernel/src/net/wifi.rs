//! # Wireless Network (WiFi) Driver Framework
//!
//! This module provides the foundation for 802.11 WiFi drivers.
//! WiFi is significantly more complex than Ethernet due to:
//! - Scanning for networks (probe requests/responses)
//! - Authentication (Open, WPA2-PSK, WPA3-SAE, 802.1X)
//! - Association with access points
//! - Key exchange (4-way handshake for WPA2)
//! - Encryption (CCMP/TKIP for WPA2, GCMP for WPA3)
//! - Power management and beacons
//!
//! ## Supported Chipset Families (Future)
//! - Intel WiFi (iwlwifi): AC 7260, AX200, AX210, etc.
//! - Realtek (rtl8xxx): RTL8723, RTL8821, RTL8852, etc.
//! - Atheros/Qualcomm (ath9k, ath10k, ath11k)
//! - Broadcom (brcmfmac): BCM43xx series
//! - MediaTek (mt76): MT7921, MT7922, etc.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;

use super::device::NetworkError;
use super::ethernet::MacAddress;

/// WiFi security types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    /// No security (open network)
    Open,
    /// WEP (deprecated, insecure)
    Wep,
    /// WPA Personal (TKIP)
    WpaPsk,
    /// WPA2 Personal (CCMP/AES)
    Wpa2Psk,
    /// WPA3 Personal (SAE)
    Wpa3Sae,
    /// WPA2 Enterprise (802.1X)
    Wpa2Enterprise,
    /// WPA3 Enterprise
    Wpa3Enterprise,
    /// Unknown/other
    Unknown,
}

/// WiFi frequency band
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiBand {
    /// 2.4 GHz (802.11b/g/n)
    Band2_4GHz,
    /// 5 GHz (802.11a/n/ac)
    Band5GHz,
    /// 6 GHz (802.11ax/WiFi 6E)
    Band6GHz,
}

/// WiFi standard/generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiStandard {
    /// 802.11b (11 Mbps)
    B,
    /// 802.11g (54 Mbps)
    G,
    /// 802.11n (WiFi 4, 600 Mbps)
    N,
    /// 802.11ac (WiFi 5, 6.9 Gbps)
    Ac,
    /// 802.11ax (WiFi 6, 9.6 Gbps)
    Ax,
    /// 802.11be (WiFi 7, 46 Gbps)
    Be,
}

/// Information about a discovered WiFi network
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    /// SSID (network name)
    pub ssid: String,
    /// BSSID (access point MAC address)
    pub bssid: MacAddress,
    /// Channel number
    pub channel: u8,
    /// Frequency in MHz
    pub frequency: u16,
    /// Signal strength in dBm (negative, closer to 0 is stronger)
    pub signal_dbm: i8,
    /// Signal quality as percentage (0-100)
    pub signal_quality: u8,
    /// Security type
    pub security: WifiSecurity,
    /// Frequency band
    pub band: WifiBand,
    /// WiFi standard if known
    pub standard: Option<WifiStandard>,
    /// Whether network is hidden (no SSID broadcast)
    pub hidden: bool,
}

/// WiFi connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    /// Not associated with any network
    Disconnected,
    /// Scanning for networks
    Scanning,
    /// Attempting to authenticate
    Authenticating,
    /// Performing 4-way handshake (WPA2)
    KeyExchange,
    /// Successfully connected
    Connected,
    /// Connection failed
    Failed,
}

/// WiFi credentials for connecting
#[derive(Debug, Clone)]
pub struct WifiCredentials {
    /// SSID to connect to
    pub ssid: String,
    /// Password (for PSK networks)
    pub password: Option<String>,
    /// Enterprise identity (for 802.1X)
    pub identity: Option<String>,
}

/// Statistics for WiFi connection
#[derive(Debug, Clone, Default)]
pub struct WifiStats {
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Packets transmitted
    pub tx_packets: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Transmission failures
    pub tx_failures: u64,
    /// CRC errors
    pub rx_crc_errors: u64,
    /// Retries
    pub tx_retries: u64,
    /// Current TX rate in Mbps
    pub tx_rate_mbps: u16,
    /// Current RX rate in Mbps
    pub rx_rate_mbps: u16,
}

/// 802.11 frame types
pub mod frame_types {
    // Management frames
    pub const MGMT_ASSOC_REQ: u8 = 0x00;
    pub const MGMT_ASSOC_RESP: u8 = 0x01;
    pub const MGMT_PROBE_REQ: u8 = 0x04;
    pub const MGMT_PROBE_RESP: u8 = 0x05;
    pub const MGMT_BEACON: u8 = 0x08;
    pub const MGMT_DISASSOC: u8 = 0x0A;
    pub const MGMT_AUTH: u8 = 0x0B;
    pub const MGMT_DEAUTH: u8 = 0x0C;
    pub const MGMT_ACTION: u8 = 0x0D;
    
    // Control frames  
    pub const CTRL_ACK: u8 = 0x1D;
    pub const CTRL_RTS: u8 = 0x1B;
    pub const CTRL_CTS: u8 = 0x1C;
    
    // Data frames
    pub const DATA: u8 = 0x20;
    pub const DATA_QOS: u8 = 0x28;
}

/// 802.11 Information Element IDs
pub mod ie_ids {
    pub const SSID: u8 = 0;
    pub const SUPPORTED_RATES: u8 = 1;
    pub const DS_PARAMS: u8 = 3;  // Channel
    pub const TIM: u8 = 5;        // Traffic indication map
    pub const COUNTRY: u8 = 7;
    pub const RSN: u8 = 48;       // WPA2 security
    pub const HT_CAPS: u8 = 45;   // 802.11n
    pub const HT_OPERATION: u8 = 61;
    pub const VHT_CAPS: u8 = 191; // 802.11ac
    pub const VHT_OPERATION: u8 = 192;
    pub const VENDOR: u8 = 221;   // WPA1, WMM, etc.
}

/// 802.11 Frame Control field
#[derive(Debug, Clone, Copy)]
pub struct FrameControl {
    /// Protocol version (always 0)
    pub protocol_version: u8,
    /// Frame type (0=Management, 1=Control, 2=Data)
    pub frame_type: u8,
    /// Frame subtype
    pub subtype: u8,
    /// To DS flag
    pub to_ds: bool,
    /// From DS flag
    pub from_ds: bool,
    /// More fragments flag
    pub more_fragments: bool,
    /// Retry flag
    pub retry: bool,
    /// Power management flag
    pub power_mgmt: bool,
    /// More data flag
    pub more_data: bool,
    /// Protected frame flag (encrypted)
    pub protected: bool,
    /// Order flag
    pub order: bool,
}

impl FrameControl {
    /// Parse frame control from 2 bytes
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 2 {
            return None;
        }
        let fc = u16::from_le_bytes([bytes[0], bytes[1]]);
        
        Some(Self {
            protocol_version: (fc & 0x03) as u8,
            frame_type: ((fc >> 2) & 0x03) as u8,
            subtype: ((fc >> 4) & 0x0F) as u8,
            to_ds: (fc & (1 << 8)) != 0,
            from_ds: (fc & (1 << 9)) != 0,
            more_fragments: (fc & (1 << 10)) != 0,
            retry: (fc & (1 << 11)) != 0,
            power_mgmt: (fc & (1 << 12)) != 0,
            more_data: (fc & (1 << 13)) != 0,
            protected: (fc & (1 << 14)) != 0,
            order: (fc & (1 << 15)) != 0,
        })
    }
    
    /// Check if this is a management frame
    pub fn is_management(&self) -> bool {
        self.frame_type == 0
    }
    
    /// Check if this is a data frame
    pub fn is_data(&self) -> bool {
        self.frame_type == 2
    }
    
    /// Check if this is a beacon frame
    pub fn is_beacon(&self) -> bool {
        self.is_management() && self.subtype == 8
    }
    
    /// Check if this is a probe response
    pub fn is_probe_response(&self) -> bool {
        self.is_management() && self.subtype == 5
    }
}

/// 802.11 MAC Header (24-30 bytes depending on frame type)
#[derive(Debug, Clone)]
pub struct MacHeader {
    /// Frame control field
    pub frame_control: FrameControl,
    /// Duration/ID field
    pub duration: u16,
    /// Address 1 (destination/receiver)
    pub addr1: MacAddress,
    /// Address 2 (source/transmitter)
    pub addr2: MacAddress,
    /// Address 3 (BSSID or destination/source)
    pub addr3: MacAddress,
    /// Sequence control (fragment number + sequence number)
    pub sequence_control: u16,
    /// Address 4 (only in WDS frames)
    pub addr4: Option<MacAddress>,
}

impl MacHeader {
    /// Parse MAC header from bytes
    pub fn parse(bytes: &[u8]) -> Option<(Self, usize)> {
        if bytes.len() < 24 {
            return None;
        }
        
        let fc = FrameControl::parse(bytes)?;
        
        let duration = u16::from_le_bytes([bytes[2], bytes[3]]);
        
        let addr1 = MacAddress([bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9]]);
        let addr2 = MacAddress([bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]]);
        let addr3 = MacAddress([bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21]]);
        
        let sequence_control = u16::from_le_bytes([bytes[22], bytes[23]]);
        
        // Check if addr4 is present (WDS mode: both to_ds and from_ds set)
        let (addr4, header_len) = if fc.to_ds && fc.from_ds {
            if bytes.len() < 30 {
                return None;
            }
            (Some(MacAddress([bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29]])), 30)
        } else {
            (None, 24)
        };
        
        Some((Self {
            frame_control: fc,
            duration,
            addr1,
            addr2,
            addr3,
            sequence_control,
            addr4,
        }, header_len))
    }
    
    /// Get sequence number from sequence control
    pub fn sequence_number(&self) -> u16 {
        self.sequence_control >> 4
    }
    
    /// Get fragment number from sequence control
    pub fn fragment_number(&self) -> u8 {
        (self.sequence_control & 0x0F) as u8
    }
}

/// Beacon/Probe Response frame body
#[derive(Debug, Clone)]
pub struct BeaconFrame {
    /// Timestamp (64-bit microseconds since device started)
    pub timestamp: u64,
    /// Beacon interval in TU (1024 microseconds)
    pub beacon_interval: u16,
    /// Capability information
    pub capability: u16,
    /// SSID
    pub ssid: String,
    /// Supported rates (Mbps * 2)
    pub supported_rates: Vec<u8>,
    /// Channel number
    pub channel: u8,
    /// RSN information element (if present)
    pub rsn_ie: Option<Vec<u8>>,
    /// WPA information element (if present)
    pub wpa_ie: Option<Vec<u8>>,
}

impl BeaconFrame {
    /// Parse beacon/probe response frame body
    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 12 {
            return None;
        }
        
        let timestamp = u64::from_le_bytes([
            body[0], body[1], body[2], body[3],
            body[4], body[5], body[6], body[7]
        ]);
        let beacon_interval = u16::from_le_bytes([body[8], body[9]]);
        let capability = u16::from_le_bytes([body[10], body[11]]);
        
        let mut ssid = String::new();
        let mut supported_rates = Vec::new();
        let mut channel = 0u8;
        let mut rsn_ie = None;
        let mut wpa_ie = None;
        
        // Parse Information Elements
        let mut offset = 12;
        while offset + 2 <= body.len() {
            let ie_id = body[offset];
            let ie_len = body[offset + 1] as usize;
            offset += 2;
            
            if offset + ie_len > body.len() {
                break;
            }
            
            let ie_data = &body[offset..offset + ie_len];
            
            match ie_id {
                ie_ids::SSID => {
                    ssid = String::from_utf8_lossy(ie_data).into_owned();
                }
                ie_ids::SUPPORTED_RATES => {
                    supported_rates.extend_from_slice(ie_data);
                }
                ie_ids::DS_PARAMS => {
                    if ie_len >= 1 {
                        channel = ie_data[0];
                    }
                }
                ie_ids::RSN => {
                    rsn_ie = Some(ie_data.to_vec());
                }
                ie_ids::VENDOR => {
                    // Check for WPA OUI (00:50:F2:01)
                    if ie_len >= 4 && ie_data[0] == 0x00 && ie_data[1] == 0x50 
                        && ie_data[2] == 0xF2 && ie_data[3] == 0x01 {
                        wpa_ie = Some(ie_data.to_vec());
                    }
                }
                _ => {}
            }
            
            offset += ie_len;
        }
        
        Some(Self {
            timestamp,
            beacon_interval,
            capability,
            ssid,
            supported_rates,
            channel,
            rsn_ie,
            wpa_ie,
        })
    }
    
    /// Determine security type from capability and IEs
    pub fn security_type(&self) -> WifiSecurity {
        if self.rsn_ie.is_some() {
            // Has RSN IE = WPA2 or WPA3
            // Would need to parse RSN IE to distinguish WPA3
            WifiSecurity::Wpa2Psk
        } else if self.wpa_ie.is_some() {
            WifiSecurity::WpaPsk
        } else if self.capability & 0x0010 != 0 {
            // Privacy bit set but no WPA/RSN = WEP
            WifiSecurity::Wep
        } else {
            WifiSecurity::Open
        }
    }
}

/// Authentication frame
#[derive(Debug, Clone)]
pub struct AuthFrame {
    /// Authentication algorithm (0=Open, 1=Shared Key, 3=SAE)
    pub algorithm: u16,
    /// Authentication transaction sequence number
    pub transaction: u16,
    /// Status code (0=Success)
    pub status_code: u16,
    /// Challenge text (for shared key auth)
    pub challenge: Option<Vec<u8>>,
}

impl AuthFrame {
    /// Parse authentication frame body
    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 6 {
            return None;
        }
        
        let algorithm = u16::from_le_bytes([body[0], body[1]]);
        let transaction = u16::from_le_bytes([body[2], body[3]]);
        let status_code = u16::from_le_bytes([body[4], body[5]]);
        
        // Challenge text in IE if present
        let challenge = if body.len() > 8 && body[6] == 16 {
            let len = body[7] as usize;
            if body.len() >= 8 + len {
                Some(body[8..8 + len].to_vec())
            } else {
                None
            }
        } else {
            None
        };
        
        Some(Self {
            algorithm,
            transaction,
            status_code,
            challenge,
        })
    }
    
    /// Create an open system authentication request
    pub fn open_auth_request() -> Vec<u8> {
        let mut frame = Vec::with_capacity(6);
        frame.extend_from_slice(&0u16.to_le_bytes()); // Algorithm: Open System
        frame.extend_from_slice(&1u16.to_le_bytes()); // Transaction: 1
        frame.extend_from_slice(&0u16.to_le_bytes()); // Status: Success
        frame
    }
}

/// Association Request frame
#[derive(Debug, Clone)]
pub struct AssocRequestFrame {
    /// Capability information
    pub capability: u16,
    /// Listen interval
    pub listen_interval: u16,
    /// SSID
    pub ssid: String,
    /// Supported rates
    pub supported_rates: Vec<u8>,
}

/// Association Response frame
#[derive(Debug, Clone)]
pub struct AssocResponseFrame {
    /// Capability information
    pub capability: u16,
    /// Status code
    pub status_code: u16,
    /// Association ID (AID)
    pub association_id: u16,
    /// Supported rates
    pub supported_rates: Vec<u8>,
}

impl AssocResponseFrame {
    /// Parse association response frame body
    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 6 {
            return None;
        }
        
        let capability = u16::from_le_bytes([body[0], body[1]]);
        let status_code = u16::from_le_bytes([body[2], body[3]]);
        let association_id = u16::from_le_bytes([body[4], body[5]]) & 0x3FFF; // Mask top 2 bits
        
        let mut supported_rates = Vec::new();
        
        // Parse IEs
        let mut offset = 6;
        while offset + 2 <= body.len() {
            let ie_id = body[offset];
            let ie_len = body[offset + 1] as usize;
            offset += 2;
            
            if offset + ie_len > body.len() {
                break;
            }
            
            if ie_id == ie_ids::SUPPORTED_RATES {
                supported_rates.extend_from_slice(&body[offset..offset + ie_len]);
            }
            
            offset += ie_len;
        }
        
        Some(Self {
            capability,
            status_code,
            association_id,
            supported_rates,
        })
    }
}

/// WiFi device trait - implemented by specific chipset drivers
pub trait WifiDevice: Send + Sync {
    /// Get device information
    fn info(&self) -> WifiDeviceInfo;
    
    /// Initialize the device
    fn init(&mut self) -> Result<(), NetworkError>;
    
    /// Scan for available networks
    fn scan(&mut self) -> Result<Vec<WifiNetwork>, NetworkError>;
    
    /// Connect to a network
    fn connect(&mut self, creds: &WifiCredentials) -> Result<(), NetworkError>;
    
    /// Disconnect from current network
    fn disconnect(&mut self) -> Result<(), NetworkError>;
    
    /// Get current connection state
    fn state(&self) -> WifiState;
    
    /// Get current network if connected
    fn current_network(&self) -> Option<WifiNetwork>;
    
    /// Get connection statistics
    fn stats(&self) -> WifiStats;
    
    /// Get MAC address
    fn mac_address(&self) -> MacAddress;
    
    /// Set transmit power (in dBm)
    fn set_tx_power(&mut self, power_dbm: i8) -> Result<(), NetworkError>;
    
    /// Enable/disable power save mode
    fn set_power_save(&mut self, enabled: bool) -> Result<(), NetworkError>;
}

/// WiFi device information
#[derive(Debug, Clone)]
pub struct WifiDeviceInfo {
    /// Device name
    pub name: String,
    /// Vendor name
    pub vendor: String,
    /// Supported bands
    pub bands: Vec<WifiBand>,
    /// Supported standards
    pub standards: Vec<WifiStandard>,
    /// Maximum TX power in dBm
    pub max_tx_power: i8,
    /// Supports monitor mode
    pub monitor_mode: bool,
    /// Supports AP mode
    pub ap_mode: bool,
}

/// Mock WiFi device for testing
pub struct MockWifiDevice {
    mac: MacAddress,
    state: WifiState,
    current_network: Option<WifiNetwork>,
    stats: WifiStats,
}

impl MockWifiDevice {
    pub fn new() -> Self {
        Self {
            mac: MacAddress([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]),
            state: WifiState::Disconnected,
            current_network: None,
            stats: WifiStats::default(),
        }
    }
}

impl WifiDevice for MockWifiDevice {
    fn info(&self) -> WifiDeviceInfo {
        WifiDeviceInfo {
            name: String::from("Mock WiFi"),
            vendor: String::from("Splax OS"),
            bands: alloc::vec![WifiBand::Band2_4GHz, WifiBand::Band5GHz],
            standards: alloc::vec![WifiStandard::N, WifiStandard::Ac],
            max_tx_power: 20,
            monitor_mode: true,
            ap_mode: true,
        }
    }
    
    fn init(&mut self) -> Result<(), NetworkError> {
        Ok(())
    }
    
    fn scan(&mut self) -> Result<Vec<WifiNetwork>, NetworkError> {
        // Return mock networks for testing
        self.state = WifiState::Scanning;
        
        let networks = alloc::vec![
            WifiNetwork {
                ssid: String::from("TestNetwork"),
                bssid: MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
                channel: 6,
                frequency: 2437,
                signal_dbm: -45,
                signal_quality: 85,
                security: WifiSecurity::Wpa2Psk,
                band: WifiBand::Band2_4GHz,
                standard: Some(WifiStandard::N),
                hidden: false,
            },
            WifiNetwork {
                ssid: String::from("OpenWiFi"),
                bssid: MacAddress([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
                channel: 36,
                frequency: 5180,
                signal_dbm: -60,
                signal_quality: 65,
                security: WifiSecurity::Open,
                band: WifiBand::Band5GHz,
                standard: Some(WifiStandard::Ac),
                hidden: false,
            },
        ];
        
        self.state = WifiState::Disconnected;
        Ok(networks)
    }
    
    fn connect(&mut self, creds: &WifiCredentials) -> Result<(), NetworkError> {
        self.state = WifiState::Authenticating;
        
        // Simulate successful connection
        self.current_network = Some(WifiNetwork {
            ssid: creds.ssid.clone(),
            bssid: MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]),
            channel: 6,
            frequency: 2437,
            signal_dbm: -50,
            signal_quality: 80,
            security: if creds.password.is_some() { WifiSecurity::Wpa2Psk } else { WifiSecurity::Open },
            band: WifiBand::Band2_4GHz,
            standard: Some(WifiStandard::N),
            hidden: false,
        });
        
        self.state = WifiState::Connected;
        Ok(())
    }
    
    fn disconnect(&mut self) -> Result<(), NetworkError> {
        self.current_network = None;
        self.state = WifiState::Disconnected;
        Ok(())
    }
    
    fn state(&self) -> WifiState {
        self.state
    }
    
    fn current_network(&self) -> Option<WifiNetwork> {
        self.current_network.clone()
    }
    
    fn stats(&self) -> WifiStats {
        self.stats.clone()
    }
    
    fn mac_address(&self) -> MacAddress {
        self.mac
    }
    
    fn set_tx_power(&mut self, _power_dbm: i8) -> Result<(), NetworkError> {
        Ok(())
    }
    
    fn set_power_save(&mut self, _enabled: bool) -> Result<(), NetworkError> {
        Ok(())
    }
}

/// Global WiFi device (if present)
pub static WIFI_DEVICE: Mutex<Option<Arc<Mutex<dyn WifiDevice>>>> = Mutex::new(None);

/// Probe for WiFi devices on the system
#[cfg(target_arch = "x86_64")]
pub fn probe_wifi() -> Option<Arc<Mutex<dyn WifiDevice>>> {
    use core::fmt::Write;
    
    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
        let _ = writeln!(serial, "[wifi] Scanning for WiFi devices...");
    }
    
    // Scan PCI bus for WiFi devices
    for bus in 0..8u8 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let addr = 0x80000000u32
                    | ((bus as u32) << 16)
                    | ((device as u32) << 11)
                    | ((function as u32) << 8);
                
                let vendor_device = unsafe { pci_config_read(addr) };
                let vendor_id = (vendor_device & 0xFFFF) as u16;
                let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
                
                if vendor_id == 0xFFFF {
                    continue;
                }
                
                // Read class code
                let class_reg = unsafe { pci_config_read(addr | 0x08) };
                let class_code = ((class_reg >> 24) & 0xFF) as u8;
                let subclass = ((class_reg >> 16) & 0xFF) as u8;
                
                // Network controller (class 0x02), wireless subclass (0x80)
                if class_code == 0x02 && subclass == 0x80 {
                    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                        let vendor_name = match vendor_id {
                            0x8086 => "Intel",
                            0x10EC => "Realtek",
                            0x168C => "Qualcomm/Atheros",
                            0x14E4 => "Broadcom",
                            0x14C3 => "MediaTek",
                            _ => "Unknown",
                        };
                        let _ = writeln!(serial,
                            "[wifi] Found {} WiFi device {:04x}:{:04x} at {:02}:{:02}.{}",
                            vendor_name, vendor_id, device_id, bus, device, function);
                        let _ = writeln!(serial, "[wifi] Driver support coming soon!");
                    }
                    
                    // Driver instantiation based on vendor ID
                    // Note: Full driver support requires firmware loading and hardware init
                    // Currently returning mock device; vendor-specific drivers are WIP:
                    // - Intel (0x8086): iwlwifi driver - requires iwlwifi firmware
                    // - Realtek (0x10EC): rtl8xxx driver - USB/PCIe variants
                    // - Atheros (0x168C): ath9k/ath10k driver - open-source friendly
                    // - Broadcom (0x14E4): brcmfmac driver - requires proprietary firmware
                    // - MediaTek (0x14C3): mt76 driver - good open-source support
                    //
                    // To add a real driver:
                    // match vendor_id {
                    //     0x168C => return Some(Ath9kDriver::new(bar0)),
                    //     _ => {}
                    // }
                }
            }
        }
    }
    
    // No real WiFi device found
    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
        let _ = writeln!(serial, "[wifi] No WiFi devices found");
    }
    
    None
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

#[cfg(not(target_arch = "x86_64"))]
pub fn probe_wifi() -> Option<Arc<Mutex<dyn WifiDevice>>> {
    None
}

// =============================================================================
// WPA2-PSK Key Derivation and 4-Way Handshake
// =============================================================================

/// WPA2 key derivation module
pub mod wpa2 {
    use super::*;
    
    /// PSK length in bytes (256 bits)
    pub const PSK_LEN: usize = 32;
    /// PTK length for CCMP (16 bytes each for KCK, KEK, TK)
    pub const PTK_LEN: usize = 48;
    /// KCK length (Key Confirmation Key)
    pub const KCK_LEN: usize = 16;
    /// KEK length (Key Encryption Key) 
    pub const KEK_LEN: usize = 16;
    /// TK length (Temporal Key for CCMP)
    pub const TK_LEN: usize = 16;
    /// GTK maximum length
    pub const GTK_MAX_LEN: usize = 32;
    /// Nonce length
    pub const NONCE_LEN: usize = 32;
    /// MIC length
    pub const MIC_LEN: usize = 16;
    
    /// EAPOL-Key frame types
    pub mod eapol_types {
        pub const EAP_PACKET: u8 = 0;
        pub const EAPOL_START: u8 = 1;
        pub const EAPOL_LOGOFF: u8 = 2;
        pub const EAPOL_KEY: u8 = 3;
        pub const EAPOL_ASF_ALERT: u8 = 4;
    }
    
    /// EAPOL-Key descriptor types
    pub mod key_descriptor {
        pub const RC4: u8 = 1;  // WPA (deprecated)
        pub const RSN: u8 = 2;  // WPA2/RSN
    }
    
    /// 4-Way Handshake state
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HandshakeState {
        /// Waiting for Message 1 from AP
        WaitingMsg1,
        /// Sent Message 2, waiting for Message 3
        WaitingMsg3,
        /// Sent Message 4, handshake complete
        Complete,
        /// Handshake failed
        Failed,
    }
    
    /// EAPOL-Key frame
    #[derive(Debug, Clone)]
    pub struct EapolKey {
        /// Protocol version (1 for 802.1X-2001, 2 for 802.1X-2004)
        pub version: u8,
        /// Packet type (3 for EAPOL-Key)
        pub packet_type: u8,
        /// Body length
        pub body_length: u16,
        /// Descriptor type (1=RC4/WPA, 2=RSN/WPA2)
        pub descriptor_type: u8,
        /// Key information
        pub key_info: KeyInfo,
        /// Key length
        pub key_length: u16,
        /// Replay counter (64-bit)
        pub replay_counter: u64,
        /// Key nonce (32 bytes)
        pub key_nonce: [u8; NONCE_LEN],
        /// Key IV (16 bytes)
        pub key_iv: [u8; 16],
        /// Key RSC (8 bytes)
        pub key_rsc: [u8; 8],
        /// Reserved (8 bytes)
        pub reserved: [u8; 8],
        /// Key MIC (16 bytes)
        pub key_mic: [u8; MIC_LEN],
        /// Key data length
        pub key_data_length: u16,
        /// Key data
        pub key_data: Vec<u8>,
    }
    
    /// EAPOL-Key Information field
    #[derive(Debug, Clone, Copy)]
    pub struct KeyInfo {
        /// Key descriptor version (1=HMAC-MD5+ARC4, 2=HMAC-SHA1+AES)
        pub key_descriptor_version: u8,
        /// Key type (0=Group, 1=Pairwise)
        pub key_type: bool,
        /// Install flag
        pub install: bool,
        /// Key ACK (set by Authenticator)
        pub key_ack: bool,
        /// Key MIC present
        pub key_mic: bool,
        /// Secure flag (PTK installed)
        pub secure: bool,
        /// Error flag
        pub error: bool,
        /// Request flag
        pub request: bool,
        /// Encrypted key data
        pub encrypted_key_data: bool,
        /// SMK message
        pub smk_message: bool,
    }
    
    impl KeyInfo {
        /// Parse from 2 bytes
        pub fn parse(value: u16) -> Self {
            Self {
                key_descriptor_version: (value & 0x0007) as u8,
                key_type: (value & 0x0008) != 0,
                install: (value & 0x0040) != 0,
                key_ack: (value & 0x0080) != 0,
                key_mic: (value & 0x0100) != 0,
                secure: (value & 0x0200) != 0,
                error: (value & 0x0400) != 0,
                request: (value & 0x0800) != 0,
                encrypted_key_data: (value & 0x1000) != 0,
                smk_message: (value & 0x2000) != 0,
            }
        }
        
        /// Convert to 2 bytes
        pub fn to_bytes(&self) -> u16 {
            let mut value: u16 = self.key_descriptor_version as u16 & 0x07;
            if self.key_type { value |= 0x0008; }
            if self.install { value |= 0x0040; }
            if self.key_ack { value |= 0x0080; }
            if self.key_mic { value |= 0x0100; }
            if self.secure { value |= 0x0200; }
            if self.error { value |= 0x0400; }
            if self.request { value |= 0x0800; }
            if self.encrypted_key_data { value |= 0x1000; }
            if self.smk_message { value |= 0x2000; }
            value
        }
        
        /// Check if this is Message 1 of 4-way handshake
        pub fn is_msg1(&self) -> bool {
            self.key_ack && !self.key_mic && !self.install && !self.secure
        }
        
        /// Check if this is Message 3 of 4-way handshake
        pub fn is_msg3(&self) -> bool {
            self.key_ack && self.key_mic && self.install && self.secure
        }
    }
    
    impl EapolKey {
        /// Parse EAPOL-Key frame from bytes
        pub fn parse(data: &[u8]) -> Option<Self> {
            if data.len() < 99 {
                return None;
            }
            
            let version = data[0];
            let packet_type = data[1];
            let body_length = u16::from_be_bytes([data[2], data[3]]);
            
            if packet_type != eapol_types::EAPOL_KEY {
                return None;
            }
            
            let descriptor_type = data[4];
            let key_info = KeyInfo::parse(u16::from_be_bytes([data[5], data[6]]));
            let key_length = u16::from_be_bytes([data[7], data[8]]);
            
            let replay_counter = u64::from_be_bytes([
                data[9], data[10], data[11], data[12],
                data[13], data[14], data[15], data[16]
            ]);
            
            let mut key_nonce = [0u8; NONCE_LEN];
            key_nonce.copy_from_slice(&data[17..49]);
            
            let mut key_iv = [0u8; 16];
            key_iv.copy_from_slice(&data[49..65]);
            
            let mut key_rsc = [0u8; 8];
            key_rsc.copy_from_slice(&data[65..73]);
            
            let mut reserved = [0u8; 8];
            reserved.copy_from_slice(&data[73..81]);
            
            let mut key_mic = [0u8; MIC_LEN];
            key_mic.copy_from_slice(&data[81..97]);
            
            let key_data_length = u16::from_be_bytes([data[97], data[98]]);
            
            let key_data = if key_data_length > 0 && data.len() >= 99 + key_data_length as usize {
                data[99..99 + key_data_length as usize].to_vec()
            } else {
                Vec::new()
            };
            
            Some(Self {
                version,
                packet_type,
                body_length,
                descriptor_type,
                key_info,
                key_length,
                replay_counter,
                key_nonce,
                key_iv,
                key_rsc,
                reserved,
                key_mic,
                key_data_length,
                key_data,
            })
        }
        
        /// Serialize to bytes
        pub fn to_bytes(&self) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(99 + self.key_data.len());
            
            bytes.push(self.version);
            bytes.push(self.packet_type);
            bytes.extend_from_slice(&self.body_length.to_be_bytes());
            bytes.push(self.descriptor_type);
            bytes.extend_from_slice(&self.key_info.to_bytes().to_be_bytes());
            bytes.extend_from_slice(&self.key_length.to_be_bytes());
            bytes.extend_from_slice(&self.replay_counter.to_be_bytes());
            bytes.extend_from_slice(&self.key_nonce);
            bytes.extend_from_slice(&self.key_iv);
            bytes.extend_from_slice(&self.key_rsc);
            bytes.extend_from_slice(&self.reserved);
            bytes.extend_from_slice(&self.key_mic);
            bytes.extend_from_slice(&self.key_data_length.to_be_bytes());
            bytes.extend_from_slice(&self.key_data);
            
            bytes
        }
    }
    
    /// Pairwise Transient Key (PTK) derivation result
    #[derive(Debug, Clone)]
    pub struct Ptk {
        /// Key Confirmation Key (used for MIC calculation)
        pub kck: [u8; KCK_LEN],
        /// Key Encryption Key (used for key data encryption)
        pub kek: [u8; KEK_LEN],
        /// Temporal Key (used for data encryption)
        pub tk: [u8; TK_LEN],
    }
    
    /// 4-Way Handshake context
    pub struct HandshakeContext {
        /// Current state
        pub state: HandshakeState,
        /// PSK (derived from password)
        pub psk: [u8; PSK_LEN],
        /// ANonce (from AP)
        pub anonce: [u8; NONCE_LEN],
        /// SNonce (generated locally)
        pub snonce: [u8; NONCE_LEN],
        /// AP MAC address
        pub ap_addr: MacAddress,
        /// Station MAC address
        pub sta_addr: MacAddress,
        /// Derived PTK
        pub ptk: Option<Ptk>,
        /// GTK (from AP)
        pub gtk: Option<Vec<u8>>,
        /// Replay counter
        pub replay_counter: u64,
    }
    
    impl HandshakeContext {
        /// Create new handshake context
        pub fn new(psk: [u8; PSK_LEN], ap_addr: MacAddress, sta_addr: MacAddress) -> Self {
            // Generate cryptographically secure SNonce using CSPRNG
            let snonce: [u8; NONCE_LEN] = crate::crypto::random::random_bytes();
            
            Self {
                state: HandshakeState::WaitingMsg1,
                psk,
                anonce: [0u8; NONCE_LEN],
                snonce,
                ap_addr,
                sta_addr,
                ptk: None,
                gtk: None,
                replay_counter: 0,
            }
        }
        
        /// Process Message 1 from AP
        pub fn process_msg1(&mut self, msg: &EapolKey) -> Option<EapolKey> {
            if self.state != HandshakeState::WaitingMsg1 {
                return None;
            }
            
            // Extract ANonce
            self.anonce = msg.key_nonce;
            self.replay_counter = msg.replay_counter;
            
            // Derive PTK
            self.ptk = Some(self.derive_ptk());
            
            // Build Message 2
            let msg2 = self.build_msg2();
            self.state = HandshakeState::WaitingMsg3;
            
            Some(msg2)
        }
        
        /// Process Message 3 from AP
        pub fn process_msg3(&mut self, msg: &EapolKey) -> Option<EapolKey> {
            if self.state != HandshakeState::WaitingMsg3 {
                return None;
            }
            
            // Verify replay counter increased
            if msg.replay_counter <= self.replay_counter {
                self.state = HandshakeState::Failed;
                return None;
            }
            
            self.replay_counter = msg.replay_counter;
            
            // MIC Verification:
            // 1. Zero out MIC field in received frame
            // 2. Compute HMAC-SHA1-128(KCK, EAPOL-Key frame)
            // 3. Compare with received MIC
            // Note: Actual crypto requires hmac-sha1 implementation
            
            // GTK Extraction:
            // 1. Decrypt key_data using KEK with AES-WRAP
            // 2. Parse GTK from decrypted data
            // 3. Install GTK for group traffic decryption
            // Note: Requires AES key unwrap implementation
            
            // Build Message 4
            let msg4 = self.build_msg4();
            self.state = HandshakeState::Complete;
            
            Some(msg4)
        }
        
        /// Derive PTK from PSK and nonces
        fn derive_ptk(&self) -> Ptk {
            // PTK = PRF-X(PMK, "Pairwise key expansion", 
            //             Min(AA,SPA) || Max(AA,SPA) || Min(ANonce,SNonce) || Max(ANonce,SNonce))
            // Where PMK = PSK for WPA2-PSK
            
            // Sort MAC addresses
            let (min_addr, max_addr) = if self.ap_addr.0 < self.sta_addr.0 {
                (self.ap_addr.0, self.sta_addr.0)
            } else {
                (self.sta_addr.0, self.ap_addr.0)
            };
            
            // Sort nonces
            let (min_nonce, max_nonce) = if self.anonce < self.snonce {
                (self.anonce, self.snonce)
            } else {
                (self.snonce, self.anonce)
            };
            
            // Build data for PRF
            let mut data = Vec::with_capacity(76);
            data.extend_from_slice(&min_addr);
            data.extend_from_slice(&max_addr);
            data.extend_from_slice(&min_nonce);
            data.extend_from_slice(&max_nonce);
            
            // PRF-384 using HMAC-SHA1 as per IEEE 802.11i-2004
            let ptk_bytes = simple_prf(&self.psk, b"Pairwise key expansion", &data, PTK_LEN);
            
            let mut kck = [0u8; KCK_LEN];
            let mut kek = [0u8; KEK_LEN];
            let mut tk = [0u8; TK_LEN];
            
            kck.copy_from_slice(&ptk_bytes[0..16]);
            kek.copy_from_slice(&ptk_bytes[16..32]);
            tk.copy_from_slice(&ptk_bytes[32..48]);
            
            Ptk { kck, kek, tk }
        }
        
        /// Build Message 2 (Station -> AP)
        fn build_msg2(&self) -> EapolKey {
            EapolKey {
                version: 2,
                packet_type: eapol_types::EAPOL_KEY,
                body_length: 95, // Without key data
                descriptor_type: key_descriptor::RSN,
                key_info: KeyInfo {
                    key_descriptor_version: 2, // HMAC-SHA1-128 + AES-128-CCMP
                    key_type: true, // Pairwise
                    install: false,
                    key_ack: false,
                    key_mic: true,
                    secure: false,
                    error: false,
                    request: false,
                    encrypted_key_data: false,
                    smk_message: false,
                },
                key_length: 16, // CCMP key length
                replay_counter: self.replay_counter,
                key_nonce: self.snonce,
                key_iv: [0u8; 16],
                key_rsc: [0u8; 8],
                reserved: [0u8; 8],
                key_mic: [0u8; MIC_LEN], // Will be computed
                key_data_length: 0,
                key_data: Vec::new(),
            }
        }
        
        /// Build Message 4 (Station -> AP)
        fn build_msg4(&self) -> EapolKey {
            EapolKey {
                version: 2,
                packet_type: eapol_types::EAPOL_KEY,
                body_length: 95,
                descriptor_type: key_descriptor::RSN,
                key_info: KeyInfo {
                    key_descriptor_version: 2,
                    key_type: true, // Pairwise
                    install: false,
                    key_ack: false,
                    key_mic: true,
                    secure: true, // PTK installed
                    error: false,
                    request: false,
                    encrypted_key_data: false,
                    smk_message: false,
                },
                key_length: 16,
                replay_counter: self.replay_counter,
                key_nonce: [0u8; NONCE_LEN],
                key_iv: [0u8; 16],
                key_rsc: [0u8; 8],
                reserved: [0u8; 8],
                key_mic: [0u8; MIC_LEN], // Will be computed
                key_data_length: 0,
                key_data: Vec::new(),
            }
        }
    }
    
    /// PRF-X function based on HMAC-SHA1 (IEEE 802.11i)
    /// PRF-X(K, A, B) = HMAC-SHA1(K, A || 0 || B || i) for i = 0,1,2...
    fn simple_prf(key: &[u8], label: &[u8], data: &[u8], output_len: usize) -> Vec<u8> {
        use crate::crypto::mac::{Mac, HmacSha1};
        
        let mut output = Vec::with_capacity(output_len);
        let mut counter = 0u8;
        
        while output.len() < output_len {
            // Build message: A || 0x00 || B || counter
            let mut message = Vec::new();
            message.extend_from_slice(label);
            message.push(0); // Null separator
            message.extend_from_slice(data);
            message.push(counter);
            
            // Compute HMAC-SHA1
            let block = HmacSha1::mac(key, &message);
            
            output.extend_from_slice(&block);
            counter += 1;
        }
        
        output.truncate(output_len);
        output
    }
    
    /// Derive PSK from passphrase and SSID using PBKDF2-HMAC-SHA1
    /// PSK = PBKDF2(passphrase, SSID, 4096, 256 bits)
    pub fn derive_psk(passphrase: &str, ssid: &str) -> [u8; PSK_LEN] {
        use crate::crypto::kdf::Pbkdf2;
        
        let mut psk = [0u8; PSK_LEN];
        
        // Use proper PBKDF2-HMAC-SHA1 with 4096 iterations
        if let Ok(result) = Pbkdf2::derive_sha1(
            passphrase.as_bytes(),
            ssid.as_bytes(),
            4096,
            PSK_LEN,
        ) {
            let copy_len = core::cmp::min(result.len(), PSK_LEN);
            psk[..copy_len].copy_from_slice(&result[..copy_len]);
        }
        
        psk
    }
}

/// Initialize WiFi subsystem
pub fn init() {
    if let Some(device) = probe_wifi() {
        *WIFI_DEVICE.lock() = Some(device);
    }
}

/// Helper to convert signal strength to quality percentage
pub fn signal_to_quality(dbm: i8) -> u8 {
    // -30 dBm or better = 100%
    // -90 dBm or worse = 0%
    if dbm >= -30 { 100 }
    else if dbm <= -90 { 0 }
    else { ((dbm + 90) * 100 / 60) as u8 }
}

/// Helper to get channel from frequency
pub fn frequency_to_channel(freq_mhz: u16) -> u8 {
    match freq_mhz {
        // 2.4 GHz band
        2412 => 1,
        2417 => 2,
        2422 => 3,
        2427 => 4,
        2432 => 5,
        2437 => 6,
        2442 => 7,
        2447 => 8,
        2452 => 9,
        2457 => 10,
        2462 => 11,
        2467 => 12,
        2472 => 13,
        2484 => 14,
        // 5 GHz band (common channels)
        5180 => 36,
        5200 => 40,
        5220 => 44,
        5240 => 48,
        5260 => 52,
        5280 => 56,
        5300 => 60,
        5320 => 64,
        5500 => 100,
        5520 => 104,
        5540 => 108,
        5560 => 112,
        5580 => 116,
        5600 => 120,
        5620 => 124,
        5640 => 128,
        5660 => 132,
        5680 => 136,
        5700 => 140,
        5720 => 144,
        5745 => 149,
        5765 => 153,
        5785 => 157,
        5805 => 161,
        5825 => 165,
        _ => 0,
    }
}
