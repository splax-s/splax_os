//! # Sound Subsystem
//!
//! This module provides audio support for Splax OS, including
//! High Definition Audio (HDA) and VirtIO-snd drivers.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                   Audio Applications                     │
//! ├─────────────────────────────────────────────────────────┤
//! │                    Audio Mixer                           │
//! ├─────────────────────────────────────────────────────────┤
//! │    PCM Stream     │    MIDI     │    Control Interface   │
//! ├───────────────────┴─────────────┴───────────────────────┤
//! │                   Audio Core                             │
//! ├─────────────────────────────────────────────────────────┤
//! │   HDA Driver   │   VirtIO-snd   │   AC97   │   USB Audio │
//! └─────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

pub mod ac97;
pub mod hda;
pub mod usb_audio;
pub mod virtio_snd;

// =============================================================================
// Audio Types
// =============================================================================

/// Audio sample format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed 8-bit
    S8,
    /// Unsigned 8-bit
    U8,
    /// Signed 16-bit little-endian
    S16Le,
    /// Signed 16-bit big-endian
    S16Be,
    /// Unsigned 16-bit little-endian
    U16Le,
    /// Signed 24-bit little-endian (packed in 32 bits)
    S24Le,
    /// Signed 32-bit little-endian
    S32Le,
    /// 32-bit float little-endian
    F32Le,
}

impl SampleFormat {
    /// Returns bytes per sample
    pub const fn bytes_per_sample(&self) -> usize {
        match self {
            SampleFormat::S8 | SampleFormat::U8 => 1,
            SampleFormat::S16Le | SampleFormat::S16Be | SampleFormat::U16Le => 2,
            SampleFormat::S24Le | SampleFormat::S32Le | SampleFormat::F32Le => 4,
        }
    }
}

/// Audio stream direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    /// Playback (output)
    Playback,
    /// Capture (input)
    Capture,
}

/// Audio stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Sample format
    pub format: SampleFormat,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Period size in frames
    pub period_frames: u32,
    /// Number of periods in buffer
    pub periods: u32,
}

impl StreamConfig {
    /// Creates a new stream configuration
    pub fn new(format: SampleFormat, sample_rate: u32, channels: u8) -> Self {
        Self {
            format,
            sample_rate,
            channels,
            period_frames: 1024,
            periods: 4,
        }
    }
    
    /// Returns bytes per frame (all channels)
    pub fn bytes_per_frame(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }
    
    /// Returns total buffer size in bytes
    pub fn buffer_size(&self) -> usize {
        self.bytes_per_frame() * self.period_frames as usize * self.periods as usize
    }
    
    /// Returns period size in bytes
    pub fn period_size(&self) -> usize {
        self.bytes_per_frame() * self.period_frames as usize
    }
    
    /// Standard CD quality: 44.1kHz, 16-bit stereo
    pub fn cd_quality() -> Self {
        Self::new(SampleFormat::S16Le, 44100, 2)
    }
    
    /// Standard DVD quality: 48kHz, 16-bit stereo
    pub fn dvd_quality() -> Self {
        Self::new(SampleFormat::S16Le, 48000, 2)
    }
    
    /// High resolution: 96kHz, 24-bit stereo
    pub fn high_res() -> Self {
        Self::new(SampleFormat::S24Le, 96000, 2)
    }
}

/// Audio stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is stopped
    Stopped,
    /// Stream is running
    Running,
    /// Stream is paused
    Paused,
    /// Stream is draining (finishing playback)
    Draining,
}

/// Audio stream handle
pub type StreamId = u32;

/// Audio device handle
pub type DeviceId = u32;

// =============================================================================
// Audio Device Interface
// =============================================================================

/// Audio device capabilities
#[derive(Debug, Clone)]
pub struct DeviceCapabilities {
    /// Supported sample formats
    pub formats: Vec<SampleFormat>,
    /// Minimum sample rate
    pub min_sample_rate: u32,
    /// Maximum sample rate
    pub max_sample_rate: u32,
    /// Minimum channels
    pub min_channels: u8,
    /// Maximum channels
    pub max_channels: u8,
    /// Supported stream directions
    pub directions: Vec<StreamDirection>,
}

