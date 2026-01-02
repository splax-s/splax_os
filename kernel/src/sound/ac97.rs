//! AC'97 Audio Codec Driver
//!
//! Implements support for the Intel AC'97 audio codec standard, which was
//! common in older systems (1997-2004) and is still emulated by QEMU/VirtualBox.
//!
//! AC'97 Architecture:
//! - Audio Codec (AC): The actual codec chip
//! - AC-Link: Serial interface between controller and codec
//! - Native Audio Mixer (NAM): Mixer registers at I/O port base
//! - Native Audio Bus Master (NABM): DMA controller at base + 0x10
//!
//! Key registers:
//! - NAM (Mixer): 0x00-0x7E - Codec registers
//! - NABM (Bus Master): 0x10-0x3F - DMA control

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::{
    AudioDevice, AudioError, DeviceCapabilities, DeviceId, DeviceInfo, DeviceType,
    SampleFormat, StreamConfig, StreamDirection, StreamId, StreamState,
};

// ============================================================================
// AC'97 Native Audio Mixer (NAM) Registers
// ============================================================================

/// NAM register offsets (relative to mixer base)
pub mod nam {
    /// Reset register - write any value to reset codec
    pub const RESET: u16 = 0x00;
    /// Master volume (L/R)
    pub const MASTER_VOLUME: u16 = 0x02;
    /// Aux out volume (headphone on some codecs)
    pub const AUX_OUT_VOLUME: u16 = 0x04;
    /// Mono volume
    pub const MONO_VOLUME: u16 = 0x06;
    /// Master tone (bass/treble)
    pub const MASTER_TONE: u16 = 0x08;
    /// PC beep volume
    pub const PC_BEEP_VOLUME: u16 = 0x0A;
    /// Phone volume
    pub const PHONE_VOLUME: u16 = 0x0C;
    /// Mic volume
    pub const MIC_VOLUME: u16 = 0x0E;
    /// Line in volume
    pub const LINE_IN_VOLUME: u16 = 0x10;
    /// CD volume
    pub const CD_VOLUME: u16 = 0x12;
    /// Video volume
    pub const VIDEO_VOLUME: u16 = 0x14;
    /// Aux in volume
    pub const AUX_IN_VOLUME: u16 = 0x16;
    /// PCM out volume
    pub const PCM_OUT_VOLUME: u16 = 0x18;
    /// Record select (input source)
    pub const RECORD_SELECT: u16 = 0x1A;
    /// Record gain
    pub const RECORD_GAIN: u16 = 0x1C;
    /// Record gain mic
    pub const RECORD_GAIN_MIC: u16 = 0x1E;
    /// General purpose
    pub const GENERAL_PURPOSE: u16 = 0x20;
    /// 3D control
    pub const CONTROL_3D: u16 = 0x22;
    /// Audio interrupt/paging
    pub const AUDIO_INT_PAGING: u16 = 0x24;
    /// Powerdown control/status
    pub const POWERDOWN: u16 = 0x26;
    /// Extended audio ID
    pub const EXTENDED_AUDIO_ID: u16 = 0x28;
    /// Extended audio status/control
    pub const EXTENDED_AUDIO_CTRL: u16 = 0x2A;
    /// Front DAC sample rate
    pub const FRONT_DAC_RATE: u16 = 0x2C;
    /// Surround DAC sample rate
    pub const SURROUND_DAC_RATE: u16 = 0x2E;
    /// LFE DAC sample rate
    pub const LFE_DAC_RATE: u16 = 0x30;
    /// ADC sample rate
    pub const ADC_RATE: u16 = 0x32;
    /// Mic ADC sample rate
    pub const MIC_ADC_RATE: u16 = 0x34;
    /// Center/LFE volume
    pub const CENTER_LFE_VOLUME: u16 = 0x36;
    /// Surround volume
    pub const SURROUND_VOLUME: u16 = 0x38;
    /// S/PDIF control
    pub const SPDIF_CONTROL: u16 = 0x3A;
    /// Vendor ID 1
    pub const VENDOR_ID1: u16 = 0x7C;
    /// Vendor ID 2
    pub const VENDOR_ID2: u16 = 0x7E;
}

