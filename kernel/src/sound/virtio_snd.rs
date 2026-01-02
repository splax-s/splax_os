//! # VirtIO Sound Driver
//!
//! Implements support for VirtIO sound devices (virtio-snd).
//!
//! ## VirtIO Sound Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    VirtIO Sound Device                   │
//! ├─────────────────────────────────────────────────────────┤
//! │  Control VQ  │  Event VQ  │  TX VQ (out) │  RX VQ (in)  │
//! ├──────────────┴────────────┴──────────────┴──────────────┤
//! │                    PCM Streams                           │
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
// VirtIO Sound Constants
// =============================================================================

/// VirtIO Sound device ID
pub const VIRTIO_DEVICE_ID_SOUND: u32 = 25;

/// VirtIO Sound feature bits
#[allow(dead_code)]
mod features {
    /// Device has control queues
    pub const VIRTIO_SND_F_CTLS: u64 = 1 << 0;
}

/// VirtIO Sound configuration space
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioSndConfig {
    /// Number of available jacks
    pub jacks: u32,
    /// Number of available PCM streams
    pub streams: u32,
    /// Number of available channel maps
    pub chmaps: u32,
}

/// VirtIO Sound control message types
#[allow(dead_code)]
mod ctrl_type {
    /// Jack info
    pub const VIRTIO_SND_R_JACK_INFO: u32 = 1;
    /// Jack remap
    pub const VIRTIO_SND_R_JACK_REMAP: u32 = 2;
    /// PCM info
    pub const VIRTIO_SND_R_PCM_INFO: u32 = 0x100;
    /// PCM set params
    pub const VIRTIO_SND_R_PCM_SET_PARAMS: u32 = 0x101;
    /// PCM prepare
    pub const VIRTIO_SND_R_PCM_PREPARE: u32 = 0x102;
    /// PCM release
    pub const VIRTIO_SND_R_PCM_RELEASE: u32 = 0x103;
    /// PCM start
    pub const VIRTIO_SND_R_PCM_START: u32 = 0x104;
    /// PCM stop
    pub const VIRTIO_SND_R_PCM_STOP: u32 = 0x105;
    /// Channel map info
    pub const VIRTIO_SND_R_CHMAP_INFO: u32 = 0x200;
    /// OK status
    pub const VIRTIO_SND_S_OK: u32 = 0x8000;
    /// Bad message
    pub const VIRTIO_SND_S_BAD_MSG: u32 = 0x8001;
    /// Not supported
    pub const VIRTIO_SND_S_NOT_SUPP: u32 = 0x8002;
    /// I/O error
    pub const VIRTIO_SND_S_IO_ERR: u32 = 0x8003;
}

/// VirtIO Sound PCM stream info
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioSndPcmInfo {
    /// Header code
    pub hdr: u32,
    /// Feature bits
    pub features: u32,
    /// Supported formats
    pub formats: u64,
    /// Supported rates
    pub rates: u64,
    /// Direction (VIRTIO_SND_D_OUTPUT or VIRTIO_SND_D_INPUT)
    pub direction: u8,
    /// Minimum number of channels
    pub channels_min: u8,
    /// Maximum number of channels
    pub channels_max: u8,
    /// Padding
    pub _padding: [u8; 5],
}

/// VirtIO Sound PCM direction
#[allow(dead_code)]
mod direction {
    pub const VIRTIO_SND_D_OUTPUT: u8 = 0;
    pub const VIRTIO_SND_D_INPUT: u8 = 1;
}

/// VirtIO Sound PCM formats
#[allow(dead_code)]
mod formats {
    pub const VIRTIO_SND_PCM_FMT_IMA_ADPCM: u64 = 1 << 0;
    pub const VIRTIO_SND_PCM_FMT_MU_LAW: u64 = 1 << 1;
    pub const VIRTIO_SND_PCM_FMT_A_LAW: u64 = 1 << 2;
    pub const VIRTIO_SND_PCM_FMT_S8: u64 = 1 << 3;
    pub const VIRTIO_SND_PCM_FMT_U8: u64 = 1 << 4;
    pub const VIRTIO_SND_PCM_FMT_S16: u64 = 1 << 5;
    pub const VIRTIO_SND_PCM_FMT_U16: u64 = 1 << 6;
    pub const VIRTIO_SND_PCM_FMT_S18_3: u64 = 1 << 7;
    pub const VIRTIO_SND_PCM_FMT_U18_3: u64 = 1 << 8;
    pub const VIRTIO_SND_PCM_FMT_S20_3: u64 = 1 << 9;
    pub const VIRTIO_SND_PCM_FMT_U20_3: u64 = 1 << 10;
    pub const VIRTIO_SND_PCM_FMT_S24_3: u64 = 1 << 11;
    pub const VIRTIO_SND_PCM_FMT_U24_3: u64 = 1 << 12;
    pub const VIRTIO_SND_PCM_FMT_S20: u64 = 1 << 13;
    pub const VIRTIO_SND_PCM_FMT_U20: u64 = 1 << 14;
    pub const VIRTIO_SND_PCM_FMT_S24: u64 = 1 << 15;
    pub const VIRTIO_SND_PCM_FMT_U24: u64 = 1 << 16;
    pub const VIRTIO_SND_PCM_FMT_S32: u64 = 1 << 17;
    pub const VIRTIO_SND_PCM_FMT_U32: u64 = 1 << 18;
    pub const VIRTIO_SND_PCM_FMT_FLOAT: u64 = 1 << 19;
    pub const VIRTIO_SND_PCM_FMT_FLOAT64: u64 = 1 << 20;
}