impl Default for DeviceCapabilities {
    fn default() -> Self {
        Self {
            formats: alloc::vec![SampleFormat::S16Le],
            min_sample_rate: 8000,
            max_sample_rate: 48000,
            min_channels: 1,
            max_channels: 2,
            directions: alloc::vec![StreamDirection::Playback],
        }
    }
}

/// Audio device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device ID
    pub id: DeviceId,
    /// Device name
    pub name: String,
    /// Device description
    pub description: String,
    /// Device type
    pub device_type: DeviceType,
    /// Device capabilities
    pub capabilities: DeviceCapabilities,
}

/// Audio device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// Intel HDA
    Hda,
    /// VirtIO sound device
    VirtioSnd,
    /// AC'97 codec
    Ac97,
    /// USB audio class
    UsbAudio,
    /// Software mixer (virtual)
    Mixer,
}

/// Audio device driver trait
pub trait AudioDevice: Send + Sync {
    /// Gets device information
    fn info(&self) -> DeviceInfo;
    
    /// Opens a stream
    fn open_stream(
        &mut self,
        direction: StreamDirection,
        config: &StreamConfig,
    ) -> Result<StreamId, AudioError>;
    
    /// Closes a stream
    fn close_stream(&mut self, stream: StreamId) -> Result<(), AudioError>;
    
    /// Starts a stream
    fn start_stream(&mut self, stream: StreamId) -> Result<(), AudioError>;
    
    /// Stops a stream
    fn stop_stream(&mut self, stream: StreamId) -> Result<(), AudioError>;
    
    /// Pauses a stream
    fn pause_stream(&mut self, stream: StreamId) -> Result<(), AudioError>;
    
    /// Resumes a stream
    fn resume_stream(&mut self, stream: StreamId) -> Result<(), AudioError>;
    
    /// Writes audio data to a playback stream
    fn write(&mut self, stream: StreamId, data: &[u8]) -> Result<usize, AudioError>;
    
    /// Reads audio data from a capture stream
    fn read(&mut self, stream: StreamId, data: &mut [u8]) -> Result<usize, AudioError>;
    
    /// Gets stream state
    fn stream_state(&self, stream: StreamId) -> Result<StreamState, AudioError>;
    
    /// Gets available space in write buffer (for playback)
    fn available_write(&self, stream: StreamId) -> Result<usize, AudioError>;
    
    /// Gets available data in read buffer (for capture)
    fn available_read(&self, stream: StreamId) -> Result<usize, AudioError>;
    
    /// Sets stream volume (0-100)
    fn set_volume(&mut self, stream: StreamId, volume: u8) -> Result<(), AudioError>;
    
    /// Gets stream volume
    fn get_volume(&self, stream: StreamId) -> Result<u8, AudioError>;
    
    /// Mutes/unmutes a stream
    fn set_mute(&mut self, stream: StreamId, mute: bool) -> Result<(), AudioError>;
    
    /// Gets mute state
    fn is_muted(&self, stream: StreamId) -> Result<bool, AudioError>;
}

/// Audio error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    /// Device not found
    DeviceNotFound,
    /// Stream not found
    StreamNotFound,
    /// Invalid configuration
    InvalidConfig,
    /// Unsupported format
    UnsupportedFormat,
    /// Unsupported sample rate
    UnsupportedSampleRate,
    /// Device busy
    DeviceBusy,
    /// Buffer underrun (playback)
    Underrun,
    /// Buffer overrun (capture)
    Overrun,
    /// I/O error
    IoError,
    /// No data available
    WouldBlock,
    /// Device not initialized
    NotInitialized,
    /// Invalid parameter
    InvalidParameter,
    /// Not enough memory
    OutOfMemory,
    /// Operation not supported
    NotSupported,
    /// Stream already running
    AlreadyRunning,
    /// Stream already stopped
    AlreadyStopped,
}

// =============================================================================
// Audio Core
// =============================================================================

/// Audio subsystem core
pub struct AudioCore {
    /// Registered audio devices
    devices: BTreeMap<DeviceId, Box<dyn AudioDevice>>,
    /// Next device ID
    next_device_id: AtomicU32,
    /// Default playback device
    default_playback: Option<DeviceId>,
    /// Default capture device
    default_capture: Option<DeviceId>,
    /// Master volume (0-100)
    master_volume: u8,
    /// Master mute
    master_mute: bool,
}

