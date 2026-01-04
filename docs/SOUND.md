# Sound Subsystem

> Comprehensive documentation for Splax OS audio functionality.

## Overview

The Splax OS sound subsystem provides a unified audio API supporting multiple audio backends including Intel High Definition Audio (HDA), VirtIO Sound, AC'97, USB Audio, and a low-latency audio engine for professional audio applications. It enables audio playback and capture with software mixing, real-time processing, and volume control.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Applications                               │
│                  Audio playback/capture                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Low-Latency Audio Engine                        │
│               (kernel/src/sound/lowlatency.rs)                  │
│    Lock-free buffers, audio graph, real-time processing         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AudioDevice Trait                            │
│                 (kernel/src/sound/mod.rs)                       │
│         Common interface for all audio drivers                  │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┬───────────────┐
          ▼                   ▼                   ▼               ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│       HDA        │ │   VirtIO Sound   │ │      AC'97       │ │   USB Audio      │
│  (Intel HD Audio)│ │  (QEMU/VMs)      │ │  (Legacy audio)  │ │  (UAC 1.0/2.0)   │
└──────────────────┘ └──────────────────┘ └──────────────────┘ └──────────────────┘
          │                   │                   │                   │
          ▼                   ▼                   ▼                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Hardware / PCI / USB                         │
└─────────────────────────────────────────────────────────────────┘
```

## Low-Latency Audio Engine

**Location:** `kernel/src/sound/lowlatency.rs`

Real-time audio processing with professional-grade latency:

### Features

- **Lock-Free Ring Buffer**: Zero-copy audio data transfer
- **Audio Graph**: Node-based processing pipeline
- **Node Types**:
  - Gain: Volume adjustment with dB control
  - Mixer: Multi-channel mixing
  - Delay: Sample-accurate delay lines
  - Filter: Biquad filters (low-pass, high-pass, band-pass, notch)
- **Priority Scheduling**: Real-time thread priority for audio callbacks
- **Configurable Latency**: Buffer sizes from 64 to 8192 samples

### Configuration

```rust
pub struct LowLatencyConfig {
    sample_rate: u32,      // 44100, 48000, 96000 Hz
    buffer_size: u32,      // 64-8192 samples
    channels: u32,         // 1-8 channels
    format: SampleFormat,  // F32, I16, I24, I32
}
```

---

## AudioDevice Trait

All audio drivers implement the common `AudioDevice` trait:

```rust
// kernel/src/sound/mod.rs

pub trait AudioDevice: Send + Sync {
    /// Get device name
    fn name(&self) -> &str;
    
    /// Get device capabilities
    fn capabilities(&self) -> AudioCapabilities;
    
    /// Initialize device
    fn init(&mut self) -> Result<(), AudioError>;
    
    /// Configure output stream
    fn configure_output(&mut self, config: &StreamConfig) -> Result<(), AudioError>;
    
    /// Configure input stream  
    fn configure_input(&mut self, config: &StreamConfig) -> Result<(), AudioError>;
    
    /// Start playback
    fn start_playback(&mut self) -> Result<(), AudioError>;
    
    /// Stop playback
    fn stop_playback(&mut self) -> Result<(), AudioError>;
    
    /// Start capture
    fn start_capture(&mut self) -> Result<(), AudioError>;
    
    /// Stop capture
    fn stop_capture(&mut self) -> Result<(), AudioError>;
    
    /// Write audio data (blocking)
    fn write(&mut self, data: &[u8]) -> Result<usize, AudioError>;
    
    /// Read audio data (blocking)
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, AudioError>;
    
    /// Set master volume (0-100)
    fn set_volume(&mut self, volume: u8) -> Result<(), AudioError>;
    
    /// Get master volume
    fn get_volume(&self) -> u8;
    
    /// Set mute state
    fn set_mute(&mut self, mute: bool) -> Result<(), AudioError>;
    
