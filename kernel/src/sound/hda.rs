//! # Intel High Definition Audio (HDA) Driver
//!
//! Implements support for Intel HDA-compatible audio controllers.
//!
//! ## HDA Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    HDA Controller                        │
//! ├─────────────────────────────────────────────────────────┤
//! │  CORB (Command)  │  RIRB (Response)  │  DMA Streams     │
//! ├──────────────────┴───────────────────┴──────────────────┤
//! │                    Codec(s)                              │
//! ├─────────────────────────────────────────────────────────┤
//! │   AFG (Audio)   │   Widgets (DAC, ADC, Mixer, Pin)      │
//! └─────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    AudioDevice, AudioError, AudioRingBuffer, DeviceCapabilities, DeviceInfo,
    DeviceType, SampleFormat, StreamConfig, StreamDirection, StreamId, StreamState,
};

// =============================================================================
// HDA Register Definitions
// =============================================================================

/// HDA Controller Registers (offsets from BAR0)
#[allow(dead_code)]
mod regs {
    /// Global Capabilities
    pub const GCAP: u32 = 0x00;
    /// Minor Version
    pub const VMIN: u32 = 0x02;
    /// Major Version
    pub const VMAJ: u32 = 0x03;
    /// Output Payload Capability
    pub const OUTPAY: u32 = 0x04;
    /// Input Payload Capability
    pub const INPAY: u32 = 0x06;
    /// Global Control
    pub const GCTL: u32 = 0x08;
    /// Wake Enable
    pub const WAKEEN: u32 = 0x0C;
    /// State Change Status
    pub const STATESTS: u32 = 0x0E;
    /// Global Status
    pub const GSTS: u32 = 0x10;
    /// Interrupt Control
    pub const INTCTL: u32 = 0x20;
    /// Interrupt Status
    pub const INTSTS: u32 = 0x24;
    /// Wall Clock Counter
    pub const WALLCLK: u32 = 0x30;
    /// Stream Synchronization
    pub const SSYNC: u32 = 0x38;
    /// CORB Lower Base Address
    pub const CORBLBASE: u32 = 0x40;
    /// CORB Upper Base Address
    pub const CORBUBASE: u32 = 0x44;
    /// CORB Write Pointer
    pub const CORBWP: u32 = 0x48;
    /// CORB Read Pointer
    pub const CORBRP: u32 = 0x4A;
    /// CORB Control
    pub const CORBCTL: u32 = 0x4C;
    /// CORB Status
    pub const CORBSTS: u32 = 0x4D;
    /// CORB Size
    pub const CORBSIZE: u32 = 0x4E;
    /// RIRB Lower Base Address
    pub const RIRBLBASE: u32 = 0x50;
    /// RIRB Upper Base Address
    pub const RIRBUBASE: u32 = 0x54;
    /// RIRB Write Pointer
    pub const RIRBWP: u32 = 0x58;
    /// Response Interrupt Count
    pub const RINTCNT: u32 = 0x5A;
    /// RIRB Control
    pub const RIRBCTL: u32 = 0x5C;
    /// RIRB Status
    pub const RIRBSTS: u32 = 0x5D;
    /// RIRB Size
    pub const RIRBSIZE: u32 = 0x5E;
    /// Immediate Command Output Interface
    pub const ICOI: u32 = 0x60;
    /// Immediate Command Input Interface
    pub const ICII: u32 = 0x64;
    /// Immediate Command Status
    pub const ICIS: u32 = 0x68;
    /// DMA Position Buffer Lower Base
    pub const DPIBLBASE: u32 = 0x70;
    /// DMA Position Buffer Upper Base
    pub const DPIBUBASE: u32 = 0x74;
    /// Stream Descriptor 0 (first output stream)
    pub const SD0: u32 = 0x80;
}

/// Stream Descriptor Register offsets
#[allow(dead_code)]
mod sd_regs {
    /// Control
    pub const CTL: u32 = 0x00;
    /// Status
    pub const STS: u32 = 0x03;
    /// Link Position in Current Buffer
    pub const LPIB: u32 = 0x04;
    /// Cyclic Buffer Length
    pub const CBL: u32 = 0x08;
    /// Last Valid Index
    pub const LVI: u32 = 0x0C;
    /// FIFO Size
    pub const FIFOS: u32 = 0x10;
    /// Format
    pub const FMT: u32 = 0x12;
    /// Buffer Descriptor List Pointer Lower
    pub const BDLPL: u32 = 0x18;
    /// Buffer Descriptor List Pointer Upper
    pub const BDLPU: u32 = 0x1C;
}