impl AudioCore {
    /// Creates a new audio core
    pub const fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
            next_device_id: AtomicU32::new(1),
            default_playback: None,
            default_capture: None,
            master_volume: 100,
            master_mute: false,
        }
    }
    
    /// Registers an audio device
    pub fn register_device(&mut self, device: Box<dyn AudioDevice>) -> DeviceId {
        let id = self.next_device_id.fetch_add(1, Ordering::SeqCst);
        
        // Set as default if first device
        let info = device.info();
        if self.default_playback.is_none() 
            && info.capabilities.directions.contains(&StreamDirection::Playback) 
        {
            self.default_playback = Some(id);
        }
        if self.default_capture.is_none() 
            && info.capabilities.directions.contains(&StreamDirection::Capture) 
        {
            self.default_capture = Some(id);
        }
        
        self.devices.insert(id, device);
        id
    }
    
    /// Unregisters an audio device
    pub fn unregister_device(&mut self, id: DeviceId) -> Option<Box<dyn AudioDevice>> {
        if self.default_playback == Some(id) {
            self.default_playback = None;
        }
        if self.default_capture == Some(id) {
            self.default_capture = None;
        }
        self.devices.remove(&id)
    }
    
    /// Gets a device by ID
    pub fn device(&self, id: DeviceId) -> Option<&dyn AudioDevice> {
        match self.devices.get(&id) {
            Some(d) => Some(d.as_ref()),
            None => None,
        }
    }
    
    /// Gets a mutable device by ID
    pub fn device_mut(&mut self, id: DeviceId) -> Option<&mut dyn AudioDevice> {
        match self.devices.get_mut(&id) {
            Some(d) => Some(d.as_mut()),
            None => None,
        }
    }
    
    /// Lists all registered devices
    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices.values().map(|d| d.info()).collect()
    }
    
    /// Gets the default playback device
    pub fn default_playback_device(&self) -> Option<DeviceId> {
        self.default_playback
    }
    
    /// Gets the default capture device
    pub fn default_capture_device(&self) -> Option<DeviceId> {
        self.default_capture
    }
    
    /// Sets the default playback device
    pub fn set_default_playback(&mut self, id: DeviceId) -> Result<(), AudioError> {
        if self.devices.contains_key(&id) {
            self.default_playback = Some(id);
            Ok(())
        } else {
            Err(AudioError::DeviceNotFound)
        }
    }
    
    /// Sets the default capture device
    pub fn set_default_capture(&mut self, id: DeviceId) -> Result<(), AudioError> {
        if self.devices.contains_key(&id) {
            self.default_capture = Some(id);
            Ok(())
        } else {
            Err(AudioError::DeviceNotFound)
        }
    }
    
    /// Sets master volume
    pub fn set_master_volume(&mut self, volume: u8) {
        self.master_volume = volume.min(100);
    }
    
    /// Gets master volume
    pub fn master_volume(&self) -> u8 {
        self.master_volume
    }
    
    /// Sets master mute
    pub fn set_master_mute(&mut self, mute: bool) {
        self.master_mute = mute;
    }
    
    /// Gets master mute state
    pub fn is_master_muted(&self) -> bool {
        self.master_mute
    }
}

// =============================================================================
// Ring Buffer for Audio Streaming
// =============================================================================

/// Ring buffer for audio data
pub struct AudioRingBuffer {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
    size: usize,
}

impl AudioRingBuffer {
    /// Creates a new ring buffer
    pub fn new(size: usize) -> Self {
        Self {
            buffer: alloc::vec![0u8; size],
            read_pos: 0,
            write_pos: 0,
            size,
        }
    }
    
    /// Returns the capacity of the buffer
    pub fn capacity(&self) -> usize {
        self.size
    }
    