    /// Check if muted
    fn is_muted(&self) -> bool;
}
```

---

## Core Types

### Audio Capabilities

```rust
pub struct AudioCapabilities {
    /// Supported sample rates
    pub sample_rates: Vec<u32>,
    /// Supported sample formats
    pub formats: Vec<SampleFormat>,
    /// Maximum output channels
    pub max_output_channels: u8,
    /// Maximum input channels  
    pub max_input_channels: u8,
    /// Whether device supports playback
    pub supports_playback: bool,
    /// Whether device supports capture
    pub supports_capture: bool,
}
```

### Sample Format

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SampleFormat {
    /// Signed 16-bit little-endian
    S16Le,
    /// Signed 24-bit little-endian (packed)
    S24Le,
    /// Signed 32-bit little-endian
    S32Le,
    /// 32-bit floating point
    F32Le,
    /// Unsigned 8-bit
    U8,
}

impl SampleFormat {
    /// Get bytes per sample
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            SampleFormat::U8 => 1,
            SampleFormat::S16Le => 2,
            SampleFormat::S24Le => 3,
            SampleFormat::S32Le | SampleFormat::F32Le => 4,
        }
    }
}
```

### Stream Configuration

```rust
pub struct StreamConfig {
    /// Sample rate in Hz (e.g., 44100, 48000)
    pub sample_rate: u32,
    /// Number of channels (1=mono, 2=stereo)
    pub channels: u8,
    /// Sample format
    pub format: SampleFormat,
    /// Buffer size in frames
    pub buffer_frames: u32,
    /// Period size in frames
    pub period_frames: u32,
}

impl StreamConfig {
    /// Create standard CD quality config (44.1kHz stereo 16-bit)
    pub fn cd_quality() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            format: SampleFormat::S16Le,
            buffer_frames: 4096,
            period_frames: 1024,
        }
    }
    
    /// Create DVD quality config (48kHz stereo 16-bit)
    pub fn dvd_quality() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            format: SampleFormat::S16Le,
            buffer_frames: 4096,
            period_frames: 1024,
        }
    }
    
    /// Calculate bytes per frame
    pub fn bytes_per_frame(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }
    
    /// Calculate buffer size in bytes
    pub fn buffer_bytes(&self) -> usize {
        self.buffer_frames as usize * self.bytes_per_frame()
    }
}
```

---

## Intel HD Audio (HDA)

### Overview

Intel High Definition Audio is the standard audio interface on modern PCs.

```rust
// kernel/src/sound/hda.rs

pub struct HdaController {
    /// MMIO base address
    base: u64,
    /// Codec information
    codecs: Vec<HdaCodec>,
    /// Output stream (stereo PCM)
    output_stream: Option<HdaStream>,
    /// Input stream (capture)
    input_stream: Option<HdaStream>,
    /// Current volume
    volume: u8,
    /// Mute state
    muted: bool,
}
```

### HDA Registers

| Offset | Register | Description |
|--------|----------|-------------|
| 0x00 | GCAP | Global Capabilities |
| 0x08 | GCTL | Global Control |
| 0x0C | WAKEEN | Wake Enable |
| 0x0E | STATESTS | State Change Status |
| 0x20 | INTCTL | Interrupt Control |
| 0x24 | INTSTS | Interrupt Status |
| 0x30 | WALCLK | Wall Clock Counter |
| 0x40 | CORBLBASE | CORB Lower Base Address |
| 0x48 | CORBWP | CORB Write Pointer |
| 0x50 | RIRBLBASE | RIRB Lower Base Address |
| 0x58 | RIRBWP | RIRB Write Pointer |
| 0x80+ | SDn* | Stream Descriptor Registers |

### Stream Descriptor