/// Global Control Register bits
#[allow(dead_code)]
mod gctl {
    /// Controller Reset
    pub const CRST: u32 = 1 << 0;
    /// Flush Control
    pub const FCNTRL: u32 = 1 << 1;
    /// Accept Unsolicited Response Enable
    pub const UNSOL: u32 = 1 << 8;
}

/// Stream Descriptor Control bits
#[allow(dead_code)]
mod sdctl {
    /// Stream Reset
    pub const SRST: u8 = 1 << 0;
    /// Stream Run
    pub const RUN: u8 = 1 << 1;
    /// Interrupt on Completion Enable
    pub const IOCE: u8 = 1 << 2;
    /// FIFO Error Interrupt Enable
    pub const FEIE: u8 = 1 << 3;
    /// Descriptor Error Interrupt Enable
    pub const DEIE: u8 = 1 << 4;
}

// =============================================================================
// HDA Codec Commands
// =============================================================================

/// Codec verb types
#[allow(dead_code)]
mod verbs {
    /// Get Parameter
    pub const GET_PARAM: u32 = 0xF00;
    /// Get Connection Select Control
    pub const GET_CONN_SEL: u32 = 0xF01;
    /// Get Connection List Entry
    pub const GET_CONN_LIST: u32 = 0xF02;
    /// Get Processing State
    pub const GET_PROC_STATE: u32 = 0xF03;
    /// Get Coefficient Index
    pub const GET_COEF_INDEX: u32 = 0xD00;
    /// Get Processing Coefficient
    pub const GET_PROC_COEF: u32 = 0xC00;
    /// Get Amplifier Gain/Mute
    pub const GET_AMP_GAIN: u32 = 0xB00;
    /// Get Converter Format
    pub const GET_CONV_FMT: u32 = 0xA00;
    /// Get Digital Converter Control
    pub const GET_DIGI_CONV: u32 = 0xF0D;
    /// Get Power State
    pub const GET_POWER_STATE: u32 = 0xF05;
    /// Get Converter Channel Count
    pub const GET_CONV_CHAN_CNT: u32 = 0xF2D;
    /// Get HDMI DIP Size
    pub const GET_HDMI_DIP_SIZE: u32 = 0xF2E;
    /// Get HDMI ELD Data
    pub const GET_HDMI_ELD: u32 = 0xF2F;
    /// Get Volume Knob Control
    pub const GET_VOL_KNOB: u32 = 0xF0F;
    /// Get GPIO Data
    pub const GET_GPIO_DATA: u32 = 0xF15;
    /// Get GPIO Enable Mask
    pub const GET_GPIO_MASK: u32 = 0xF16;
    /// Get GPIO Direction
    pub const GET_GPIO_DIR: u32 = 0xF17;
    /// Get Pin Sense
    pub const GET_PIN_SENSE: u32 = 0xF09;
    /// Get EAPD/BTL Enable
    pub const GET_EAPD: u32 = 0xF0C;
    /// Get Pin Widget Control
    pub const GET_PIN_CTL: u32 = 0xF07;
    /// Get Unsolicited Response
    pub const GET_UNSOL_RSP: u32 = 0xF08;
    /// Get Beep Control
    pub const GET_BEEP: u32 = 0xF0A;
    /// Get Config Default
    pub const GET_CONFIG_DEFAULT: u32 = 0xF1C;
    /// Get Subsystem ID
    pub const GET_SUBSYSTEM_ID: u32 = 0xF20;
    
