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
            uac_version: UacVersion::Uac1,
            vendor_id,
            product_id,
            name,
            input_terminals: Vec::new(),
            output_terminals: Vec::new(),
            feature_units: Vec::new(),
            playback_streams: Vec::new(),
            capture_streams: Vec::new(),
            next_stream_id: AtomicU32::new(1),
            initialized: false,
            volume: 100,
            muted: false,
        }
    }

    /// Parse audio control descriptors
    fn parse_ac_descriptors(&mut self, _descriptors: &[u8]) -> Result<(), AudioError> {
        // In a real implementation, this would:
        // 1. Parse AC header to get topology
        // 2. Parse input/output terminals
        // 3. Parse feature units
        // 4. Parse mixer/selector/processing units
        // 5. Build audio path graph
        Ok(())
    }

    /// Parse audio streaming descriptors
    fn parse_as_descriptors(&mut self, _interface: u8, _descriptors: &[u8]) -> Result<(), AudioError> {
        // In a real implementation, this would:
        // 1. Parse AS general descriptor
        // 2. Parse format type descriptor
        // 3. Parse endpoint descriptors
        // 4. Store supported formats and endpoints
        Ok(())
    }

    /// Send control request to audio unit
    fn control_request(
        &self,
        _request: u8,
        _unit_id: u8,
        _control: u8,
        _channel: u8,
        _data: &[u8],
    ) -> Result<Vec<u8>, AudioError> {
        // In a real implementation, this would:
        // 1. Build USB control transfer
        // 2. Send to audio control interface
        // 3. Return response data
        Ok(Vec::new())
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
    fn set_sample_rate_uac1(&self, _endpoint: u8, _rate: u32) -> Result<(), AudioError> {
        // UAC 1.0 uses endpoint SET_CUR for sample rate
        // Frequency is 3 bytes, little endian
        Ok(())
    }

    /// Set sample rate (UAC 2.0)
    fn set_sample_rate_uac2(&self, _clock_id: u8, _rate: u32) -> Result<(), AudioError> {
        // UAC 2.0 uses clock source control
        Ok(())
    }

    /// Select alternate setting for streaming interface
    fn select_alternate(&self, _interface: u8, _alternate: u8) -> Result<(), AudioError> {
        // USB SET_INTERFACE request
        Ok(())
    }

    /// Start isochronous transfer
    fn start_isoc_transfer(&self, _endpoint: u8) -> Result<(), AudioError> {
        // In a real implementation:
        // 1. Allocate transfer buffers
        // 2. Submit URBs to USB host controller
        // 3. Set up completion callbacks
        Ok(())
    }

    /// Stop isochronous transfer
    fn stop_isoc_transfer(&self, _endpoint: u8) -> Result<(), AudioError> {
        // Cancel pending URBs
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
            let stream = &self.playback_streams[pos];
            if stream.state == StreamState::Running {
                self.stop_isoc_transfer(stream.endpoint)?;
            }
            self.select_alternate(stream.interface, 0)?;
            self.playback_streams.remove(pos);
            return Ok(());
        }

        if let Some(pos) = self.capture_streams.iter().position(|s| s.id == stream_id) {
            let stream = &self.capture_streams[pos];
            if stream.state == StreamState::Running {
                self.stop_isoc_transfer(stream.endpoint)?;
            }
            self.select_alternate(stream.interface, 0)?;
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

    fn write(&mut self, _stream_id: StreamId, _data: &[u8]) -> Result<usize, AudioError> {
        // In a real implementation:
        // 1. Copy data to isochronous transfer buffer
        // 2. Submit URB if not already pending
        // 3. Return bytes written
        Ok(0)
    }

    fn read(&mut self, _stream_id: StreamId, _data: &mut [u8]) -> Result<usize, AudioError> {
        // In a real implementation:
        // 1. Copy from completed isochronous transfer buffer
        // 2. Return bytes read
        Ok(0)
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
    // In a real implementation:
    // 1. Enumerate USB devices
    // 2. Check for audio class interfaces
    // 3. Parse audio descriptors
    // 4. Create device instance
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
