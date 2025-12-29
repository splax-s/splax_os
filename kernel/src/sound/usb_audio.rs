//! USB Audio Class (UAC) Driver
//!
//! Implements USB Audio Class 1.0 and 2.0 support for USB audio devices
//! including headsets, microphones, DACs, and sound cards.
//!
//! USB Audio Architecture:
//! - Audio Control Interface: Manages device topology
//! - Audio Streaming Interface: Handles audio data transfer
//! - Endpoints: Isochronous for audio, interrupt for feedback
//!
//! Supported features:
//! - UAC 1.0 (USB Audio Class specification 1.0)
//! - UAC 2.0 (USB Audio Class specification 2.0)
//! - Isochronous transfers for low-latency audio
//! - Volume/mute control
//! - Sample rate switching

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    AudioDevice, AudioError, DeviceCapabilities, DeviceId, DeviceInfo, DeviceType,
    SampleFormat, StreamConfig, StreamDirection, StreamId, StreamState,
};

// ============================================================================
// USB Audio Class Constants
// ============================================================================

/// Audio class code
pub const USB_CLASS_AUDIO: u8 = 0x01;

/// Audio subclass codes
pub mod subclass {
    /// Undefined
    pub const UNDEFINED: u8 = 0x00;
    /// Audio Control
    pub const AUDIO_CONTROL: u8 = 0x01;
    /// Audio Streaming
    pub const AUDIO_STREAMING: u8 = 0x02;
    /// MIDI Streaming
    pub const MIDI_STREAMING: u8 = 0x03;
}

/// Audio protocol codes
pub mod protocol {
    /// Undefined (UAC 1.0)
    pub const PR_PROTOCOL_UNDEFINED: u8 = 0x00;
    /// IP version 2.0 (UAC 2.0)
    pub const IP_VERSION_02_00: u8 = 0x20;
    /// IP version 3.0 (UAC 3.0)
    pub const IP_VERSION_03_00: u8 = 0x30;
}

/// Audio Control interface descriptor subtypes
pub mod ac_descriptor {
    /// Undefined
    pub const AC_DESCRIPTOR_UNDEFINED: u8 = 0x00;
    /// Header
    pub const HEADER: u8 = 0x01;
    /// Input Terminal
    pub const INPUT_TERMINAL: u8 = 0x02;
    /// Output Terminal
    pub const OUTPUT_TERMINAL: u8 = 0x03;
    /// Mixer Unit
    pub const MIXER_UNIT: u8 = 0x04;
    /// Selector Unit
    pub const SELECTOR_UNIT: u8 = 0x05;
    /// Feature Unit
    pub const FEATURE_UNIT: u8 = 0x06;
    /// Processing Unit (UAC1) / Effect Unit (UAC2)
    pub const PROCESSING_UNIT: u8 = 0x07;
    /// Extension Unit
    pub const EXTENSION_UNIT: u8 = 0x08;
    /// Clock Source (UAC2)
    pub const CLOCK_SOURCE: u8 = 0x0A;
    /// Clock Selector (UAC2)
    pub const CLOCK_SELECTOR: u8 = 0x0B;
    /// Clock Multiplier (UAC2)
    pub const CLOCK_MULTIPLIER: u8 = 0x0C;
    /// Sample Rate Converter (UAC2)
    pub const SAMPLE_RATE_CONVERTER: u8 = 0x0D;
}

/// Audio Streaming interface descriptor subtypes
pub mod as_descriptor {
    /// Undefined
    pub const AS_DESCRIPTOR_UNDEFINED: u8 = 0x00;
    /// General
    pub const AS_GENERAL: u8 = 0x01;
    /// Format Type
    pub const FORMAT_TYPE: u8 = 0x02;
    /// Format Specific (UAC1) / Encoder (UAC2)
    pub const FORMAT_SPECIFIC: u8 = 0x03;
    /// Decoder (UAC2)
    pub const DECODER: u8 = 0x04;
}

/// Terminal types
pub mod terminal_type {
    // USB Terminal Types
    /// USB Undefined
    pub const USB_UNDEFINED: u16 = 0x0100;
    /// USB Streaming
    pub const USB_STREAMING: u16 = 0x0101;
    /// USB Vendor Specific
    pub const USB_VENDOR_SPECIFIC: u16 = 0x01FF;

    // Input Terminal Types
    /// Undefined Input
    pub const INPUT_UNDEFINED: u16 = 0x0200;
    /// Microphone
    pub const MICROPHONE: u16 = 0x0201;
    /// Desktop Microphone
    pub const DESKTOP_MICROPHONE: u16 = 0x0202;
    /// Personal Microphone
    pub const PERSONAL_MICROPHONE: u16 = 0x0203;
    /// Omni-directional Microphone
    pub const OMNI_MICROPHONE: u16 = 0x0204;
    /// Microphone Array
    pub const MICROPHONE_ARRAY: u16 = 0x0205;
    /// Processing Microphone Array
    pub const PROCESSING_MICROPHONE_ARRAY: u16 = 0x0206;

    // Output Terminal Types
    /// Undefined Output
    pub const OUTPUT_UNDEFINED: u16 = 0x0300;
    /// Speaker
    pub const SPEAKER: u16 = 0x0301;
    /// Headphones
    pub const HEADPHONES: u16 = 0x0302;
    /// Head Mounted Display
    pub const HEAD_MOUNTED_DISPLAY: u16 = 0x0303;
    /// Desktop Speaker
    pub const DESKTOP_SPEAKER: u16 = 0x0304;
    /// Room Speaker
    pub const ROOM_SPEAKER: u16 = 0x0305;
    /// Communication Speaker
    pub const COMMUNICATION_SPEAKER: u16 = 0x0306;
    /// Low Frequency Speaker
    pub const LFE_SPEAKER: u16 = 0x0307;
}