// ============================================================================
// AC'97 Native Audio Bus Master (NABM) Registers
// ============================================================================

/// NABM register offsets (relative to bus master base)
pub mod nabm {
    // PCM Input (record)
    /// PCM In buffer descriptor base address
    pub const PI_BDBAR: u16 = 0x00;
    /// PCM In current index value
    pub const PI_CIV: u16 = 0x04;
    /// PCM In last valid index
    pub const PI_LVI: u16 = 0x05;
    /// PCM In status
    pub const PI_SR: u16 = 0x06;
    /// PCM In position in current buffer
    pub const PI_PICB: u16 = 0x08;
    /// PCM In prefetch index value
    pub const PI_PIV: u16 = 0x0A;
    /// PCM In control
    pub const PI_CR: u16 = 0x0B;

    // PCM Output (playback)
    /// PCM Out buffer descriptor base address
    pub const PO_BDBAR: u16 = 0x10;
    /// PCM Out current index value
    pub const PO_CIV: u16 = 0x14;
    /// PCM Out last valid index
    pub const PO_LVI: u16 = 0x15;
    /// PCM Out status
    pub const PO_SR: u16 = 0x16;
    /// PCM Out position in current buffer
    pub const PO_PICB: u16 = 0x18;
    /// PCM Out prefetch index value
    pub const PO_PIV: u16 = 0x1A;
    /// PCM Out control
    pub const PO_CR: u16 = 0x1B;

    // Mic Input
    /// Mic buffer descriptor base address
    pub const MC_BDBAR: u16 = 0x20;
    /// Mic current index value
    pub const MC_CIV: u16 = 0x24;
    /// Mic last valid index
    pub const MC_LVI: u16 = 0x25;
    /// Mic status
    pub const MC_SR: u16 = 0x26;
    /// Mic position in current buffer
    pub const MC_PICB: u16 = 0x28;
    /// Mic prefetch index value
    pub const MC_PIV: u16 = 0x2A;
    /// Mic control
    pub const MC_CR: u16 = 0x2B;

    /// Global control
    pub const GLOB_CNT: u16 = 0x2C;
    /// Global status
    pub const GLOB_STA: u16 = 0x30;
    /// Codec access semaphore
    pub const CAS: u16 = 0x34;
}

/// Status register bits
pub mod status {
    /// DMA controller halted
    pub const DCH: u16 = 1 << 0;
    /// Codec ready
    pub const CELV: u16 = 1 << 1;
    /// Last valid buffer completion interrupt
    pub const LVBCI: u16 = 1 << 2;
    /// Buffer completion interrupt
    pub const BCIS: u16 = 1 << 3;
    /// FIFO error
    pub const FIFOE: u16 = 1 << 4;
}

/// Control register bits
pub mod control {
    /// Run/pause bus master
    pub const RPBM: u8 = 1 << 0;
    /// Reset registers
    pub const RR: u8 = 1 << 1;
    /// Last valid buffer interrupt enable
    pub const LVBIE: u8 = 1 << 2;
    /// FIFO error interrupt enable
    pub const FEIE: u8 = 1 << 3;
    /// Interrupt on completion enable
    pub const IOCE: u8 = 1 << 4;
}

/// Global control register bits
pub mod glob_cnt {
    /// Cold reset
    pub const COLD_RESET: u32 = 1 << 1;
    /// Warm reset
    pub const WARM_RESET: u32 = 1 << 2;
    /// AC-link shut off
    pub const LINK_SHUT_OFF: u32 = 1 << 3;
    /// Primary codec ready
    pub const PRIMARY_READY: u32 = 1 << 8;
    /// Secondary codec ready
    pub const SECONDARY_READY: u32 = 1 << 9;
    /// 2 channel mode
    pub const CHANNELS_2: u32 = 0 << 20;
    /// 4 channel mode
    pub const CHANNELS_4: u32 = 1 << 20;
    /// 6 channel mode
    pub const CHANNELS_6: u32 = 2 << 20;
}