/// VirtIO Sound PCM rates
#[allow(dead_code)]
mod rates {
    pub const VIRTIO_SND_PCM_RATE_5512: u64 = 1 << 0;
    pub const VIRTIO_SND_PCM_RATE_8000: u64 = 1 << 1;
    pub const VIRTIO_SND_PCM_RATE_11025: u64 = 1 << 2;
    pub const VIRTIO_SND_PCM_RATE_16000: u64 = 1 << 3;
    pub const VIRTIO_SND_PCM_RATE_22050: u64 = 1 << 4;
    pub const VIRTIO_SND_PCM_RATE_32000: u64 = 1 << 5;
    pub const VIRTIO_SND_PCM_RATE_44100: u64 = 1 << 6;
    pub const VIRTIO_SND_PCM_RATE_48000: u64 = 1 << 7;
    pub const VIRTIO_SND_PCM_RATE_64000: u64 = 1 << 8;
    pub const VIRTIO_SND_PCM_RATE_88200: u64 = 1 << 9;
    pub const VIRTIO_SND_PCM_RATE_96000: u64 = 1 << 10;
    pub const VIRTIO_SND_PCM_RATE_176400: u64 = 1 << 11;
    pub const VIRTIO_SND_PCM_RATE_192000: u64 = 1 << 12;
    pub const VIRTIO_SND_PCM_RATE_384000: u64 = 1 << 13;
}

// =============================================================================
// VirtIO Sound Stream
// =============================================================================

/// VirtIO Sound stream
pub struct VirtioSndStream {
    /// Stream ID
    pub id: StreamId,
    /// Hardware stream index
    pub hw_stream: u32,
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
}

impl VirtioSndStream {
    /// Creates a new VirtIO sound stream
    pub fn new(id: StreamId, hw_stream: u32, direction: StreamDirection, config: StreamConfig) -> Self {
        let buffer_size = config.buffer_size();
        Self {
            id,
            hw_stream,
            direction,
            config,
            state: StreamState::Stopped,
            buffer: AudioRingBuffer::new(buffer_size),
            volume: 100,
            muted: false,
        }
    }
}

// =============================================================================
// VirtIO Sound Device
// =============================================================================

/// VirtIO common configuration register offsets
mod virtio_regs {
    pub const DEVICE_FEATURES: u64 = 0x00;
    pub const DRIVER_FEATURES: u64 = 0x20;
    pub const DEVICE_STATUS: u64 = 0x14;
    pub const QUEUE_SELECT: u64 = 0x30;
    pub const QUEUE_SIZE: u64 = 0x38;
    pub const QUEUE_READY: u64 = 0x44;
}

/// VirtIO device status bits
mod virtio_status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
}

/// VirtIO Sound device
pub struct VirtioSndDevice {
    /// Device configuration
    config: VirtioSndConfig,
    /// Active streams
    streams: BTreeMap<StreamId, VirtioSndStream>,
    /// Next stream ID
    next_stream_id: AtomicU32,
    /// Available output stream indices
    available_output: Vec<u32>,
    /// Available input stream indices
    available_input: Vec<u32>,
    /// MMIO base address for VirtIO common config
    mmio_base: u64,
}

impl VirtioSndDevice {
    /// Creates a new VirtIO sound device
    pub fn new(config: VirtioSndConfig) -> Self {
        // Assume half streams are output, half are input
        let num_output = config.streams / 2;
        let num_input = config.streams - num_output;
        
        Self {
            config,
            streams: BTreeMap::new(),
            next_stream_id: AtomicU32::new(1),
            available_output: (0..num_output).collect(),
            available_input: (num_output..config.streams).collect(),
            mmio_base: 0, // Will be set during probe
        }
    }
    