/// Feature Unit control selectors
pub mod feature_control {
    /// Mute
    pub const MUTE: u8 = 0x01;
    /// Volume
    pub const VOLUME: u8 = 0x02;
    /// Bass
    pub const BASS: u8 = 0x03;
    /// Mid
    pub const MID: u8 = 0x04;
    /// Treble
    pub const TREBLE: u8 = 0x05;
    /// Graphic Equalizer
    pub const GRAPHIC_EQUALIZER: u8 = 0x06;
    /// Automatic Gain Control
    pub const AUTOMATIC_GAIN: u8 = 0x07;
    /// Delay
    pub const DELAY: u8 = 0x08;
    /// Bass Boost
    pub const BASS_BOOST: u8 = 0x09;
    /// Loudness
    pub const LOUDNESS: u8 = 0x0A;
}

/// Audio class-specific requests
pub mod request {
    /// Set Current
    pub const SET_CUR: u8 = 0x01;
    /// Set Minimum
    pub const SET_MIN: u8 = 0x02;
    /// Set Maximum
    pub const SET_MAX: u8 = 0x03;
    /// Set Resolution
    pub const SET_RES: u8 = 0x04;
    /// Get Current
    pub const GET_CUR: u8 = 0x81;
    /// Get Minimum
    pub const GET_MIN: u8 = 0x82;
    /// Get Maximum
    pub const GET_MAX: u8 = 0x83;
    /// Get Resolution
    pub const GET_RES: u8 = 0x84;
    /// Set Memory
    pub const SET_MEM: u8 = 0x05;
    /// Get Memory
    pub const GET_MEM: u8 = 0x85;
}

/// Format type codes
pub mod format_type {
    /// Undefined
    pub const FORMAT_TYPE_UNDEFINED: u8 = 0x00;
    /// Type I (PCM)
    pub const FORMAT_TYPE_I: u8 = 0x01;
    /// Type II (Compressed)
    pub const FORMAT_TYPE_II: u8 = 0x02;
    /// Type III (Non-PCM)
    pub const FORMAT_TYPE_III: u8 = 0x03;
    /// Type IV (Extended)
    pub const FORMAT_TYPE_IV: u8 = 0x04;
    /// Extended Type I
    pub const EXT_FORMAT_TYPE_I: u8 = 0x81;
    /// Extended Type II
    pub const EXT_FORMAT_TYPE_II: u8 = 0x82;
    /// Extended Type III
    pub const EXT_FORMAT_TYPE_III: u8 = 0x83;
}

/// Format type I bit resolutions
pub mod format_type_i {
    /// PCM
    pub const PCM: u16 = 0x0001;
    /// PCM8 (8-bit)
    pub const PCM8: u16 = 0x0002;
    /// IEEE Float
    pub const IEEE_FLOAT: u16 = 0x0003;
    /// A-Law
    pub const ALAW: u16 = 0x0004;
    /// μ-Law
    pub const MULAW: u16 = 0x0005;
}

// ============================================================================
// USB Audio Descriptors
// ============================================================================

/// Audio Control Header Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AcHeaderDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub bcd_adc: u16,
    pub total_length: u16,
    pub in_collection: u8,
    // Followed by interface numbers
}

/// Input Terminal Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct InputTerminalDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub terminal_id: u8,
    pub terminal_type: u16,
    pub assoc_terminal: u8,
    pub nr_channels: u8,
    pub channel_config: u16,
    pub channel_names: u8,
    pub terminal: u8,
}

/// Output Terminal Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct OutputTerminalDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub terminal_id: u8,
    pub terminal_type: u16,
    pub assoc_terminal: u8,
    pub source_id: u8,
    pub terminal: u8,
}

/// Feature Unit Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct FeatureUnitDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub unit_id: u8,
    pub source_id: u8,
    pub control_size: u8,
    // Followed by control bytes
}

/// Audio Streaming General Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AsGeneralDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub terminal_link: u8,
    pub delay: u8,
    pub format_tag: u16,
}

/// Format Type I Descriptor (UAC 1.0)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct FormatTypeIDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub descriptor_subtype: u8,
    pub format_type: u8,
    pub nr_channels: u8,
    pub subframe_size: u8,
    pub bit_resolution: u8,
    pub sample_freq_type: u8,
    // Followed by sample frequencies (3 bytes each)
}

// ============================================================================
// USB Audio Device Model
// ============================================================================

/// Audio Terminal
#[derive(Debug, Clone)]
pub struct AudioTerminal {
    /// Terminal ID
    pub id: u8,
    /// Terminal type
    pub terminal_type: u16,
    /// Number of channels
    pub channels: u8,
    /// Channel configuration
    pub channel_config: u16,
    /// Is input terminal
    pub is_input: bool,
}

/// Feature Unit
#[derive(Debug, Clone)]
pub struct FeatureUnit {
    /// Unit ID
    pub id: u8,
    /// Source ID
    pub source_id: u8,
    /// Controls per channel (bitmap)
    pub controls: Vec<u8>,
    /// Has mute control
    pub has_mute: bool,
    /// Has volume control
    pub has_volume: bool,
}

/// Audio Streaming Format
#[derive(Debug, Clone)]
pub struct AudioFormat {
    /// Format type
    pub format_type: u8,
    /// Number of channels
    pub channels: u8,
    /// Subframe size (bytes per sample per channel)
    pub subframe_size: u8,
    /// Bit resolution
    pub bit_resolution: u8,
    /// Supported sample rates
    pub sample_rates: Vec<u32>,
}