```rust
pub struct HdaStream {
    /// Stream index
    index: u8,
    /// Stream descriptor register offset
    offset: u64,
    /// DMA buffer (ring buffer)
    buffer: Vec<u8>,
    /// Buffer Descriptor List
    bdl: Vec<BufferDescriptor>,
    /// Current position
    position: usize,
    /// Stream configuration
    config: StreamConfig,
    /// Running state
    running: bool,
}

#[repr(C)]
struct BufferDescriptor {
    /// Buffer address (physical)
    address: u64,
    /// Buffer length in bytes
    length: u32,
    /// Interrupt on completion flag
    ioc: u32,
}
```

### HDA Initialization

```rust
impl HdaController {
    pub fn new(pci_device: PciDevice) -> Result<Self, AudioError> {
        let base = pci_device.bar0() as u64;
        
        let mut controller = Self {
            base,
            codecs: Vec::new(),
            output_stream: None,
            input_stream: None,
            volume: 100,
            muted: false,
        };
        
        controller.reset()?;
        controller.enumerate_codecs()?;
        controller.configure_outputs()?;
        
        Ok(controller)
    }
    
    fn reset(&mut self) -> Result<(), AudioError> {
        // Enter reset
        self.write_reg(HDA_GCTL, 0);
        
        // Wait for reset
        for _ in 0..1000 {
            if self.read_reg(HDA_GCTL) & 1 == 0 {
                break;
            }
            // Small delay
        }
        
        // Exit reset
        self.write_reg(HDA_GCTL, 1);
        
        // Wait for codec ready
        for _ in 0..1000 {
            if self.read_reg(HDA_GCTL) & 1 == 1 {
                return Ok(());
            }
        }
        
        Err(AudioError::Timeout)
    }
}
```

### HDA Codec Communication

```rust
impl HdaController {
    /// Send verb to codec
    fn send_verb(&mut self, codec: u8, nid: u8, verb: u32) -> Result<u32, AudioError> {
        // Build command (codec_addr, nid, verb)
        let cmd = ((codec as u32) << 28) | ((nid as u32) << 20) | verb;
        
        // Write to CORB
        self.corb_write(cmd)?;
        
        // Read response from RIRB
        self.rirb_read()
    }
    
    /// Set widget volume
    fn set_amp(&mut self, codec: u8, nid: u8, gain: u8, output: bool, left: bool, right: bool) {
        let mut verb = 0x3 << 16;  // Set Amplifier Gain
        if output { verb |= 1 << 15; }
        if left   { verb |= 1 << 13; }
        if right  { verb |= 1 << 12; }
        verb |= gain as u32 & 0x7F;
        
        self.send_verb(codec, nid, verb).ok();
    }
}
```

---

## VirtIO Sound

### Overview

VirtIO Sound provides audio in virtual machines (QEMU, etc.).

```rust
// kernel/src/sound/virtio_snd.rs

pub struct VirtioSndDevice {
    /// VirtIO device
    virtio: VirtioDevice,
    /// Control virtqueue
    control_vq: VirtQueue,
    /// Event virtqueue
    event_vq: VirtQueue,
    /// TX virtqueue (playback)
    tx_vq: VirtQueue,
    /// RX virtqueue (capture)
    rx_vq: VirtQueue,
    /// Output streams
    output_streams: Vec<VirtioStream>,
    /// Input streams
    input_streams: Vec<VirtioStream>,
    /// Device config
    config: VirtioSndConfig,
}
```

### VirtIO Sound Config

```rust
#[repr(C)]
struct VirtioSndConfig {
    /// Number of jacks
    jacks: u32,
    /// Number of streams
    streams: u32,
    /// Number of channel maps
    chmaps: u32,
}
```

### Control Messages

```rust
#[repr(u32)]
enum VirtioSndControlCode {
    // Jack control
    JackInfo = 1,
    JackRemap = 2,
    
    // PCM control
    PcmInfo = 0x0100,
    PcmSetParams = 0x0101,
    PcmPrepare = 0x0102,
    PcmRelease = 0x0103,
    PcmStart = 0x0104,
    PcmStop = 0x0105,
    
    // Channel map
    ChmapInfo = 0x0200,
}

#[repr(C)]
struct VirtioSndPcmSetParams {
    hdr: VirtioSndPcmHdr,
    buffer_bytes: u32,
    period_bytes: u32,
    features: u32,
    channels: u8,
    format: u8,
    rate: u8,
    _padding: u8,
}
```