/// Extended audio ID bits
pub mod ext_audio {
    /// Variable rate audio supported
    pub const VRA: u16 = 1 << 0;
    /// Double rate audio supported
    pub const DRA: u16 = 1 << 1;
    /// S/PDIF supported
    pub const SPDIF: u16 = 1 << 2;
    /// Variable rate mic supported
    pub const VRM: u16 = 1 << 3;
    /// Center DAC present
    pub const CDAC: u16 = 1 << 6;
    /// Surround DAC present
    pub const SDAC: u16 = 1 << 7;
    /// LFE DAC present
    pub const LDAC: u16 = 1 << 8;
}

// ============================================================================
// Buffer Descriptor List Entry
// ============================================================================

/// Buffer Descriptor List entry (8 bytes each)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct BdlEntry {
    /// Physical address of buffer
    pub address: u32,
    /// Buffer length in samples (not bytes!)
    pub length: u16,
    /// Control flags
    pub control: u16,
}

impl BdlEntry {
    /// Create a new BDL entry
    pub const fn new(address: u32, samples: u16, ioc: bool, bup: bool) -> Self {
        let mut control = 0u16;
        if ioc {
            control |= 1 << 15; // Interrupt on completion
        }
        if bup {
            control |= 1 << 14; // Buffer underrun policy
        }
        Self {
            address,
            length: samples,
            control,
        }
    }

    /// Empty entry
    pub const fn empty() -> Self {
        Self {
            address: 0,
            length: 0,
            control: 0,
        }
    }
}

/// Maximum BDL entries (AC'97 supports up to 32)
pub const MAX_BDL_ENTRIES: usize = 32;

// ============================================================================
// AC'97 Stream
// ============================================================================

/// AC'97 audio stream
pub struct Ac97Stream {
    /// Stream ID
    id: StreamId,
    /// Direction
    direction: StreamDirection,
    /// Current configuration
    config: StreamConfig,
    /// Current state
    state: StreamState,
    /// Buffer Descriptor List
    bdl: [BdlEntry; MAX_BDL_ENTRIES],
    /// Current buffer index
    current_index: usize,
    /// Buffers (physical addresses)
    buffers: Vec<u32>,
    /// Buffer size in bytes
    buffer_size: usize,
}

impl Ac97Stream {
    /// Create a new stream
    pub fn new(id: StreamId, direction: StreamDirection) -> Self {
        Self {
            id,
            direction,
            config: StreamConfig::cd_quality(),
            state: StreamState::Stopped,
            bdl: [BdlEntry::empty(); MAX_BDL_ENTRIES],
            current_index: 0,
            buffers: Vec::new(),
            buffer_size: 0,
        }
    }
}

// ============================================================================
// AC'97 Controller
// ============================================================================

/// AC'97 codec vendor information
#[derive(Debug, Clone)]
pub struct Ac97Codec {
    /// Vendor ID
    pub vendor_id: u32,
    /// Vendor name
    pub vendor_name: &'static str,
    /// Extended capabilities
    pub capabilities: u16,
    /// Variable rate supported
    pub variable_rate: bool,
    /// Max sample rate
    pub max_sample_rate: u32,
}

impl Ac97Codec {
    /// Get vendor name from ID
    fn vendor_name_from_id(id: u32) -> &'static str {
        match id >> 16 {
            0x4144 => "Analog Devices",
            0x414C => "Avance Logic/Realtek",
            0x4352 => "Cirrus Logic",
            0x4358 => "Conexant",
            0x4943 => "ICEnsemble",
            0x4E53 => "National Semiconductor",
            0x5349 => "Silicon Laboratories",
            0x5452 => "TriTech",
            0x574D => "Wolfson",
            0x8384 => "SigmaTel",
            0x8086 => "Intel",
            _ => "Unknown",
        }
    }
}