/// Unit type for audio topology
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitType {
    /// Input terminal
    InputTerminal,
    /// Output terminal
    OutputTerminal,
    /// Feature unit (volume, mute, etc.)
    FeatureUnit,
    /// Mixer unit
    MixerUnit,
    /// Selector unit
    SelectorUnit,
    /// Processing unit
    ProcessingUnit,
    /// Extension unit
    ExtensionUnit,
}

/// Audio unit in the device topology
#[derive(Debug, Clone)]
pub struct AudioUnit {
    /// Unit ID
    pub unit_id: u8,
    /// Unit type
    pub unit_type: UnitType,
    /// Terminal type (for terminals)
    pub terminal_type: u16,
    /// Number of channels
    pub num_channels: u8,
    /// Source IDs this unit connects to
    pub source_ids: Vec<u8>,
}

/// Stream format descriptor
#[derive(Debug, Clone)]
pub struct StreamFormat {
    /// Interface number
    pub interface: u8,
    /// Terminal link
    pub terminal_link: u8,
    /// Format tag
    pub format_tag: u16,
    /// Number of channels
    pub channels: u8,
    /// Sample size in bits
    pub sample_size: u8,
    /// Supported sample rates
    pub sample_rates: Vec<u32>,
    /// Endpoint address
    pub endpoint: u8,
}

/// USB Audio Stream
pub struct UsbAudioStream {
    /// Stream ID
    id: StreamId,
    /// Direction
    direction: StreamDirection,
    /// Interface number
    interface: u8,
    /// Alternate setting
    alternate: u8,
    /// Endpoint address
    endpoint: u8,
    /// Max packet size
    max_packet_size: u16,
    /// Current configuration
    config: StreamConfig,
    /// Current state
    state: StreamState,
    /// Audio format
    format: AudioFormat,
    /// Audio buffer
    buffer: Vec<u8>,
    /// Buffer write position
    buffer_write_pos: usize,
    /// Buffer read position
    buffer_read_pos: usize,
    /// Buffer position (legacy)
    buffer_pos: usize,
    /// Bytes per frame
    bytes_per_frame: usize,
    /// Whether stream is active
    active: bool,
}

impl UsbAudioStream {
    /// Create a new USB audio stream
    pub fn new(id: StreamId, direction: StreamDirection, interface: u8) -> Self {
        Self {
            id,
            direction,
            interface,
            alternate: 0,
            endpoint: 0,
            max_packet_size: 0,
            config: StreamConfig::cd_quality(),
            state: StreamState::Stopped,
            format: AudioFormat {
                format_type: format_type::FORMAT_TYPE_I,
                channels: 2,
                subframe_size: 2,
                bit_resolution: 16,
                sample_rates: Vec::new(),
            },
            buffer: Vec::new(),
            buffer_write_pos: 0,
            buffer_read_pos: 0,
            buffer_pos: 0,
            bytes_per_frame: 0,
            active: false,
        }
    }
}

// ============================================================================
// USB Audio Device
// ============================================================================

/// USB Audio Class version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UacVersion {
    /// UAC 1.0
    Uac1,
    /// UAC 2.0
    Uac2,
    /// UAC 3.0
    Uac3,
}

/// USB Audio Device
pub struct UsbAudioDevice {
    /// Device ID
    device_id: DeviceId,
    /// USB device address
    usb_address: u8,
    /// USB device address (alias for usb_address)
    device_address: u8,
    /// UAC version
    uac_version: UacVersion,
    /// Vendor ID
    vendor_id: u16,
    /// Product ID
    product_id: u16,
    /// Device name
    name: String,
    /// Input terminals
    input_terminals: Vec<AudioTerminal>,
    /// Output terminals
    output_terminals: Vec<AudioTerminal>,
    /// Feature units
    feature_units: Vec<FeatureUnit>,
    /// Audio units in the topology
    units: Vec<AudioUnit>,
    /// Stream formats
    stream_formats: Vec<StreamFormat>,
    /// Active streams (for isochronous transfer tracking)
    streams: Vec<UsbAudioStream>,
    /// Control interface number
    control_interface: u8,
    /// Feature unit ID for volume control
    feature_unit: Option<u8>,
    /// Playback streams
    playback_streams: Vec<UsbAudioStream>,
    /// Capture streams
    capture_streams: Vec<UsbAudioStream>,
    /// Next stream ID
    next_stream_id: AtomicU32,
    /// Is initialized
    initialized: bool,
    /// Current volume (0-100)
    volume: u8,
    /// Mute state
    muted: bool,
}

impl UsbAudioDevice {
    /// Create a new USB audio device
    pub fn new(
        device_id: DeviceId,
        usb_address: u8,
        vendor_id: u16,
        product_id: u16,
        name: String,
    ) -> Self {
        Self {
            device_id,
            usb_address,
            device_address: usb_address,
            uac_version: UacVersion::Uac1,
            vendor_id,
            product_id,
            name,
            input_terminals: Vec::new(),
            output_terminals: Vec::new(),
            feature_units: Vec::new(),
            units: Vec::new(),
            stream_formats: Vec::new(),
            streams: Vec::new(),
            control_interface: 0,
            feature_unit: None,
            playback_streams: Vec::new(),
            capture_streams: Vec::new(),
            next_stream_id: AtomicU32::new(1),
            initialized: false,
            volume: 100,
            muted: false,
        }
    }