### VirtIO Stream Control

```rust
impl VirtioSndDevice {
    /// Start playback stream
    fn start_stream(&mut self, stream_id: u32) -> Result<(), AudioError> {
        let msg = VirtioSndPcmHdr {
            code: VirtioSndControlCode::PcmStart as u32,
            stream_id,
        };
        
        self.send_control(&msg)?;
        Ok(())
    }
    
    /// Write audio data
    fn write_pcm(&mut self, stream_id: u32, data: &[u8]) -> Result<usize, AudioError> {
        let tx_msg = VirtioSndPcmXfer {
            stream_id,
        };
        
        // Submit to TX virtqueue
        self.tx_vq.add_buffer(&[
            &tx_msg as &[u8],
            data,
        ])?;
        
        self.tx_vq.notify();
        
        // Wait for completion
        let written = self.tx_vq.wait_completion()?;
        Ok(written)
    }
}
```

---

## AC'97 (Legacy)

### Overview

AC'97 is a legacy audio standard, useful for older hardware and VMs.

```rust
// kernel/src/sound/ac97.rs

pub struct Ac97Controller {
    /// Mixer I/O base
    mixer_base: u16,
    /// Bus Master I/O base
    nabm_base: u16,
    /// Output stream
    output_stream: Ac97Stream,
    /// Volume level
    volume: u8,
    /// Mute state
    muted: bool,
}
```

### AC'97 Registers

| Register | Offset | Description |
|----------|--------|-------------|
| MASTER_VOL | 0x02 | Master Volume |
| PCM_OUT_VOL | 0x18 | PCM Output Volume |
| SAMPLE_RATE | 0x2C | PCM Sample Rate |
| EXTENDED_ID | 0x28 | Extended Audio ID |

### Buffer Descriptor List

```rust
#[repr(C, packed)]
struct BdlEntry {
    /// Physical address of buffer
    address: u32,
    /// Buffer length in samples (not bytes!)
    length: u16,
    /// Flags (IOC = Interrupt On Completion)
    flags: u16,
}

const MAX_BDL_ENTRIES: usize = 32;
```

### AC'97 Initialization

```rust
impl Ac97Controller {
    pub fn new(pci_device: PciDevice) -> Result<Self, AudioError> {
        let mixer_base = (pci_device.bar0() & 0xFFFE) as u16;
        let nabm_base = (pci_device.bar1() & 0xFFFE) as u16;
        
        let mut controller = Self {
            mixer_base,
            nabm_base,
            output_stream: Ac97Stream::new(),
            volume: 100,
            muted: false,
        };
        
        controller.reset()?;
        controller.configure()?;
        
        Ok(controller)
    }
    
    fn reset(&mut self) -> Result<(), AudioError> {
        // Cold reset
        self.write_nabm8(GLOB_CNT, 0x02);  // Cold reset
        
        // Wait for codec ready
        for _ in 0..1000 {
            if self.read_nabm8(GLOB_STA) & 0x01 != 0 {
                return Ok(());
            }
        }
        
        Err(AudioError::Timeout)
    }
    
    fn configure(&mut self) -> Result<(), AudioError> {
        // Enable VRA (Variable Rate Audio)
        let ext_id = self.read_mixer16(EXTENDED_ID);
        if ext_id & 0x01 != 0 {
            self.write_mixer16(EXTENDED_CTRL, 0x01);  // Enable VRA
        }
        
        // Set sample rate to 48kHz
        self.write_mixer16(SAMPLE_RATE, 48000);
        
        // Set volume (0 = max, 0x8000 = mute)
        self.write_mixer16(MASTER_VOL, 0x0000);
        self.write_mixer16(PCM_OUT_VOL, 0x0808);
        
        Ok(())
    }
}
```