    /// Creates a new VirtIO sound device with MMIO base address
    pub fn with_mmio(config: VirtioSndConfig, mmio_base: u64) -> Self {
        let num_output = config.streams / 2;
        let num_input = config.streams - num_output;
        
        Self {
            config,
            streams: BTreeMap::new(),
            next_stream_id: AtomicU32::new(1),
            available_output: (0..num_output).collect(),
            available_input: (num_output..config.streams).collect(),
            mmio_base,
        }
    }
    
    /// Read from VirtIO MMIO register
    fn read_reg(&self, offset: u64) -> u32 {
        if self.mmio_base == 0 {
            return 0;
        }
        unsafe { core::ptr::read_volatile((self.mmio_base + offset) as *const u32) }
    }
    
    /// Write to VirtIO MMIO register
    fn write_reg(&self, offset: u64, value: u32) {
        if self.mmio_base == 0 {
            return;
        }
        unsafe { core::ptr::write_volatile((self.mmio_base + offset) as *mut u32, value) }
    }
    
    /// Initializes the device
    pub fn init(&mut self) -> Result<(), AudioError> {
        // VirtIO sound device initialization sequence:
        
        if self.mmio_base != 0 {
            // Step 1: Reset device (write 0 to status register)
            self.write_reg(virtio_regs::DEVICE_STATUS, 0);
            
            // Step 2: Acknowledge the device
            self.write_reg(virtio_regs::DEVICE_STATUS, virtio_status::ACKNOWLEDGE as u32);
            
            // Step 3: Tell device we're a driver
            self.write_reg(virtio_regs::DEVICE_STATUS, 
                (virtio_status::ACKNOWLEDGE | virtio_status::DRIVER) as u32);
            
            // Step 4: Read and negotiate features
            let _features = self.read_reg(virtio_regs::DEVICE_FEATURES);
            // Accept basic features for now
            self.write_reg(virtio_regs::DRIVER_FEATURES, 0);
            
            // Step 5: Set FEATURES_OK
            self.write_reg(virtio_regs::DEVICE_STATUS,
                (virtio_status::ACKNOWLEDGE | virtio_status::DRIVER | virtio_status::FEATURES_OK) as u32);
            
            // Step 6: Set up virtqueues
            // Queue 0: controlq
            self.write_reg(virtio_regs::QUEUE_SELECT, 0);
            let _queue_size = self.read_reg(virtio_regs::QUEUE_SIZE);
            self.write_reg(virtio_regs::QUEUE_READY, 1);
            
            // Queue 1: eventq
            self.write_reg(virtio_regs::QUEUE_SELECT, 1);
            self.write_reg(virtio_regs::QUEUE_READY, 1);
            
            // Queue 2: txq (PCM output)
            self.write_reg(virtio_regs::QUEUE_SELECT, 2);
            self.write_reg(virtio_regs::QUEUE_READY, 1);
            
            // Queue 3: rxq (PCM input)
            self.write_reg(virtio_regs::QUEUE_SELECT, 3);
            self.write_reg(virtio_regs::QUEUE_READY, 1);
            
            // Step 7: Mark device as ready
            self.write_reg(virtio_regs::DEVICE_STATUS,
                (virtio_status::ACKNOWLEDGE | virtio_status::DRIVER | 
                 virtio_status::FEATURES_OK | virtio_status::DRIVER_OK) as u32);
            
            crate::serial_println!("[virtio-snd] Device initialized at MMIO 0x{:x}", self.mmio_base);
        }
        
        // Device is now initialized and ready for use
        Ok(())
    }
    
    /// Converts SampleFormat to VirtIO format code
    #[allow(dead_code)]
    fn format_to_virtio(format: SampleFormat) -> u64 {
        match format {
            SampleFormat::S8 => formats::VIRTIO_SND_PCM_FMT_S8,
            SampleFormat::U8 => formats::VIRTIO_SND_PCM_FMT_U8,
            SampleFormat::S16Le | SampleFormat::S16Be => formats::VIRTIO_SND_PCM_FMT_S16,
            SampleFormat::U16Le => formats::VIRTIO_SND_PCM_FMT_U16,
            SampleFormat::S24Le => formats::VIRTIO_SND_PCM_FMT_S24,
            SampleFormat::S32Le => formats::VIRTIO_SND_PCM_FMT_S32,
            SampleFormat::F32Le => formats::VIRTIO_SND_PCM_FMT_FLOAT,
        }
    }
    