/// AC'97 Audio Controller
pub struct Ac97Controller {
    /// Device ID
    device_id: DeviceId,
    /// Mixer base I/O port
    mixer_base: u16,
    /// Bus master base I/O port
    bus_master_base: u16,
    /// Codec information
    codec: Option<Ac97Codec>,
    /// Playback stream
    playback_stream: Option<Ac97Stream>,
    /// Capture stream
    capture_stream: Option<Ac97Stream>,
    /// Next stream ID
    next_stream_id: AtomicU32,
    /// Is initialized
    initialized: bool,
}

impl Ac97Controller {
    /// Create a new AC'97 controller
    pub const fn new(device_id: DeviceId, mixer_base: u16, bus_master_base: u16) -> Self {
        Self {
            device_id,
            mixer_base,
            bus_master_base,
            codec: None,
            playback_stream: None,
            capture_stream: None,
            next_stream_id: AtomicU32::new(1),
            initialized: false,
        }
    }

    /// Read from mixer register
    fn read_mixer(&self, reg: u16) -> u16 {
        let port = self.mixer_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let value: u16;
            core::arch::asm!(
                "in ax, dx",
                in("dx") port,
                out("ax") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = port; 0 }
    }

    /// Write to mixer register
    fn write_mixer(&self, reg: u16, value: u16) {
        let port = self.mixer_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "out dx, ax",
                in("dx") port,
                in("ax") value,
                options(nomem, nostack, preserves_flags)
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = (port, value); }
    }

    /// Read from bus master register (8-bit)
    fn read_nabm8(&self, reg: u16) -> u8 {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let value: u8;
            core::arch::asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = port; 0 }
    }

    /// Write to bus master register (8-bit)
    fn write_nabm8(&self, reg: u16, value: u8) {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = (port, value); }
    }

    /// Read from bus master register (16-bit)
    fn read_nabm16(&self, reg: u16) -> u16 {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let value: u16;
            core::arch::asm!(
                "in ax, dx",
                in("dx") port,
                out("ax") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = port; 0 }
    }

    /// Write to bus master register (16-bit)
    fn write_nabm16(&self, reg: u16, value: u16) {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "out dx, ax",
                in("dx") port,
                in("ax") value,
                options(nomem, nostack, preserves_flags)
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = (port, value); }
    }

    /// Read from bus master register (32-bit)
    fn read_nabm32(&self, reg: u16) -> u32 {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let value: u32;
            core::arch::asm!(
                "in eax, dx",
                in("dx") port,
                out("eax") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = port; 0 }
    }

    /// Write to bus master register (32-bit)
    fn write_nabm32(&self, reg: u16, value: u32) {
        let port = self.bus_master_base + reg;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") value,
                options(nomem, nostack, preserves_flags)
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        { let _ = (port, value); }
    }

    /// Reset the controller
    fn reset_controller(&mut self) -> Result<(), AudioError> {
        // Cold reset
        self.write_nabm32(nabm::GLOB_CNT, glob_cnt::COLD_RESET);
        
        // Wait for codec ready (would need actual delay)
        // For now just check status
        let status = self.read_nabm32(nabm::GLOB_STA);
        if status & glob_cnt::PRIMARY_READY == 0 {
            return Err(AudioError::NotInitialized);
        }

        Ok(())
    }

    /// Detect and initialize codec
    fn detect_codec(&mut self) -> Result<(), AudioError> {
        // Reset codec
        self.write_mixer(nam::RESET, 0);

        // Read vendor ID
        let vendor1 = self.read_mixer(nam::VENDOR_ID1) as u32;
        let vendor2 = self.read_mixer(nam::VENDOR_ID2) as u32;
        let vendor_id = (vendor1 << 16) | vendor2;

        // Read extended audio capabilities
        let ext_id = self.read_mixer(nam::EXTENDED_AUDIO_ID);
        let variable_rate = ext_id & ext_audio::VRA != 0;

        self.codec = Some(Ac97Codec {
            vendor_id,
            vendor_name: Ac97Codec::vendor_name_from_id(vendor_id),
            capabilities: ext_id,
            variable_rate,
            max_sample_rate: if variable_rate { 48000 } else { 48000 },
        });

        // Enable variable rate if supported
        if variable_rate {
            let ctrl = self.read_mixer(nam::EXTENDED_AUDIO_CTRL);
            self.write_mixer(nam::EXTENDED_AUDIO_CTRL, ctrl | ext_audio::VRA);
        }

        Ok(())
    }

    /// Set sample rate
    fn set_sample_rate(&self, rate: u32, direction: StreamDirection) -> Result<u32, AudioError> {
        if let Some(ref codec) = self.codec {
            if !codec.variable_rate {
                // Fixed 48kHz
                return Ok(48000);
            }
        }

        let rate = rate.min(48000).max(8000) as u16;

        match direction {
            StreamDirection::Playback => {
                self.write_mixer(nam::FRONT_DAC_RATE, rate);
                Ok(self.read_mixer(nam::FRONT_DAC_RATE) as u32)
            }
            StreamDirection::Capture => {
                self.write_mixer(nam::ADC_RATE, rate);
                Ok(self.read_mixer(nam::ADC_RATE) as u32)
            }
        }
    }

    /// Initialize a stream
    fn init_stream(&mut self, direction: StreamDirection) -> Result<StreamId, AudioError> {
        let id = self.next_stream_id.fetch_add(1, Ordering::SeqCst);
        let stream = Ac97Stream::new(id, direction);

        match direction {
            StreamDirection::Playback => {
                self.playback_stream = Some(stream);
            }
            StreamDirection::Capture => {
                self.capture_stream = Some(stream);
            }
        }

        Ok(id)
    }

    /// Start DMA for a stream
    fn start_dma(&mut self, direction: StreamDirection) -> Result<(), AudioError> {
        let (bdbar_reg, cr_reg, lvi_reg) = match direction {
            StreamDirection::Playback => (nabm::PO_BDBAR, nabm::PO_CR, nabm::PO_LVI),
            StreamDirection::Capture => (nabm::PI_BDBAR, nabm::PI_CR, nabm::PI_LVI),
        };

        // Get the stream to access its BDL
        let stream = match direction {
            StreamDirection::Playback => self.playback_stream.as_ref(),
            StreamDirection::Capture => self.capture_stream.as_ref(),
        };

        if let Some(stream) = stream {
            // Get physical address of BDL
            // The BDL must be in physical memory accessible by the DMA controller
            // For now, we use the BDL array address directly (requires identity mapping)
            let bdl_ptr = stream.bdl.as_ptr() as u64;
            let bdl_phys = bdl_ptr as u32; // Lower 32 bits for AC'97
            
            // Set BDL base address register
            self.write_nabm32(bdbar_reg, bdl_phys);
            
            // Set last valid index (number of valid BDL entries - 1)
            let last_valid_index = stream.buffers.len().saturating_sub(1).min(31) as u8;
            self.write_nabm8(lvi_reg, last_valid_index);
            
            crate::serial_println!("[ac97] Starting DMA: BDBAR=0x{:x}, LVI={}", bdl_phys, last_valid_index);
        }

        // Start DMA: set run bit and enable interrupt on completion
        self.write_nabm8(cr_reg, control::RPBM | control::IOCE);

        Ok(())
    }

    /// Stop DMA for a stream
    fn stop_dma(&mut self, direction: StreamDirection) -> Result<(), AudioError> {
        let cr_reg = match direction {
            StreamDirection::Playback => nabm::PO_CR,
            StreamDirection::Capture => nabm::PI_CR,
        };

        // Clear run bit, set reset
        self.write_nabm8(cr_reg, control::RR);

        Ok(())
    }

    /// Initialize the device (internal use)
    fn init_device(&mut self) -> Result<(), AudioError> {
        if self.initialized {
            return Ok(());
        }

        self.reset_controller()?;
        self.detect_codec()?;

        // Set default volume (unmute, half volume)
        // 0 = max volume, 0x8000 = mute, lower 5 bits = attenuation
        self.write_mixer(nam::MASTER_VOLUME, 0x0808);
        self.write_mixer(nam::PCM_OUT_VOLUME, 0x0808);

        self.initialized = true;
        Ok(())
    }
}