    /// Parse audio control descriptors
    fn parse_ac_descriptors(&mut self, descriptors: &[u8]) -> Result<(), AudioError> {
        let mut offset = 0;
        
        while offset < descriptors.len() {
            if offset + 2 > descriptors.len() {
                break;
            }
            
            let length = descriptors[offset] as usize;
            if length < 2 || offset + length > descriptors.len() {
                break;
            }
            
            let desc_subtype = descriptors[offset + 2];
            
            match desc_subtype {
                ac_descriptor::HEADER => {
                    // Parse AC header - contains total length and interface numbers
                    if length >= 8 {
                        let total_length = u16::from_le_bytes([
                            descriptors[offset + 5],
                            descriptors[offset + 6],
                        ]);
                        let num_interfaces = descriptors[offset + 7];
                        
                        #[cfg(target_arch = "x86_64")]
                        {
                            use core::fmt::Write;
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial, "[usb-audio] AC Header: {} bytes, {} interfaces", 
                                    total_length, num_interfaces);
                            }
                        }
                    }
                }
                ac_descriptor::INPUT_TERMINAL => {
                    // Input terminal (microphone, line in, etc.)
                    if length >= 12 {
                        let terminal_id = descriptors[offset + 3];
                        let terminal_type = u16::from_le_bytes([
                            descriptors[offset + 4],
                            descriptors[offset + 5],
                        ]);
                        let num_channels = descriptors[offset + 7];
                        
                        self.units.push(AudioUnit {
                            unit_id: terminal_id,
                            unit_type: UnitType::InputTerminal,
                            terminal_type,
                            num_channels,
                            source_ids: Vec::new(),
                        });
                    }
                }
                ac_descriptor::OUTPUT_TERMINAL => {
                    // Output terminal (speaker, headphone, etc.)
                    if length >= 9 {
                        let terminal_id = descriptors[offset + 3];
                        let terminal_type = u16::from_le_bytes([
                            descriptors[offset + 4],
                            descriptors[offset + 5],
                        ]);
                        let source_id = descriptors[offset + 7];
                        
                        self.units.push(AudioUnit {
                            unit_id: terminal_id,
                            unit_type: UnitType::OutputTerminal,
                            terminal_type,
                            num_channels: 0,
                            source_ids: alloc::vec![source_id],
                        });
                    }
                }
                ac_descriptor::FEATURE_UNIT => {
                    // Feature unit (volume, mute, etc.)
                    if length >= 7 {
                        let unit_id = descriptors[offset + 3];
                        let source_id = descriptors[offset + 4];
                        
                        self.units.push(AudioUnit {
                            unit_id,
                            unit_type: UnitType::FeatureUnit,
                            terminal_type: 0,
                            num_channels: 0,
                            source_ids: alloc::vec![source_id],
                        });
                        
                        // Store feature unit for volume control
                        self.feature_unit = Some(unit_id);
                    }
                }
                ac_descriptor::MIXER_UNIT => {
                    // Mixer unit
                    if length >= 10 {
                        let unit_id = descriptors[offset + 3];
                        let num_inputs = descriptors[offset + 4] as usize;
                        let mut source_ids = Vec::new();
                        
                        for i in 0..num_inputs {
                            if offset + 5 + i < descriptors.len() {
                                source_ids.push(descriptors[offset + 5 + i]);
                            }
                        }
                        
                        self.units.push(AudioUnit {
                            unit_id,
                            unit_type: UnitType::MixerUnit,
                            terminal_type: 0,
                            num_channels: 0,
                            source_ids,
                        });
                    }
                }
                _ => {
                    // Other descriptors - skip
                }
            }
            