    // Set verbs (0x7xx, 0x3xx)
    /// Set Connection Select Control
    pub const SET_CONN_SEL: u32 = 0x701;
    /// Set Power State
    pub const SET_POWER_STATE: u32 = 0x705;
    /// Set Converter Stream/Channel
    pub const SET_CONV_STREAM: u32 = 0x706;
    /// Set Pin Widget Control
    pub const SET_PIN_CTL: u32 = 0x707;
    /// Set Unsolicited Response
    pub const SET_UNSOL_RSP: u32 = 0x708;
    /// Set EAPD/BTL Enable
    pub const SET_EAPD: u32 = 0x70C;
    /// Set Converter Format
    pub const SET_CONV_FMT: u32 = 0x200;
    /// Set Amplifier Gain/Mute
    pub const SET_AMP_GAIN: u32 = 0x300;
    /// Set Digital Converter Control
    pub const SET_DIGI_CONV_1: u32 = 0x70D;
    /// Set Digital Converter Control 2
    pub const SET_DIGI_CONV_2: u32 = 0x70E;
    /// Set Volume Knob Control
    pub const SET_VOL_KNOB: u32 = 0x70F;
    /// Set GPIO Data
    pub const SET_GPIO_DATA: u32 = 0x715;
    /// Set GPIO Enable Mask
    pub const SET_GPIO_MASK: u32 = 0x716;
    /// Set GPIO Direction
    pub const SET_GPIO_DIR: u32 = 0x717;
    /// Set Beep Control
    pub const SET_BEEP: u32 = 0x70A;
    /// Set Config Default
    pub const SET_CONFIG_DEFAULT: u32 = 0x71C;
}

/// Codec parameters
#[allow(dead_code)]
mod params {
    /// Vendor ID
    pub const VENDOR_ID: u8 = 0x00;
    /// Revision ID
    pub const REVISION_ID: u8 = 0x02;
    /// Subordinate Node Count
    pub const NODE_COUNT: u8 = 0x04;
    /// Function Group Type
    pub const FN_GROUP_TYPE: u8 = 0x05;
    /// Audio Function Group Capabilities
    pub const AFG_CAP: u8 = 0x08;
    /// Audio Widget Capabilities
    pub const WIDGET_CAP: u8 = 0x09;
    /// Supported PCM Size/Rates
    pub const PCM_RATES: u8 = 0x0A;
    /// Supported Stream Formats
    pub const STREAM_FORMATS: u8 = 0x0B;
    /// Pin Capabilities
    pub const PIN_CAP: u8 = 0x0C;
    /// Input Amplifier Capabilities
    pub const INPUT_AMP_CAP: u8 = 0x0D;
    /// Output Amplifier Capabilities
    pub const OUTPUT_AMP_CAP: u8 = 0x12;
    /// Connection List Length
    pub const CONN_LIST_LEN: u8 = 0x0E;
    /// Supported Power States
    pub const POWER_STATES: u8 = 0x0F;
    /// Processing Capabilities
    pub const PROC_CAP: u8 = 0x10;
    /// GPIO Count
    pub const GPIO_COUNT: u8 = 0x11;
    /// Volume Knob Capabilities
    pub const VOL_KNOB_CAP: u8 = 0x13;
}

// =============================================================================
// HDA Data Structures
// =============================================================================

/// Buffer Descriptor List Entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BdlEntry {
    /// Lower 32 bits of buffer address
    pub addr_lo: u32,
    /// Upper 32 bits of buffer address
    pub addr_hi: u32,
    /// Buffer length in bytes
    pub length: u32,
    /// Interrupt on Completion flag
    pub ioc: u32,
}

/// HDA Stream
pub struct HdaStream {
    /// Stream ID
    pub id: StreamId,
    /// Stream direction
    pub direction: StreamDirection,
    /// Stream configuration
    pub config: StreamConfig,
    /// Stream state
    pub state: StreamState,
    /// Audio ring buffer
    pub buffer: AudioRingBuffer,
    /// Volume (0-100)
    pub volume: u8,
    /// Muted
    pub muted: bool,
    /// Stream tag (1-15)
    pub tag: u8,
}

impl HdaStream {
    /// Creates a new HDA stream
    pub fn new(id: StreamId, direction: StreamDirection, config: StreamConfig, tag: u8) -> Self {
        let buffer_size = config.buffer_size();
        Self {
            id,
            direction,
            config,
            state: StreamState::Stopped,
            buffer: AudioRingBuffer::new(buffer_size),
            volume: 100,
            muted: false,
            tag,
        }
    }
}

/// HDA Codec information
#[derive(Debug, Clone)]
pub struct HdaCodec {
    /// Codec address (0-14)
    pub address: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Revision ID
    pub revision: u8,
}