impl AudioDevice for Ac97Controller {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.device_id,
            name: String::from("AC'97 Audio Controller"),
            description: String::from("Intel AC'97 compatible audio codec"),
            device_type: DeviceType::Ac97,
            capabilities: DeviceCapabilities {
                formats: alloc::vec![SampleFormat::S16Le],
                min_sample_rate: 8000,
                max_sample_rate: if self.codec.as_ref().map_or(false, |c| c.variable_rate) {
                    48000
                } else {
                    48000
                },
                min_channels: 2,
                max_channels: 6,
                directions: alloc::vec![StreamDirection::Playback, StreamDirection::Capture],
            },
        }
    }

    fn open_stream(&mut self, direction: StreamDirection, config: &StreamConfig) -> Result<StreamId, AudioError> {
        // Initialize device on first stream open
        if !self.initialized {
            self.init_device()?;
        }

        // Set sample rate
        let _actual_rate = self.set_sample_rate(config.sample_rate, direction)?;

        self.init_stream(direction)
    }

    fn close_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        // Find and remove stream
        if let Some(ref stream) = self.playback_stream {
            if stream.id == stream_id {
                self.stop_dma(StreamDirection::Playback)?;
                self.playback_stream = None;
                return Ok(());
            }
        }

        if let Some(ref stream) = self.capture_stream {
            if stream.id == stream_id {
                self.stop_dma(StreamDirection::Capture)?;
                self.capture_stream = None;
                return Ok(());
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn start_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.playback_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Running;
                return self.start_dma(StreamDirection::Playback);
            }
        }

        if let Some(ref mut stream) = self.capture_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Running;
                return self.start_dma(StreamDirection::Capture);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn stop_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.playback_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Stopped;
                return self.stop_dma(StreamDirection::Playback);
            }
        }

        if let Some(ref mut stream) = self.capture_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Stopped;
                return self.stop_dma(StreamDirection::Capture);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn pause_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.playback_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Paused;
                return self.stop_dma(StreamDirection::Playback);
            }
        }

        if let Some(ref mut stream) = self.capture_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Paused;
                return self.stop_dma(StreamDirection::Capture);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn resume_stream(&mut self, stream_id: StreamId) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.playback_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Running;
                return self.start_dma(StreamDirection::Playback);
            }
        }

        if let Some(ref mut stream) = self.capture_stream {
            if stream.id == stream_id {
                stream.state = StreamState::Running;
                return self.start_dma(StreamDirection::Capture);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn stream_state(&self, stream_id: StreamId) -> Result<StreamState, AudioError> {
        if let Some(ref stream) = self.playback_stream {
            if stream.id == stream_id {
                return Ok(stream.state);
            }
        }

        if let Some(ref stream) = self.capture_stream {
            if stream.id == stream_id {
                return Ok(stream.state);
            }
        }

        Err(AudioError::StreamNotFound)
    }

    fn write(&mut self, stream_id: StreamId, data: &[u8]) -> Result<usize, AudioError> {
        // First, validate and extract all needed values from the stream
        let (current_idx, buffer_addr, bytes_to_write, buffer_size, buffers_len, buffer_phys_addr) = {
            let stream = self.playback_stream.as_ref()
                .filter(|s| s.id == stream_id)
                .ok_or(AudioError::StreamNotFound)?;
            
            if stream.state != StreamState::Running {
                return Err(AudioError::StreamNotRunning);
            }
            
            let current_idx = stream.current_index;
            if current_idx >= stream.buffers.len() {
                return Ok(0); // No buffers available
            }
            
            let buffer_addr = stream.buffers[current_idx] as *mut u8;
            let bytes_to_write = data.len().min(stream.buffer_size);
            let buffer_phys_addr = stream.buffers[current_idx];
            
            (current_idx, buffer_addr, bytes_to_write, stream.buffer_size, stream.buffers.len(), buffer_phys_addr)
        };
        
        // Copy data to DMA buffer (no borrow on self here)
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), buffer_addr, bytes_to_write);
        }
        
        // Calculate next index
        let next_index = (current_idx + 1) % buffers_len;
        
        // Now mutably borrow to update stream state and BDL
        {
            let stream = self.playback_stream.as_mut().unwrap();
            
            // Update BDL entry with sample count (16-bit stereo = 4 bytes per sample)
            let samples = (bytes_to_write / 4) as u16;
            stream.bdl[current_idx] = BdlEntry::new(
                buffer_phys_addr,
                samples,
                true,  // Interrupt on completion
                false, // Don't stop on underrun
            );
            
            // Move to next buffer
            stream.current_index = next_index;
        }
        
        // Update LVI (last valid index) - now safe to borrow self immutably
        self.write_nabm8(nabm::PO_LVI, next_index as u8);
        
        Ok(bytes_to_write)
    }

    fn read(&mut self, stream_id: StreamId, data: &mut [u8]) -> Result<usize, AudioError> {
        // First, validate stream exists and is running, extract buffer info
        let (buffers_len, buffer_size) = {
            let stream = self.capture_stream.as_ref()
                .filter(|s| s.id == stream_id)
                .ok_or(AudioError::StreamNotFound)?;
            
            if stream.state != StreamState::Running {
                return Err(AudioError::StreamNotRunning);
            }
            
            (stream.buffers.len(), stream.buffer_size)
        };
        
        // Check status for buffer completion (now safe to borrow self)
        let status = self.read_nabm16(nabm::PI_SR);
        if status & status::BCIS == 0 {
            // No buffer completed yet
            return Ok(0);
        }
        
        // Clear the interrupt
        self.write_nabm16(nabm::PI_SR, status::BCIS);
        
        // Get current buffer index from hardware
        let hw_index = self.read_nabm8(nabm::PI_CIV) as usize;
        let prev_index = if hw_index == 0 { 
            buffers_len - 1 
        } else { 
            hw_index - 1 
        };
        
        if prev_index >= buffers_len {
            return Ok(0);
        }
        
        // Get buffer address from stream
        let buffer_addr = {
            let stream = self.capture_stream.as_ref().unwrap();
            stream.buffers[prev_index] as *const u8
        };
        
        let bytes_to_read = data.len().min(buffer_size);
        
        // Copy from completed DMA buffer
        unsafe {
            core::ptr::copy_nonoverlapping(buffer_addr, data.as_mut_ptr(), bytes_to_read);
        }
        
        Ok(bytes_to_read)
    }

    fn available_write(&self, stream_id: StreamId) -> Result<usize, AudioError> {
        if let Some(ref stream) = self.playback_stream {
            if stream.id == stream_id {
                // Return available buffer space
                return Ok(stream.buffer_size);
            }
        }
        Err(AudioError::StreamNotFound)
    }

    fn available_read(&self, stream_id: StreamId) -> Result<usize, AudioError> {
        if let Some(ref stream) = self.capture_stream {
            if stream.id == stream_id {
                // Return available data
                return Ok(0);
            }
        }
        Err(AudioError::StreamNotFound)
    }

    fn set_volume(&mut self, _stream_id: StreamId, volume: u8) -> Result<(), AudioError> {
        // AC'97 uses 5-bit attenuation (0 = max, 31 = min)
        // Convert 0-100 to 0-31 attenuation
        let attenuation = (100 - volume.min(100)) as u16 * 31 / 100;
        let value = (attenuation << 8) | attenuation;
        self.write_mixer(nam::MASTER_VOLUME, value);
        Ok(())
    }

    fn get_volume(&self, _stream_id: StreamId) -> Result<u8, AudioError> {
        let value = self.read_mixer(nam::MASTER_VOLUME);
        let attenuation = (value & 0x1F) as u8;
        let volume = 100 - (attenuation * 100 / 31);
        Ok(volume)
    }

    fn set_mute(&mut self, _stream_id: StreamId, mute: bool) -> Result<(), AudioError> {
        let current = self.read_mixer(nam::MASTER_VOLUME);
        if mute {
            self.write_mixer(nam::MASTER_VOLUME, current | 0x8000);
        } else {
            self.write_mixer(nam::MASTER_VOLUME, current & !0x8000);
        }
        Ok(())
    }

    fn is_muted(&self, _stream_id: StreamId) -> Result<bool, AudioError> {
        let value = self.read_mixer(nam::MASTER_VOLUME);
        Ok(value & 0x8000 != 0)
    }
}