            offset += length;
        }
        
        Ok(())
    }

    /// Parse audio streaming descriptors
    fn parse_as_descriptors(&mut self, interface: u8, descriptors: &[u8]) -> Result<(), AudioError> {
        let mut offset = 0;
        let mut current_format: Option<StreamFormat> = None;
        
        while offset < descriptors.len() {
            if offset + 2 > descriptors.len() {
                break;
            }
            
            let length = descriptors[offset] as usize;
            if length < 2 || offset + length > descriptors.len() {
                break;
            }
            
            let desc_type = descriptors[offset + 1];
            
            // Audio Streaming interface descriptor (CS_INTERFACE)
            if desc_type == 0x24 && length >= 7 {
                let subtype = descriptors[offset + 2];
                
                match subtype {
                    0x01 => {
                        // AS_GENERAL
                        let terminal_link = descriptors[offset + 3];
                        let format_tag = u16::from_le_bytes([
                            descriptors[offset + 5],
                            descriptors[offset + 6],
                        ]);
                        
                        current_format = Some(StreamFormat {
                            interface,
                            terminal_link,
                            format_tag,
                            channels: 2,
                            sample_size: 16,
                            sample_rates: Vec::new(),
                            endpoint: 0,
                        });
                    }
                    0x02 => {
                        // FORMAT_TYPE
                        if let Some(ref mut fmt) = current_format {
                            if length >= 8 {
                                fmt.channels = descriptors[offset + 4];
                                fmt.sample_size = descriptors[offset + 6];
                                
                                // Parse sample rates (discrete or continuous)
                                let freq_type = descriptors[offset + 7];
                                if freq_type == 0 {
                                    // Continuous - min and max
                                    if length >= 14 {
                                        let min_freq = u32::from_le_bytes([
                                            descriptors[offset + 8],
                                            descriptors[offset + 9],
                                            descriptors[offset + 10],
                                            0,
                                        ]);
                                        let max_freq = u32::from_le_bytes([
                                            descriptors[offset + 11],
                                            descriptors[offset + 12],
                                            descriptors[offset + 13],
                                            0,
                                        ]);
                                        // Add common rates in range
                                        for rate in [8000, 11025, 16000, 22050, 32000, 44100, 48000, 96000] {
                                            if rate >= min_freq && rate <= max_freq {
                                                fmt.sample_rates.push(rate);
                                            }
                                        }
                                    }
                                } else {
                                    // Discrete rates
                                    for i in 0..freq_type as usize {
                                        let rate_offset = offset + 8 + i * 3;
                                        if rate_offset + 3 <= descriptors.len() {
                                            let rate = u32::from_le_bytes([
                                                descriptors[rate_offset],
                                                descriptors[rate_offset + 1],
                                                descriptors[rate_offset + 2],
                                                0,
                                            ]);
                                            fmt.sample_rates.push(rate);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            
            // Endpoint descriptor
            if desc_type == 0x05 && length >= 7 {
                let ep_addr = descriptors[offset + 2];
                if let Some(ref mut fmt) = current_format {
                    fmt.endpoint = ep_addr;
                }
            }
            
            offset += length;
        }
        
        // Store the format if valid
        if let Some(fmt) = current_format {
            if fmt.endpoint != 0 && !fmt.sample_rates.is_empty() {
                self.stream_formats.push(fmt);
            }
        }
        
        Ok(())
    }

    /// Send control request to audio unit
    fn control_request(
        &self,
        request: u8,
        unit_id: u8,
        control: u8,
        channel: u8,
        data: &[u8],
    ) -> Result<Vec<u8>, AudioError> {
        use crate::usb::{USB_SUBSYSTEM, SetupPacket};
        
        let is_get = request == request::GET_CUR || request == request::GET_MIN 
                  || request == request::GET_MAX || request == request::GET_RES;
        
        let request_type = if is_get {
            0xA1 // Device-to-host, class, interface
        } else {
            0x21 // Host-to-device, class, interface
        };
        
        // wValue = control selector (high) | channel number (low)
        let w_value = ((control as u16) << 8) | (channel as u16);
        // wIndex = unit ID (high) | interface (low)
        let w_index = ((unit_id as u16) << 8) | (self.control_interface as u16);
        
        let setup = SetupPacket {
            request_type,
            request,
            value: w_value,
            index: w_index,
            length: if is_get { 2 } else { data.len() as u16 },
        };
        
        let mut usb_guard = USB_SUBSYSTEM.lock();
        let usb = usb_guard.as_mut().ok_or(AudioError::DeviceError)?;
        
        if is_get {
            let mut response = alloc::vec![0u8; 2];
            match usb.control_transfer(self.device_address, setup, Some(&mut response)) {
                crate::usb::TransferResult::Success(len) => {
                    response.truncate(len);
                    Ok(response)
                }
                _ => Err(AudioError::DeviceError),
            }
        } else {
            // For SET requests, we need to send data
            // The USB stack takes mutable reference, so clone the data
            let mut send_data = data.to_vec();
            match usb.control_transfer(self.device_address, setup, Some(&mut send_data)) {
                crate::usb::TransferResult::Success(_) => Ok(Vec::new()),
                _ => Err(AudioError::DeviceError),
            }
        }
    }

    /// Get volume from feature unit
    fn get_volume_from_unit(&self, unit_id: u8, channel: u8) -> Result<i16, AudioError> {
        let data = self.control_request(
            request::GET_CUR,
            unit_id,
            feature_control::VOLUME,
            channel,
            &[],
        )?;
        
        if data.len() >= 2 {
            Ok(i16::from_le_bytes([data[0], data[1]]))
        } else {
            Err(AudioError::InvalidConfig)
        }
    }

    /// Set volume on feature unit
    fn set_volume_on_unit(&self, unit_id: u8, channel: u8, volume: i16) -> Result<(), AudioError> {
        let data = volume.to_le_bytes();
        self.control_request(
            request::SET_CUR,
            unit_id,
            feature_control::VOLUME,
            channel,
            &data,
        )?;
        Ok(())
    }

    /// Get mute from feature unit
    fn get_mute_from_unit(&self, unit_id: u8, channel: u8) -> Result<bool, AudioError> {
        let data = self.control_request(
            request::GET_CUR,
            unit_id,
            feature_control::MUTE,
            channel,
            &[],
        )?;
        
        if !data.is_empty() {
            Ok(data[0] != 0)
        } else {
            Err(AudioError::InvalidConfig)
        }
    }

    /// Set mute on feature unit
    fn set_mute_on_unit(&self, unit_id: u8, channel: u8, mute: bool) -> Result<(), AudioError> {
        let data = [if mute { 1u8 } else { 0u8 }];
        self.control_request(
            request::SET_CUR,
            unit_id,
            feature_control::MUTE,
            channel,
            &data,
        )?;
        Ok(())
    }

    /// Set sample rate (UAC 1.0)
    fn set_sample_rate_uac1(&self, endpoint: u8, rate: u32) -> Result<(), AudioError> {
        use crate::usb::{USB_SUBSYSTEM, SetupPacket};
        
        // UAC 1.0 uses endpoint SET_CUR for sample rate
        // Frequency is 3 bytes, little endian
        let freq_bytes = [
            (rate & 0xFF) as u8,
            ((rate >> 8) & 0xFF) as u8,
            ((rate >> 16) & 0xFF) as u8,
        ];
        
        let setup = SetupPacket {
            request_type: 0x22, // Host-to-device, class, endpoint
            request: request::SET_CUR,
            value: 0x0100, // Sampling Frequency Control
            index: endpoint as u16,
            length: 3,
        };
        
        let mut data = freq_bytes.to_vec();
        let mut usb_guard = USB_SUBSYSTEM.lock();
        let usb = usb_guard.as_mut().ok_or(AudioError::DeviceError)?;
        match usb.control_transfer(self.device_address, setup, Some(&mut data)) {
            crate::usb::TransferResult::Success(_) => Ok(()),
            _ => Err(AudioError::DeviceError),
        }
    }

    /// Set sample rate (UAC 2.0)
    fn set_sample_rate_uac2(&self, clock_id: u8, rate: u32) -> Result<(), AudioError> {
        // UAC 2.0 uses clock source control
        let rate_bytes = rate.to_le_bytes();
        self.control_request(
            request::SET_CUR,
            clock_id,
            0x01, // CS_SAM_FREQ_CONTROL
            0,
            &rate_bytes,
        )?;
        Ok(())
    }

    /// Select alternate setting for streaming interface
    fn select_alternate(&self, interface: u8, alternate: u8) -> Result<(), AudioError> {
        use crate::usb::{USB_SUBSYSTEM, SetupPacket};
        
        // USB SET_INTERFACE request
        let setup = SetupPacket {
            request_type: 0x01, // Host-to-device, standard, interface
            request: 0x0B, // SET_INTERFACE
            value: alternate as u16,
            index: interface as u16,
            length: 0,
        };
        
        let mut usb_guard = USB_SUBSYSTEM.lock();
        let usb = usb_guard.as_mut().ok_or(AudioError::DeviceError)?;
        match usb.control_transfer(self.device_address, setup, None) {
            crate::usb::TransferResult::Success(_) => Ok(()),
            _ => Err(AudioError::DeviceError),
        }
    }

    /// Start isochronous transfer
    fn start_isoc_transfer(&mut self, endpoint: u8) -> Result<(), AudioError> {
        // Calculate buffer size based on sample rate and format
        // bytes_per_frame = sample_rate / 1000 * channels * bytes_per_sample
        // USB sends 1 packet per frame (1ms for full-speed, 125µs for high-speed)
        
        let sample_rate = 48000u32;
        let channels = 2u32;
        let bytes_per_sample = 2u32; // 16-bit
        let bytes_per_frame = sample_rate * channels * bytes_per_sample / 1000;
        
        // Allocate ring buffer for audio data (enough for ~50ms)
        let buffer_frames = 50;
        let buffer_size = (bytes_per_frame as usize) * buffer_frames;
        
        // Store buffer info in the stream state
        if let Some(stream) = self.streams.iter_mut().find(|s| s.endpoint == endpoint) {
            stream.buffer = alloc::vec![0u8; buffer_size];
            stream.buffer_pos = 0;
            stream.bytes_per_frame = bytes_per_frame as usize;
            stream.active = true;
        }
        
        // Enable the endpoint by selecting the appropriate alternate setting
        // (alternate 0 is typically zero-bandwidth, alternate 1+ have actual endpoints)
        if let Some(fmt) = self.stream_formats.iter().find(|f| f.endpoint == endpoint) {
            self.select_alternate(fmt.interface, 1)?;
        }
        
        Ok(())
    }

    /// Stop isochronous transfer
    fn stop_isoc_transfer(&mut self, endpoint: u8) -> Result<(), AudioError> {
        // Select alternate setting 0 (zero bandwidth) to stop transfers
        if let Some(fmt) = self.stream_formats.iter().find(|f| f.endpoint == endpoint) {
            self.select_alternate(fmt.interface, 0)?;
        }
        
        // Mark stream as inactive
        if let Some(stream) = self.streams.iter_mut().find(|s| s.endpoint == endpoint) {
            stream.active = false;
        }
        
        Ok(())
    }

    /// Convert 0-100 volume to dB (in 1/256 dB units as per UAC spec)
    fn volume_to_db(volume: u8) -> i16 {
        // UAC volume is in 1/256 dB units
        // Typical range: -127.9961 dB to 0 dB
        // 0 = 0 dB (max), -32768 = -128 dB (min/mute)
        if volume == 0 {
            -32768 // Mute
        } else {
            // Linear-ish mapping: 100 = 0dB, 0 = -96dB
            let db = -96.0 * (1.0 - (volume as f32 / 100.0));
            (db * 256.0) as i16
        }
    }

    /// Convert dB to 0-100 volume
    fn db_to_volume(db: i16) -> u8 {
        if db <= -24576 { // -96 dB or less
            0
        } else {
            let db_float = db as f32 / 256.0;
            let volume = 100.0 * (1.0 + db_float / 96.0);
            volume.clamp(0.0, 100.0) as u8
        }
    }
}

impl AudioDevice for UsbAudioDevice {
    fn info(&self) -> DeviceInfo {
        let mut min_rate = 48000u32;
        let mut max_rate = 44100u32;
        let mut sample_formats = Vec::new();
        let mut max_channels = 2u8;

        // Collect capabilities from all streams
        for stream in &self.playback_streams {
            for rate in &stream.format.sample_rates {
                min_rate = min_rate.min(*rate);
                max_rate = max_rate.max(*rate);
            }
            max_channels = max_channels.max(stream.format.channels);
            
            // Map bit resolution to sample format
            let format = match (stream.format.subframe_size, stream.format.bit_resolution) {
                (1, 8) => SampleFormat::U8,
                (2, 16) => SampleFormat::S16Le,
                (3, 24) => SampleFormat::S24Le,
                (4, 24) | (4, 32) => SampleFormat::S32Le,
                _ => SampleFormat::S16Le,
            };
            if !sample_formats.contains(&format) {
                sample_formats.push(format);
            }
        }

        if sample_formats.is_empty() {
            sample_formats = alloc::vec![SampleFormat::S16Le];
        }
        if min_rate > max_rate {
            min_rate = 44100;
            max_rate = 48000;
        }

        let mut directions = Vec::new();
        if !self.playback_streams.is_empty() {
            directions.push(StreamDirection::Playback);
        }
        if !self.capture_streams.is_empty() {
            directions.push(StreamDirection::Capture);
        }
        if directions.is_empty() {
            directions = alloc::vec![StreamDirection::Playback, StreamDirection::Capture];
        }

        DeviceInfo {
            id: self.device_id,
            name: self.name.clone(),
            description: String::from("USB Audio Class device"),
            device_type: DeviceType::UsbAudio,
            capabilities: DeviceCapabilities {
                formats: sample_formats,
                min_sample_rate: min_rate,
                max_sample_rate: max_rate,
                min_channels: 1,
                max_channels,
                directions,
            },
        }
    }

    fn open_stream(&mut self, direction: StreamDirection, config: &StreamConfig) -> Result<StreamId, AudioError> {
        // Initialize device on first stream open
        if !self.initialized {
            self.initialized = true;
        }

        let id = self.next_stream_id.fetch_add(1, Ordering::SeqCst);
        let stream = UsbAudioStream::new(id, direction, 0);

        // Find appropriate streaming interface
        // Select alternate setting with matching format
        // Set sample rate

        match direction {
            StreamDirection::Playback => {
                // Set sample rate
                if let Some(s) = self.playback_streams.first() {
                    match self.uac_version {
                        UacVersion::Uac1 => {
                            self.set_sample_rate_uac1(s.endpoint, config.sample_rate)?;
                        }
                        UacVersion::Uac2 | UacVersion::Uac3 => {
                            self.set_sample_rate_uac2(0, config.sample_rate)?;
                        }
                    }
                }
                self.playback_streams.push(stream);
            }
            StreamDirection::Capture => {
                if let Some(s) = self.capture_streams.first() {
                    match self.uac_version {
                        UacVersion::Uac1 => {
                            self.set_sample_rate_uac1(s.endpoint, config.sample_rate)?;
                        }
                        UacVersion::Uac2 | UacVersion::Uac3 => {
                            self.set_sample_rate_uac2(0, config.sample_rate)?;
                        }
                    }
                }
                self.capture_streams.push(stream);
            }
        }

        Ok(id)
    }

    fn close_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        // Find and remove stream
        if let Some(pos) = self.playback_streams.iter().position(|s| s.id == stream_id) {
            let (is_running, endpoint, interface) = {
                let stream = &self.playback_streams[pos];
                (stream.state == StreamState::Running, stream.endpoint, stream.interface)
            };
            if is_running {
                self.stop_isoc_transfer(endpoint)?;
            }
            self.select_alternate(interface, 0)?;
            self.playback_streams.remove(pos);
            return Ok(());
        }

        if let Some(pos) = self.capture_streams.iter().position(|s| s.id == stream_id) {
            let (is_running, endpoint, interface) = {
                let stream = &self.capture_streams[pos];
                (stream.state == StreamState::Running, stream.endpoint, stream.interface)
            };
            if is_running {
                self.stop_isoc_transfer(endpoint)?;
            }
            self.select_alternate(interface, 0)?;
            self.capture_streams.remove(pos);
            return Ok(());
        }

        Err(AudioError::StreamNotFound)
    }

    fn start_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        // Find stream info first (immutable borrow)
        let playback_info = self.playback_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| (s.interface, s.alternate, s.endpoint));
        
        if let Some((interface, alternate, endpoint)) = playback_info {
            self.select_alternate(interface, alternate)?;
            self.start_isoc_transfer(endpoint)?;
            if let Some(stream) = self.playback_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Running;
            }
            return Ok(());
        }

        let capture_info = self.capture_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| (s.interface, s.alternate, s.endpoint));
        
        if let Some((interface, alternate, endpoint)) = capture_info {
            self.select_alternate(interface, alternate)?;
            self.start_isoc_transfer(endpoint)?;
            if let Some(stream) = self.capture_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Running;
            }
            return Ok(());
        }

        Err(AudioError::StreamNotFound)
    }

    fn stop_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        let playback_endpoint = self.playback_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = playback_endpoint {
            self.stop_isoc_transfer(endpoint)?;
            if let Some(stream) = self.playback_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Stopped;
            }
            return Ok(());
        }

        let capture_endpoint = self.capture_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = capture_endpoint {
            self.stop_isoc_transfer(endpoint)?;
            if let Some(stream) = self.capture_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Stopped;
            }
            return Ok(());
        }

        Err(AudioError::StreamNotFound)
    }

    fn pause_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        let playback_endpoint = self.playback_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = playback_endpoint {
            self.stop_isoc_transfer(endpoint)?;
            if let Some(stream) = self.playback_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Paused;
            }
            return Ok(());
        }

        let capture_endpoint = self.capture_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = capture_endpoint {
            self.stop_isoc_transfer(endpoint)?;
            if let Some(stream) = self.capture_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Paused;
            }
            return Ok(());
        }

        Err(AudioError::StreamNotFound)
    }

    fn resume_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        let playback_endpoint = self.playback_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = playback_endpoint {
            self.start_isoc_transfer(endpoint)?;
            if let Some(stream) = self.playback_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Running;
            }
            return Ok(());
        }

        let capture_endpoint = self.capture_streams.iter()
            .find(|s| s.id == stream_id)
            .map(|s| s.endpoint);
        
        if let Some(endpoint) = capture_endpoint {
            self.start_isoc_transfer(endpoint)?;
            if let Some(stream) = self.capture_streams.iter_mut().find(|s| s.id == stream_id) {
                stream.state = StreamState::Running;
            }
            return Ok(());
        }

        Err(AudioError::StreamNotFound)
    }

    fn stream_state(&self, stream_id: StreamId) -> Result<StreamState, AudioError> {
        for stream in &self.playback_streams {
            if stream.id == stream_id {
                return Ok(stream.state);
            }
        }

        for stream in &self.capture_streams {
            if stream.id == stream_id {
                return Ok(stream.state);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn write(&mut self, stream_id: StreamId, data: &[u8]) -> Result<usize, AudioError> {
        // Find the playback stream
        let stream = self.playback_streams.iter_mut()
            .find(|s| s.id == stream_id)
            .ok_or(AudioError::StreamNotFound)?;
        
        if stream.state != StreamState::Running {
            return Err(AudioError::StreamNotRunning);
        }
        
        // Calculate available space in the ring buffer
        let buffer_size = stream.buffer.len();
        let write_pos = stream.buffer_write_pos;
        let read_pos = stream.buffer_read_pos;
        
        let available = if write_pos >= read_pos {
            buffer_size - (write_pos - read_pos) - 1
        } else {
            read_pos - write_pos - 1
        };
        
        let to_write = data.len().min(available);
        
        // Copy data to ring buffer
        for i in 0..to_write {
            let pos = (write_pos + i) % buffer_size;
            stream.buffer[pos] = data[i];
        }
        
        stream.buffer_write_pos = (write_pos + to_write) % buffer_size;
        Ok(to_write)
    }

    fn read(&mut self, stream_id: StreamId, data: &mut [u8]) -> Result<usize, AudioError> {
        // Find the capture stream
        let stream = self.capture_streams.iter_mut()
            .find(|s| s.id == stream_id)
            .ok_or(AudioError::StreamNotFound)?;
        
        if stream.state != StreamState::Running {
            return Err(AudioError::StreamNotRunning);
        }
        
        // Calculate available data in the ring buffer
        let buffer_size = stream.buffer.len();
        let write_pos = stream.buffer_write_pos;
        let read_pos = stream.buffer_read_pos;
        
        let available = if write_pos >= read_pos {
            write_pos - read_pos
        } else {
            buffer_size - read_pos + write_pos
        };
        
        let to_read = data.len().min(available);
        
        // Copy data from ring buffer
        for i in 0..to_read {
            let pos = (read_pos + i) % buffer_size;
            data[i] = stream.buffer[pos];
        }
        
        stream.buffer_read_pos = (read_pos + to_read) % buffer_size;
        Ok(to_read)
    }

    fn available_write(&self, stream_id: StreamId) -> Result<usize, AudioError> {
        for stream in &self.playback_streams {
            if stream.id == stream_id {
                return Ok(stream.max_packet_size as usize);
            }
        }
        Err(AudioError::StreamNotFound)
    }

    fn available_read(&self, stream_id: StreamId) -> Result<usize, AudioError> {
        for stream in &self.capture_streams {
            if stream.id == stream_id {
                return Ok(0);
            }
        }
        Err(AudioError::StreamNotFound)
    }

    fn set_volume(&mut self, _stream_id: StreamId, volume: u8) -> Result<(), AudioError> {
        self.volume = volume.min(100);
        
        // Set on all feature units with volume control
        let db = Self::volume_to_db(self.volume);
        for unit in &self.feature_units {
            if unit.has_volume {
                // Set master channel (0) and optionally L/R channels
                self.set_volume_on_unit(unit.id, 0, db)?;
            }
        }
        
        Ok(())
    }

    fn get_volume(&self, _stream_id: StreamId) -> Result<u8, AudioError> {
        Ok(self.volume)
    }

    fn set_mute(&mut self, _stream_id: StreamId, mute: bool) -> Result<(), AudioError> {
        self.muted = mute;
        
        for unit in &self.feature_units {
            if unit.has_mute {
                self.set_mute_on_unit(unit.id, 0, mute)?;
            }
        }
        
        Ok(())
    }

    fn is_muted(&self, _stream_id: StreamId) -> Result<bool, AudioError> {
        Ok(self.muted)
    }
}

// ============================================================================
// Device Detection
// ============================================================================

/// Check if a USB device is an audio device
pub fn is_audio_device(class: u8, subclass: u8, _protocol: u8) -> bool {
    class == USB_CLASS_AUDIO && (subclass == subclass::AUDIO_CONTROL || subclass == subclass::AUDIO_STREAMING)
}

/// Probe for USB audio devices
pub fn probe() -> Option<Box<dyn AudioDevice>> {
    use crate::usb::USB_SUBSYSTEM;
    
    let usb_guard = USB_SUBSYSTEM.lock();
    let usb = usb_guard.as_ref()?;
    
    // Iterate through connected USB devices
    for device in usb.devices() {
        // Check if this is an audio class device (using public fields)
        let class = device.device_class;
        let subclass = device.device_subclass;
        
        if is_audio_device(class, subclass, 0) {
            let product_name = device.product.clone()
                .unwrap_or_else(|| String::from("USB Audio Device"));
            let audio_device = UsbAudioDevice::new(
                crate::sound::NEXT_DEVICE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst),
                device.address,
                device.vendor_id,
                device.product_id,
                product_name,
            );
            
            // If we found valid streams, return the device
            // Note: Full descriptor parsing requires additional USB infrastructure
            if !audio_device.stream_formats.is_empty() {
                return Some(Box::new(audio_device));
            }
            
            // Return even without parsing (device was detected as audio class)
            return Some(Box::new(audio_device));
        }
        
        // Also check interface-level class (composite devices)
        for config in &device.configurations {
            for interface in &config.interfaces {
                if interface.class == USB_CLASS_AUDIO && interface.subclass == subclass::AUDIO_CONTROL {
                    let product_name = device.product.clone()
                        .unwrap_or_else(|| String::from("USB Audio Device"));
                    let mut audio_device = UsbAudioDevice::new(
                        crate::sound::NEXT_DEVICE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst),
                        device.address,
                        device.vendor_id,
                        device.product_id,
                        product_name,
                    );
                    
                    audio_device.control_interface = interface.number;
                    
                    // Return the audio device
                    return Some(Box::new(audio_device));
                }
            }
        }
    }
    
    None
}

/// Create a USB audio device from USB device info
pub fn create(
    usb_address: u8,
    vendor_id: u16,
    product_id: u16,
    name: &str,
) -> UsbAudioDevice {
    UsbAudioDevice::new(0, usb_address, vendor_id, product_id, String::from(name))
}
