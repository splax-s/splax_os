//! # Sound Subsystem for S-DEV
//!
//! Userspace audio device management and mixing.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::driver::AudioFormat;
use super::DevError;

/// Audio direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDirection {
    /// Playback (output)
    Playback,
    /// Capture (input)
    Capture,
}

/// Audio stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream is closed
    Closed,
    /// Stream is open but not running
    Open,
    /// Stream is running
    Running,
    /// Stream is paused
    Paused,
    /// Stream error
    Error,
}

/// Audio stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Audio format
    pub format: AudioFormat,
    /// Buffer size in frames
    pub buffer_frames: u32,
    /// Period size in frames
    pub period_frames: u32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            format: AudioFormat::S16Le,
            buffer_frames: 2048,
            period_frames: 512,
        }
    }
}

/// Audio stream
#[derive(Debug)]
pub struct AudioStream {
    /// Stream ID
    pub id: u32,
    /// Stream direction
    pub direction: AudioDirection,
    /// Configuration
    pub config: StreamConfig,
    /// Current state
    pub state: StreamState,
    /// Ring buffer
    buffer: Vec<u8>,
    /// Write position
    write_pos: usize,
    /// Read position
    read_pos: usize,
    /// Underrun count
    pub underruns: u64,
    /// Overrun count
    pub overruns: u64,
}

impl AudioStream {
    /// Creates a new audio stream
    pub fn new(id: u32, direction: AudioDirection, config: StreamConfig) -> Self {
        let buffer_size = config.buffer_frames as usize
            * config.channels as usize
            * Self::format_bytes(&config.format);

        Self {
            id,
            direction,
            config,
            state: StreamState::Open,
            buffer: alloc::vec![0u8; buffer_size],
            write_pos: 0,
            read_pos: 0,
            underruns: 0,
            overruns: 0,
        }
    }

    /// Gets bytes per sample for a format
    fn format_bytes(format: &AudioFormat) -> usize {
        match format {
            AudioFormat::S16Le => 2,
            AudioFormat::S24Le => 3,
            AudioFormat::S32Le | AudioFormat::Float32 => 4,
        }
    }

    /// Returns available space for writing
    pub fn available_write(&self) -> usize {
        let used = if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.buffer.len() - self.read_pos + self.write_pos
        };
        self.buffer.len() - used - 1
    }

    /// Returns available data for reading
    pub fn available_read(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.buffer.len() - self.read_pos + self.write_pos
        }
    }

    /// Writes data to the stream
    pub fn write(&mut self, data: &[u8]) -> usize {
        let available = self.available_write();
        let to_write = data.len().min(available);

        for i in 0..to_write {
            self.buffer[self.write_pos] = data[i];
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
        }

        if to_write < data.len() {
            self.overruns += 1;
        }

        to_write
    }

    /// Reads data from the stream
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let available = self.available_read();
        let to_read = buf.len().min(available);

        for i in 0..to_read {
            buf[i] = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
        }

        if to_read < buf.len() {
            self.underruns += 1;
        }

        to_read
    }

    /// Starts the stream
    pub fn start(&mut self) {
        if self.state == StreamState::Open || self.state == StreamState::Paused {
            self.state = StreamState::Running;
        }
    }

    /// Pauses the stream
    pub fn pause(&mut self) {
        if self.state == StreamState::Running {
            self.state = StreamState::Paused;
        }
    }

    /// Stops the stream
    pub fn stop(&mut self) {
        self.state = StreamState::Open;
        self.write_pos = 0;
        self.read_pos = 0;
    }

    /// Returns frame size in bytes
    pub fn frame_size(&self) -> usize {
        self.config.channels as usize * Self::format_bytes(&self.config.format)
    }
}

/// Audio device
#[derive(Debug, Clone)]
pub struct AudioDevice {
    /// Device ID
    pub id: u32,
    /// Device name
    pub name: String,
    /// Supported sample rates
    pub sample_rates: Vec<u32>,
    /// Supported formats
    pub formats: Vec<AudioFormat>,
    /// Maximum channels
    pub max_channels: u8,
    /// Has playback capability
    pub playback: bool,
    /// Has capture capability
    pub capture: bool,
}

/// Volume control
#[derive(Debug, Clone, Copy)]
pub struct Volume {
    /// Left channel (0-100)
    pub left: u8,
    /// Right channel (0-100)
    pub right: u8,
    /// Muted
    pub muted: bool,
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            left: 100,
            right: 100,
            muted: false,
        }
    }
}

/// Audio mixer channel
#[derive(Debug, Clone)]
pub struct MixerChannel {
    /// Channel name
    pub name: String,
    /// Volume
    pub volume: Volume,
    /// Is capture channel
    pub is_capture: bool,
}

/// Audio mixer
#[derive(Debug)]
pub struct AudioMixer {
    /// Master volume
    pub master: Volume,
    /// Channels
    pub channels: BTreeMap<String, MixerChannel>,
}

impl AudioMixer {
    /// Creates a new mixer
    pub fn new() -> Self {
        Self {
            master: Volume::default(),
            channels: BTreeMap::new(),
        }
    }