// ============================================================================
// Device Detection
// ============================================================================

/// Standard AC'97 I/O ports (ICH-style)
pub const STANDARD_MIXER_BASE: u16 = 0x0000; // Actually varies by PCI device
pub const STANDARD_BUS_MASTER_BASE: u16 = 0x0000;

/// Probe for AC'97 devices
pub fn probe() -> Option<Box<dyn AudioDevice>> {
    use crate::pci;
    
    // Known PCI vendor/device IDs for AC'97:
    // Intel ICH: 8086:2415, 8086:2425, 8086:2445, 8086:2485, 8086:24C5, 8086:24D5
    // VIA: 1106:3058
    // SiS: 1039:7012
    // nForce: 10DE:01B1
    const AC97_DEVICES: &[(u16, u16)] = &[
        (0x8086, 0x2415), // Intel 82801AA
        (0x8086, 0x2425), // Intel 82801AB
        (0x8086, 0x2445), // Intel 82801BA
        (0x8086, 0x2485), // Intel ICH3
        (0x8086, 0x24C5), // Intel ICH4
        (0x8086, 0x24D5), // Intel ICH5
        (0x8086, 0x266E), // Intel ICH6
        (0x8086, 0x27DE), // Intel ICH7
        (0x1106, 0x3058), // VIA VT82C686
        (0x1039, 0x7012), // SiS 7012
        (0x10DE, 0x01B1), // nForce
        (0x10DE, 0x006A), // nForce2
    ];
    
    // Scan PCI bus for AC'97 controllers
    for &(vendor, device) in AC97_DEVICES {
        if let Some(pci_device) = pci::find_device(vendor, device) {
            // Read BAR0 (mixer base) and BAR1 (bus master base)
            let mixer_base = pci_device.bar(0).map(|b| (b.address & 0xFFFC) as u16).unwrap_or(0);
            let bus_master_base = pci_device.bar(1).map(|b| (b.address & 0xFFFC) as u16).unwrap_or(0);
            
            if mixer_base != 0 && bus_master_base != 0 {
                // Enable bus mastering and I/O space
                let cmd = pci_device.command();
                pci_device.set_command(cmd | 0x05);
                
                let device_id = crate::sound::NEXT_DEVICE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
                let mut controller = Ac97Controller::new(device_id, mixer_base, bus_master_base);
                
                // Try to initialize
                if controller.init_device().is_ok() {
                    return Some(Box::new(controller));
                }
            }
        }
    }
    
    None
}

/// Create a controller instance with known I/O ports
pub fn create(mixer_base: u16, bus_master_base: u16) -> Ac97Controller {
    Ac97Controller::new(0, mixer_base, bus_master_base)
}