/// HDA Controller
pub struct HdaController {
    /// Base address of MMIO registers
    base_addr: usize,
    /// Number of output streams
    num_output_streams: u8,
    /// Number of input streams
    num_input_streams: u8,
    /// Number of bidirectional streams
    num_bidir_streams: u8,
    /// Detected codecs
    codecs: Vec<HdaCodec>,
    /// Active streams
    streams: BTreeMap<StreamId, HdaStream>,
    /// Next stream ID
    next_stream_id: AtomicU32,
    /// Next stream tag
    next_stream_tag: u8,
}

impl HdaController {
    /// Creates a new HDA controller instance
    pub fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            num_output_streams: 0,
            num_input_streams: 0,
            num_bidir_streams: 0,
            codecs: Vec::new(),
            streams: BTreeMap::new(),
            next_stream_id: AtomicU32::new(1),
            next_stream_tag: 1,
        }
    }
    
    /// Reads a 32-bit register
    #[allow(dead_code)]
    fn read32(&self, offset: u32) -> u32 {
        unsafe {
            core::ptr::read_volatile((self.base_addr + offset as usize) as *const u32)
        }
    }
    
    /// Writes a 32-bit register
    #[allow(dead_code)]
    fn write32(&self, offset: u32, value: u32) {
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset as usize) as *mut u32, value);
        }
    }
    
    /// Reads a 16-bit register
    #[allow(dead_code)]
    fn read16(&self, offset: u32) -> u16 {
        unsafe {
            core::ptr::read_volatile((self.base_addr + offset as usize) as *const u16)
        }
    }
    
    /// Writes a 16-bit register
    #[allow(dead_code)]
    fn write16(&self, offset: u32, value: u16) {
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset as usize) as *mut u16, value);
        }
    }
    
    /// Reads an 8-bit register
    #[allow(dead_code)]
    fn read8(&self, offset: u32) -> u8 {
        unsafe {
            core::ptr::read_volatile((self.base_addr + offset as usize) as *const u8)
        }
    }
    
    /// Writes an 8-bit register
    #[allow(dead_code)]
    fn write8(&self, offset: u32, value: u8) {
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset as usize) as *mut u8, value);
        }
    }
    
    /// Initializes the controller
    pub fn init(&mut self) -> Result<(), AudioError> {
        // TODO: Implement full HDA initialization:
        // 1. Reset controller
        // 2. Read GCAP to get stream counts
        // 3. Set up CORB/RIRB for codec communication
        // 4. Enumerate codecs
        // 5. Configure AFG (Audio Function Group)
        // 6. Set up stream descriptors
        
        // For now, just simulate initialization
        self.num_output_streams = 4;
        self.num_input_streams = 4;
        self.num_bidir_streams = 0;
        
        Ok(())
    }
    
    /// Allocates a stream tag
    fn allocate_tag(&mut self) -> u8 {
        let tag = self.next_stream_tag;
        self.next_stream_tag = if self.next_stream_tag >= 15 { 1 } else { self.next_stream_tag + 1 };
        tag
    }
    
    /// Formats sample rate and format for HDA register
    #[allow(dead_code)]
    fn encode_format(config: &StreamConfig) -> u16 {
        // Format encoding:
        // Bits 15-14: Stream type (0 = PCM)
        // Bits 13-11: Sample base rate (0 = 48kHz, 1 = 44.1kHz)
        // Bits 10-8: Sample base rate multiplier
        // Bits 7-4: Sample base rate divisor
        // Bits 3-0: Bits per sample (0 = 8, 1 = 16, 2 = 20, 3 = 24, 4 = 32)
        
        let base = match config.sample_rate {
            44100 | 22050 | 11025 => 1u16 << 14,
            _ => 0u16,
        };
        
        let bits = match config.format {
            SampleFormat::S8 | SampleFormat::U8 => 0,
            SampleFormat::S16Le | SampleFormat::S16Be | SampleFormat::U16Le => 1,
            SampleFormat::S24Le => 3,
            SampleFormat::S32Le | SampleFormat::F32Le => 4,
        };
        
        let channels = (config.channels.saturating_sub(1) as u16) & 0x0F;
        
        base | (bits << 4) | channels
    }
}