    /// Converts sample rate to VirtIO rate code
    #[allow(dead_code)]
    fn rate_to_virtio(rate: u32) -> Option<u64> {
        Some(match rate {
            5512 => rates::VIRTIO_SND_PCM_RATE_5512,
            8000 => rates::VIRTIO_SND_PCM_RATE_8000,
            11025 => rates::VIRTIO_SND_PCM_RATE_11025,
            16000 => rates::VIRTIO_SND_PCM_RATE_16000,
            22050 => rates::VIRTIO_SND_PCM_RATE_22050,
            32000 => rates::VIRTIO_SND_PCM_RATE_32000,
            44100 => rates::VIRTIO_SND_PCM_RATE_44100,
            48000 => rates::VIRTIO_SND_PCM_RATE_48000,
            64000 => rates::VIRTIO_SND_PCM_RATE_64000,
            88200 => rates::VIRTIO_SND_PCM_RATE_88200,
            96000 => rates::VIRTIO_SND_PCM_RATE_96000,
            176400 => rates::VIRTIO_SND_PCM_RATE_176400,
            192000 => rates::VIRTIO_SND_PCM_RATE_192000,
            384000 => rates::VIRTIO_SND_PCM_RATE_384000,
            _ => return None,
        })
    }
}

impl AudioDevice for VirtioSndDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: 0,
            name: "VirtIO Sound".to_string(),
            description: "VirtIO Sound Device".to_string(),
            device_type: DeviceType::VirtioSnd,
            capabilities: DeviceCapabilities {
                formats: alloc::vec![
                    SampleFormat::S8,
                    SampleFormat::U8,
                    SampleFormat::S16Le,
                    SampleFormat::S24Le,
                    SampleFormat::S32Le,
                    SampleFormat::F32Le,
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
        // Get an available hardware stream
        let hw_stream = match direction {
            StreamDirection::Playback => {
                self.available_output.pop().ok_or(AudioError::DeviceBusy)?
            }
            StreamDirection::Capture => {
                self.available_input.pop().ok_or(AudioError::DeviceBusy)?
            }
        };
        
        let stream_id = self.next_stream_id.fetch_add(1, Ordering::SeqCst);
        let stream = VirtioSndStream::new(stream_id, hw_stream, direction, config.clone());
        self.streams.insert(stream_id, stream);
        
        Ok(stream_id)
    }
    
    fn close_stream(&mut self, stream: StreamId) -> Result<(), AudioError> {
        let s = self.streams.remove(&stream).ok_or(AudioError::StreamNotFound)?;
        
        // Return hardware stream to available pool
        match s.direction {
            StreamDirection::Playback => self.available_output.push(s.hw_stream),
            StreamDirection::Capture => self.available_input.push(s.hw_stream),
        }
        
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

/// Probes for VirtIO sound devices
pub fn probe() -> Option<Box<dyn AudioDevice>> {
    // VirtIO sound device ID is 25 (VIRTIO_ID_SOUND)
    // We scan for MMIO-based VirtIO devices at known addresses
    
    // Common VirtIO MMIO addresses for QEMU
    const VIRTIO_MMIO_ADDRESSES: &[usize] = &[
        0x0A00_0000,  // QEMU virt machine VirtIO MMIO base
        0x0A00_0200,
        0x0A00_0400,
        0x0A00_0600,
        0x0A00_0800,
    ];
    
    // VirtIO MMIO register offsets
    const VIRTIO_MMIO_MAGIC: usize = 0x000;
    const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
    const VIRTIO_MAGIC_VALUE: u32 = 0x74726976; // "virt"
    const VIRTIO_DEVICE_SOUND: u32 = 25;
    
    for &base in VIRTIO_MMIO_ADDRESSES {
        // Check for VirtIO magic value
        let magic = unsafe {
            core::ptr::read_volatile((base + VIRTIO_MMIO_MAGIC) as *const u32)
        };
        
        if magic != VIRTIO_MAGIC_VALUE {
            continue;
        }
        
        // Check device ID
        let device_id = unsafe {
            core::ptr::read_volatile((base + VIRTIO_MMIO_DEVICE_ID) as *const u32)
        };
        
        if device_id == VIRTIO_DEVICE_SOUND {
            // Found a VirtIO sound device
            let config = VirtioSndConfig {
                jacks: 0,
                streams: 4, // Default to 4 streams (2 output, 2 input)
                chmaps: 2,
            };
            
            let mut device = VirtioSndDevice::new(config);
            if device.init().is_ok() {
                return Some(Box::new(device));
            }
        }
    }
    
    // No VirtIO sound device found
    None
}