---

## USB Audio

### Overview

USB Audio Class (UAC) support for USB headsets and DACs.

```rust
// kernel/src/sound/usb_audio.rs

pub struct UsbAudioDevice {
    /// USB device handle
    usb_device: UsbDevice,
    /// Audio control interface
    control_interface: u8,
    /// Streaming interfaces
    streaming_interfaces: Vec<AudioStreamInterface>,
    /// Current stream config
    stream_config: Option<StreamConfig>,
    /// Volume level
    volume: u8,
}
```

### Audio Class Descriptors

```rust
#[repr(u8)]
enum AudioDescriptorSubtype {
    Header = 0x01,
    InputTerminal = 0x02,
    OutputTerminal = 0x03,
    MixerUnit = 0x04,
    SelectorUnit = 0x05,
    FeatureUnit = 0x06,
    EffectUnit = 0x07,
    ProcessingUnit = 0x08,
    ExtensionUnit = 0x09,
}

struct AudioTerminal {
    terminal_id: u8,
    terminal_type: u16,  // USB Terminal Types
    channels: u8,
    channel_config: u32,
}
```

### USB Audio Control

```rust
impl UsbAudioDevice {
    /// Set feature unit control (e.g., volume)
    fn set_feature_control(
        &mut self,
        unit_id: u8,
        channel: u8,
        control: FeatureControl,
        value: i16,
    ) -> Result<(), AudioError> {
        let request = UsbControlRequest {
            request_type: 0x21,  // Class, Interface, Host-to-Device
            request: 0x01,       // SET_CUR
            value: (control as u16) << 8 | channel as u16,
            index: unit_id as u16,
            data: value.to_le_bytes().to_vec(),
        };
        
        self.usb_device.control_transfer(request)?;
        Ok(())
    }
    
    /// Set volume in dB (USB uses 1/256 dB units)
    fn set_volume_db(&mut self, db: f32) -> Result<(), AudioError> {
        let value = (db * 256.0) as i16;
        self.set_feature_control(
            self.feature_unit_id,
            0,  // Master channel
            FeatureControl::Volume,
            value,
        )
    }
}
```

---

## Audio Manager

### Global Audio Management

```rust
// kernel/src/sound/mod.rs

pub struct AudioManager {
    /// All registered audio devices
    devices: Vec<Arc<Mutex<dyn AudioDevice>>>,
    /// Default output device index
    default_output: Option<usize>,
    /// Default input device index
    default_input: Option<usize>,
}

impl AudioManager {
    /// Register audio device
    pub fn register_device(&mut self, device: impl AudioDevice + 'static) {
        self.devices.push(Arc::new(Mutex::new(device)));
        
        // Auto-select if first device
        if self.devices.len() == 1 {
            self.default_output = Some(0);
            self.default_input = Some(0);
        }
    }
    
    /// Get default output device
    pub fn get_output(&self) -> Option<Arc<Mutex<dyn AudioDevice>>> {
        self.default_output.map(|i| self.devices[i].clone())
    }
    
    /// Get default input device
    pub fn get_input(&self) -> Option<Arc<Mutex<dyn AudioDevice>>> {
        self.default_input.map(|i| self.devices[i].clone())
    }
    
    /// List all devices
    pub fn list_devices(&self) -> Vec<String> {
        self.devices.iter()
            .map(|d| d.lock().name().to_string())
            .collect()
    }
}
```

---

## Audio API

### Playback Example