impl AudioDevice for HdaController {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: 0,
            name: "Intel HDA".to_string(),
            description: "Intel High Definition Audio Controller".to_string(),
            device_type: DeviceType::Hda,
            capabilities: DeviceCapabilities {
                formats: alloc::vec![
                    SampleFormat::S16Le,
                    SampleFormat::S24Le,
                    SampleFormat::S32Le,
                ],
                min_sample_rate: 8000,
                max_sample_rate: 192000,
                min_channels: 1,
                max_channels: 8,
                directions: alloc::vec![StreamDirection::Playback, StreamDirection::Capture],
            },
        }
    }
    
    fn open_stream(
        &mut self,
        direction: StreamDirection,
        config: &StreamConfig,
    ) -> Result<StreamId, AudioError> {
        // Check if we have available streams
        let available = match direction {
            StreamDirection::Playback => self.num_output_streams,
            StreamDirection::Capture => self.num_input_streams,
        };
        
        if self.streams.len() >= available as usize {
            return Err(AudioError::DeviceBusy);
        }
        
        let stream_id = self.next_stream_id.fetch_add(1, Ordering::SeqCst);
        let tag = self.allocate_tag();
        
        let stream = HdaStream::new(stream_id, direction, config.clone(), tag);
        self.streams.insert(stream_id, stream);
        
        Ok(stream_id)
    }
    
    fn close_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        self.streams.remove(&stream).ok_or(AudioError::StreamNotFound)?;
        Ok(())
    }
    
    fn start_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        if s.state == StreamState::Running {
            return Err(AudioError::AlreadyRunning);
        }
        s.state = StreamState::Running;
        Ok(())
    }
    
    fn stop_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        if s.state == StreamState::Stopped {
            return Err(AudioError::AlreadyStopped);
        }
        s.state = StreamState::Stopped;
        Ok(())
    }
    
    fn pause_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        s.state = StreamState::Paused;
        Ok(())
    }
    
    fn resume_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        if s.state != StreamState::Paused {
            return Err(AudioError::InvalidParameter);
        }
        s.state = StreamState::Running;
        Ok(())
    }
    
    fn write(&mut self, stream: StreamId, data: &[u8]) -> Result<usize, AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        if s.direction != StreamDirection::Playback {
            return Err(AudioError::InvalidParameter);
        }
        Ok(s.buffer.write(data))
    }
    
    fn read(&mut self, stream: StreamId, data: &mut [u8]) -> Result<usize, AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        if s.direction != StreamDirection::Capture {
            return Err(AudioError::InvalidParameter);
        }
        Ok(s.buffer.read(data))
    }
    
    fn stream_state(&self, stream: StreamId) -> Result<StreamState, AudioError> {
        self.streams.get(&stream)
            .map(|s| s.state)
            .ok_or(AudioError::StreamNotFound)
    }
    
    fn available_write(&self, stream: StreamId) -> Result<usize, AudioError> {
        self.streams.get(&stream)
            .map(|s| s.buffer.available_write())
            .ok_or(AudioError::StreamNotFound)
    }
    
    fn available_read(&self, stream: StreamId) -> Result<usize, AudioError> {
        self.streams.get(&stream)
            .map(|s| s.buffer.available_read())
            .ok_or(AudioError::StreamNotFound)
    }
    
    fn set_volume(&mut self, stream: StreamId, volume: u8) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        s.volume = volume.min(100);
        Ok(())
    }
    
    fn get_volume(&self, stream: StreamId) -> Result<u8, AudioError> {
        self.streams.get(&stream)
            .map(|s| s.volume)
            .ok_or(AudioError::StreamNotFound)
    }
    
    fn set_mute(&mut self, stream: StreamId, mute: bool) -> Result<(), AudioError> {
        let s = self.streams.get_mut(&stream).ok_or(AudioError::StreamNotFound)?;
        s.muted = mute;
        Ok(())
    }
    
    fn is_muted(&self, stream: StreamId) -> Result<bool, AudioError> {
        self.streams.get(&stream)
            .map(|s| s.muted)
            .ok_or(AudioError::StreamNotFound)
    }
}

/// Probes for HDA controllers on PCI bus
pub fn probe() -> Option<Box<dyn AudioDevice>> {
    // TODO: Implement PCI scanning for HDA devices
    // HDA devices have class code 0x0403 (Audio device, HD Audio)
    // Common vendor/device IDs:
    // - Intel: various
    // - Realtek: 0x10EC:various
    // - AMD: 0x1022:various
    
    // For now, return None (no hardware detected)
    None
}