    /// Returns the number of bytes available for reading
    pub fn available_read(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.size - self.read_pos + self.write_pos
        }
    }
    
    /// Returns the number of bytes available for writing
    pub fn available_write(&self) -> usize {
        self.size - self.available_read() - 1
    }
    
    /// Writes data to the buffer
    pub fn write(&mut self, data: &[u8]) -> usize {
        let available = self.available_write();
        let to_write = data.len().min(available);
        
        for i in 0..to_write {
            self.buffer[self.write_pos] = data[i];
            self.write_pos = (self.write_pos + 1) % self.size;
        }
        
        to_write
    }
    
    /// Reads data from the buffer
    pub fn read(&mut self, data: &mut [u8]) -> usize {
        let available = self.available_read();
        let to_read = data.len().min(available);
        
        for i in 0..to_read {
            data[i] = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.size;
        }
        
        to_read
    }
    
    /// Clears the buffer
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
    }
    
    /// Returns true if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.read_pos == self.write_pos
    }
    
    /// Returns true if the buffer is full
    pub fn is_full(&self) -> bool {
        self.available_write() == 0
    }
}

// =============================================================================
// PCM Stream
// =============================================================================

/// PCM stream for audio playback/capture
pub struct PcmStream {
    /// Device ID
    device_id: DeviceId,
    /// Stream ID on the device
    stream_id: StreamId,
    /// Stream direction
    direction: StreamDirection,
    /// Stream configuration
    config: StreamConfig,
    /// Stream state
    state: StreamState,
    /// Volume (0-100)
    volume: u8,
    /// Muted
    muted: bool,
}

impl PcmStream {
    /// Creates a new PCM stream
    pub fn new(
        device_id: DeviceId,
        stream_id: StreamId,
        direction: StreamDirection,
        config: StreamConfig,
    ) -> Self {
        Self {
            device_id,
            stream_id,
            direction,
            config,
            state: StreamState::Stopped,
            volume: 100,
            muted: false,
        }
    }
    
    /// Gets the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
    
    /// Gets the stream ID
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }
    
    /// Gets the stream direction
    pub fn direction(&self) -> StreamDirection {
        self.direction
    }
    
    /// Gets the stream configuration
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }
    
    /// Gets the stream state
    pub fn state(&self) -> StreamState {
        self.state
    }
    
    /// Gets the volume
    pub fn volume(&self) -> u8 {
        self.volume
    }
    
    /// Sets the volume
    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }
    
    /// Gets the mute state
    pub fn is_muted(&self) -> bool {
        self.muted
    }
    
    /// Sets the mute state
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }
}

// =============================================================================
// Global Instance
// =============================================================================

/// Global audio core instance
pub static AUDIO_CORE: Mutex<AudioCore> = Mutex::new(AudioCore::new());

/// Initializes the audio subsystem
pub fn init() {
    // Probe for HDA devices
    if let Some(hda) = hda::probe() {
        let _ = AUDIO_CORE.lock().register_device(hda);
    }
    
    // Probe for VirtIO-snd devices
    if let Some(virtio) = virtio_snd::probe() {
        let _ = AUDIO_CORE.lock().register_device(virtio);
    }
}

/// Lists all audio devices
pub fn list_devices() -> Vec<DeviceInfo> {
    AUDIO_CORE.lock().list_devices()
}

/// Opens a playback stream on the default device
pub fn open_playback(config: &StreamConfig) -> Result<(DeviceId, StreamId), AudioError> {
    let mut core = AUDIO_CORE.lock();
    let device_id = core.default_playback_device().ok_or(AudioError::DeviceNotFound)?;
    let device = core.device_mut(device_id).ok_or(AudioError::DeviceNotFound)?;
    let stream_id = device.open_stream(StreamDirection::Playback, config)?;
    Ok((device_id, stream_id))
}

/// Opens a capture stream on the default device
pub fn open_capture(config: &StreamConfig) -> Result<(DeviceId, StreamId), AudioError> {
    let mut core = AUDIO_CORE.lock();
    let device_id = core.default_capture_device().ok_or(AudioError::DeviceNotFound)?;
    let device = core.device_mut(device_id).ok_or(AudioError::DeviceNotFound)?;
    let stream_id = device.open_stream(StreamDirection::Capture, config)?;
    Ok((device_id, stream_id))
}

/// Sets the master volume
pub fn set_master_volume(volume: u8) {
    AUDIO_CORE.lock().set_master_volume(volume);
}

/// Gets the master volume
pub fn master_volume() -> u8 {
    AUDIO_CORE.lock().master_volume()
}

/// Sets master mute
pub fn set_master_mute(mute: bool) {
    AUDIO_CORE.lock().set_master_mute(mute);
}

/// Gets master mute state
pub fn is_master_muted() -> bool {
    AUDIO_CORE.lock().is_master_muted()
}