```rust
// Play a simple tone
fn play_tone(frequency: f32, duration_ms: u32) -> Result<(), AudioError> {
    let audio = AUDIO_MANAGER.lock();
    let device = audio.get_output().ok_or(AudioError::NoDevice)?;
    let mut device = device.lock();
    
    // Configure stream
    let config = StreamConfig::cd_quality();
    device.configure_output(&config)?;
    device.start_playback()?;
    
    // Generate sine wave
    let samples_per_second = config.sample_rate as f32;
    let total_samples = (duration_ms as f32 / 1000.0 * samples_per_second) as usize;
    let mut buffer = Vec::with_capacity(total_samples * 2 * 2);  // stereo, 16-bit
    
    for i in 0..total_samples {
        let t = i as f32 / samples_per_second;
        let sample = (t * frequency * 2.0 * core::f32::consts::PI).sin();
        let value = (sample * 16000.0) as i16;
        
        // Left channel
        buffer.extend_from_slice(&value.to_le_bytes());
        // Right channel  
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    
    device.write(&buffer)?;
    device.stop_playback()?;
    
    Ok(())
}
```

### Capture Example

```rust
// Record audio
fn record_audio(duration_ms: u32) -> Result<Vec<u8>, AudioError> {
    let audio = AUDIO_MANAGER.lock();
    let device = audio.get_input().ok_or(AudioError::NoDevice)?;
    let mut device = device.lock();
    
    // Configure stream
    let config = StreamConfig::cd_quality();
    device.configure_input(&config)?;
    device.start_capture()?;
    
    // Calculate buffer size
    let bytes_needed = (duration_ms as usize * config.sample_rate as usize 
        * config.bytes_per_frame()) / 1000;
    
    let mut buffer = vec![0u8; bytes_needed];
    let mut total_read = 0;
    
    while total_read < bytes_needed {
        let read = device.read(&mut buffer[total_read..])?;
        total_read += read;
    }
    
    device.stop_capture()?;
    Ok(buffer)
}
```

---

## Shell Commands

### Audio Commands

| Command | Description | Example |
|---------|-------------|---------|
| `audio list` | List audio devices | `audio list` |
| `audio info` | Show current device info | `audio info` |
| `audio volume <0-100>` | Set volume | `audio volume 75` |
| `audio mute` | Mute audio | `audio mute` |
| `audio unmute` | Unmute audio | `audio unmute` |
| `audio test` | Play test tone | `audio test` |
| `audio beep [freq] [ms]` | Play beep | `audio beep 440 200` |

### Example Output

```
splax> audio list
Audio Devices:
  0: Intel HD Audio (default)
  1: VirtIO Sound

splax> audio info
Current Audio Device: Intel HD Audio
  Sample Rate: 48000 Hz
  Channels: 2 (stereo)
  Format: S16LE
  Volume: 75%
  Muted: no
```

---

## Error Handling

```rust
#[derive(Debug, Clone, Copy)]
pub enum AudioError {
    /// No audio device available
    NoDevice,
    /// Device not initialized
    NotInitialized,
    /// Invalid configuration
    InvalidConfig,
    /// Buffer underrun (playback starved)
    Underrun,
    /// Buffer overrun (capture overflow)
    Overrun,
    /// Device timeout
    Timeout,
    /// Device busy
    Busy,
    /// Hardware error
    HardwareError,
    /// Unsupported format
    UnsupportedFormat,
    /// Unsupported sample rate
    UnsupportedRate,
    /// I/O error
    IoError,
}
```

---

## File Structure

```
kernel/src/sound/
├── mod.rs          # AudioDevice trait, AudioManager
├── hda.rs          # Intel HD Audio driver
├── virtio_snd.rs   # VirtIO Sound driver
├── ac97.rs         # AC'97 driver
└── usb_audio.rs    # USB Audio Class driver
```

---

## Future Work

1. **Software Mixer**: Mix multiple audio streams
2. **Resampling**: Convert between sample rates
3. **Effects**: EQ, reverb, compression
4. **MIDI**: MIDI input/output support
5. **Bluetooth Audio**: A2DP/HFP support
6. **Spatial Audio**: 3D/surround sound
7. **Low Latency**: Real-time audio paths
8. **PulseAudio/JACK**: Userspace audio server