    /// Adds a channel
    pub fn add_channel(&mut self, name: &str, is_capture: bool) {
        self.channels.insert(
            String::from(name),
            MixerChannel {
                name: String::from(name),
                volume: Volume::default(),
                is_capture,
            },
        );
    }

    /// Sets master volume
    pub fn set_master(&mut self, volume: Volume) {
        self.master = volume;
    }

    /// Sets channel volume
    pub fn set_channel_volume(&mut self, name: &str, volume: Volume) -> bool {
        if let Some(channel) = self.channels.get_mut(name) {
            channel.volume = volume;
            true
        } else {
            false
        }
    }

    /// Gets effective volume for a channel
    pub fn effective_volume(&self, name: &str) -> Option<Volume> {
        let channel = self.channels.get(name)?;
        
        if self.master.muted || channel.volume.muted {
            return Some(Volume {
                left: 0,
                right: 0,
                muted: true,
            });
        }

        Some(Volume {
            left: (channel.volume.left as u16 * self.master.left as u16 / 100) as u8,
            right: (channel.volume.right as u16 * self.master.right as u16 / 100) as u8,
            muted: false,
        })
    }
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

/// Sound subsystem manager
pub struct SoundManager {
    /// Audio devices
    devices: BTreeMap<u32, AudioDevice>,
    /// Active streams
    streams: BTreeMap<u32, AudioStream>,
    /// Audio mixer
    pub mixer: AudioMixer,
    /// Next stream ID
    next_stream_id: u32,
    /// Next device ID
    next_device_id: u32,
    /// Default playback device
    default_playback: Option<u32>,
    /// Default capture device
    default_capture: Option<u32>,
}

impl SoundManager {
    /// Creates a new sound manager
    pub fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
            streams: BTreeMap::new(),
            mixer: AudioMixer::new(),
            next_stream_id: 1,
            next_device_id: 1,
            default_playback: None,
            default_capture: None,
        }
    }

    /// Registers an audio device
    pub fn register_device(&mut self, device: AudioDevice) -> u32 {
        let id = self.next_device_id;
        self.next_device_id += 1;

        // Set as default if first device
        if device.playback && self.default_playback.is_none() {
            self.default_playback = Some(id);
        }
        if device.capture && self.default_capture.is_none() {
            self.default_capture = Some(id);
        }

        // Add mixer channels
        if device.playback {
            self.mixer.add_channel(&device.name, false);
        }
        if device.capture {
            let capture_name = alloc::format!("{} Capture", device.name);
            self.mixer.add_channel(&capture_name, true);
        }

        self.devices.insert(id, device);
        id
    }

    /// Unregisters an audio device
    pub fn unregister_device(&mut self, id: u32) -> Option<AudioDevice> {
        if self.default_playback == Some(id) {
            self.default_playback = None;
        }
        if self.default_capture == Some(id) {
            self.default_capture = None;
        }
        self.devices.remove(&id)
    }

    /// Opens a stream
    pub fn open_stream(
        &mut self,
        _device_id: u32,
        direction: AudioDirection,
        config: StreamConfig,
    ) -> Result<u32, DevError> {
        let id = self.next_stream_id;
        self.next_stream_id += 1;

        let stream = AudioStream::new(id, direction, config);
        self.streams.insert(id, stream);
        Ok(id)
    }

    /// Closes a stream
    pub fn close_stream(&mut self, stream_id: u32) -> Result<(), DevError> {
        self.streams.remove(&stream_id).ok_or(DevError::DeviceNotFound)?;
        Ok(())
    }

    /// Gets a stream
    pub fn get_stream(&mut self, stream_id: u32) -> Option<&mut AudioStream> {
        self.streams.get_mut(&stream_id)
    }

    /// Writes to a playback stream
    pub fn write_stream(&mut self, stream_id: u32, data: &[u8]) -> Result<usize, DevError> {
        let stream = self.streams.get_mut(&stream_id).ok_or(DevError::DeviceNotFound)?;
        
        if stream.direction != AudioDirection::Playback {
            return Err(DevError::InvalidArgument);
        }
        
        Ok(stream.write(data))
    }

    /// Reads from a capture stream
    pub fn read_stream(&mut self, stream_id: u32, buf: &mut [u8]) -> Result<usize, DevError> {
        let stream = self.streams.get_mut(&stream_id).ok_or(DevError::DeviceNotFound)?;
        
        if stream.direction != AudioDirection::Capture {
            return Err(DevError::InvalidArgument);
        }
        
        Ok(stream.read(buf))
    }

    /// Lists all devices
    pub fn list_devices(&self) -> Vec<&AudioDevice> {
        self.devices.values().collect()
    }

    /// Gets the default playback device
    pub fn default_playback_device(&self) -> Option<&AudioDevice> {
        self.default_playback.and_then(|id| self.devices.get(&id))
    }

    /// Gets the default capture device
    pub fn default_capture_device(&self) -> Option<&AudioDevice> {
        self.default_capture.and_then(|id| self.devices.get(&id))
    }

    /// Sets master volume
    pub fn set_master_volume(&mut self, volume: Volume) {
        self.mixer.set_master(volume);
    }

    /// Gets master volume
    pub fn master_volume(&self) -> Volume {
        self.mixer.master
    }
}

impl Default for SoundManager {
    fn default() -> Self {
        Self::new()
    }
}
