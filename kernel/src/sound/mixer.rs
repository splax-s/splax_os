//! # Audio Mixing Layer
//!
//! This module implements software audio mixing for Splax OS, enabling
//! multiple audio streams to be combined and processed in real-time.
//!
//! ## Features
//!
//! - Multi-stream mixing with independent volume control
//! - Sample rate conversion
//! - Format conversion (int16, float32, etc.)
//! - Low-latency processing
//! - Per-stream and master effects
//! - Spatial audio (basic pan/balance)

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// =============================================================================
// Audio Format Types
// =============================================================================

/// Sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed 16-bit integer.
    S16,
    /// Signed 24-bit integer (packed).
    S24,
    /// Signed 32-bit integer.
    S32,
    /// 32-bit floating point.
    F32,
}

impl SampleFormat {
    /// Bytes per sample.
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            Self::S16 => 2,
            Self::S24 => 3,
            Self::S32 | Self::F32 => 4,
        }
    }
}

/// Audio format descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Sample format.
    pub format: SampleFormat,
}

impl AudioFormat {
    /// Create a new format.
    pub const fn new(sample_rate: u32, channels: u16, format: SampleFormat) -> Self {
        Self {
            sample_rate,
            channels,
            format,
        }
    }

    /// Standard CD quality format.
    pub const fn cd_quality() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            format: SampleFormat::S16,
        }
    }

    /// Standard 48kHz stereo format.
    pub const fn standard() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            format: SampleFormat::S16,
        }
    }

    /// High quality float format.
    pub const fn high_quality() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            format: SampleFormat::F32,
        }
    }

    /// Bytes per frame (all channels).
    pub fn bytes_per_frame(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }

    /// Calculate buffer size for given duration in milliseconds.
    pub fn buffer_size_for_ms(&self, ms: u32) -> usize {
        let frames = (self.sample_rate * ms) / 1000;
        frames as usize * self.bytes_per_frame()
    }
}

// =============================================================================
// Audio Stream
// =============================================================================

/// Stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Stopped,
    Playing,
    Paused,
    Finished,
}

/// Audio stream ID.
pub type StreamId = u32;

/// An audio stream to be mixed.
pub struct AudioStream {
    /// Unique stream ID.
    pub id: StreamId,
    /// Stream name (for debugging).
    pub name: String,
    /// Audio format.
    pub format: AudioFormat,
    /// Current state.
    state: StreamState,
    /// Volume (0.0 to 1.0).
    volume: f32,
    /// Pan (-1.0 left to 1.0 right).
    pan: f32,
    /// Muted flag.
    muted: bool,
    /// Audio buffer (samples in native format).
    buffer: Vec<u8>,
    /// Read position in buffer.
    read_pos: usize,
    /// Write position in buffer.
    write_pos: usize,
    /// Loop flag.
    looping: bool,
}

impl AudioStream {
    /// Create a new audio stream.
    pub fn new(id: StreamId, name: String, format: AudioFormat) -> Self {
        // Default buffer size: 100ms
        let buffer_size = format.buffer_size_for_ms(100);

        Self {
            id,
            name,
            format,
            state: StreamState::Stopped,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            buffer: vec![0; buffer_size],
            read_pos: 0,
            write_pos: 0,
            looping: false,
        }
    }

    /// Set volume (0.0 to 1.0).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Get current volume.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Set pan (-1.0 left to 1.0 right).
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan.clamp(-1.0, 1.0);
    }

    /// Get current pan.
    pub fn pan(&self) -> f32 {
        self.pan
    }

    /// Set muted state.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Check if muted.
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Set looping.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Check if looping.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// Play the stream.
    pub fn play(&mut self) {
        self.state = StreamState::Playing;
    }

    /// Pause the stream.
    pub fn pause(&mut self) {
        if self.state == StreamState::Playing {
            self.state = StreamState::Paused;
        }
    }

    /// Stop the stream.
    pub fn stop(&mut self) {
        self.state = StreamState::Stopped;
        self.read_pos = 0;
    }

    /// Get current state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// Write audio data to the stream buffer.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let available = self.buffer.len() - self.available_data();
        let to_write = data.len().min(available);

        for &byte in data.iter().take(to_write) {
            self.buffer[self.write_pos] = byte;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
        }

        to_write
    }

    /// Read audio data from the stream buffer.
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        if self.state != StreamState::Playing || self.muted {
            // Fill with silence
            buffer.fill(0);
            return buffer.len();
        }

        let available = self.available_data();
        let to_read = buffer.len().min(available);

        for byte in buffer.iter_mut().take(to_read) {
            *byte = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
        }

        // Fill remaining with silence
        if to_read < buffer.len() {
            buffer[to_read..].fill(0);
            if !self.looping && to_read == 0 {
                self.state = StreamState::Finished;
            }
        }

        to_read
    }

    /// Get available data in buffer.
    fn available_data(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.buffer.len() - self.read_pos + self.write_pos
        }
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.buffer.fill(0);
        self.read_pos = 0;
        self.write_pos = 0;
    }
}

// =============================================================================
// Sample Conversion
// =============================================================================

/// Convert S16 sample to F32.
#[inline]
pub fn s16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

/// Convert F32 sample to S16 with clipping.
#[inline]
pub fn f32_to_s16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * 32767.0) as i16
}

/// Convert S32 sample to F32.
#[inline]
pub fn s32_to_f32(sample: i32) -> f32 {
    sample as f32 / 2147483648.0
}

/// Convert F32 sample to S32 with clipping.
#[inline]
pub fn f32_to_s32(sample: f32) -> i32 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * 2147483647.0) as i32
}

/// Read S16 samples from byte buffer.
pub fn read_s16_samples(data: &[u8]) -> Vec<f32> {
    let mut samples = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        samples.push(s16_to_f32(sample));
    }
    samples
}

/// Write F32 samples as S16 to byte buffer.
pub fn write_s16_samples(samples: &[f32], buffer: &mut [u8]) {
    for (i, &sample) in samples.iter().enumerate() {
        let s16 = f32_to_s16(sample);
        let bytes = s16.to_le_bytes();
        let pos = i * 2;
        if pos + 1 < buffer.len() {
            buffer[pos] = bytes[0];
            buffer[pos + 1] = bytes[1];
        }
    }
}

// =============================================================================
// Sample Rate Conversion
// =============================================================================

/// Simple linear interpolation resampler.
pub struct LinearResampler {
    /// Source sample rate.
    src_rate: u32,
    /// Destination sample rate.
    dst_rate: u32,
    /// Fractional position.
    frac_pos: f64,
    /// Last sample (for interpolation).
    last_sample: f32,
}

impl LinearResampler {
    /// Create a new resampler.
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate,
            dst_rate,
            frac_pos: 0.0,
            last_sample: 0.0,
        }
    }

    /// Resample a buffer of samples.
    pub fn resample(&mut self, input: &[f32]) -> Vec<f32> {
        if self.src_rate == self.dst_rate {
            return input.to_vec();
        }

        let ratio = self.src_rate as f64 / self.dst_rate as f64;
        let output_len = ((input.len() as f64 / ratio) + 1.0) as usize;
        let mut output = Vec::with_capacity(output_len);

        let mut src_pos = self.frac_pos;

        while (src_pos as usize) < input.len() {
            let idx = src_pos as usize;
            let frac = src_pos - idx as f64;

            // Linear interpolation
            let sample1 = if idx > 0 { input[idx - 1] } else { self.last_sample };
            let sample2 = input[idx];
            let interpolated = sample1 + (sample2 - sample1) * frac as f32;

            output.push(interpolated);
            src_pos += ratio;
        }

        // Save state for next call
        self.frac_pos = src_pos - input.len() as f64;
        if !input.is_empty() {
            self.last_sample = input[input.len() - 1];
        }

        output
    }

    /// Reset the resampler state.
    pub fn reset(&mut self) {
        self.frac_pos = 0.0;
        self.last_sample = 0.0;
    }
}

// =============================================================================
// Audio Effects
// =============================================================================

/// Audio effect trait.
pub trait AudioEffect: Send {
    /// Process audio samples in place.
    fn process(&mut self, samples: &mut [f32]);

    /// Reset effect state.
    fn reset(&mut self);
}

/// Simple gain effect.
pub struct GainEffect {
    gain: f32,
}

impl GainEffect {
    pub fn new(gain_db: f32) -> Self {
        Self {
            gain: 10.0_f32.powf(gain_db / 20.0),
        }
    }

    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain = 10.0_f32.powf(gain_db / 20.0);
    }
}

impl AudioEffect for GainEffect {
    fn process(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            *sample *= self.gain;
        }
    }

    fn reset(&mut self) {}
}

/// Simple low-pass filter.
pub struct LowPassFilter {
    cutoff: f32,
    sample_rate: f32,
    alpha: f32,
    prev_output: f32,
}

impl LowPassFilter {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * core::f32::consts::PI * cutoff);
        let dt = 1.0 / sample_rate;
        let alpha = dt / (rc + dt);

        Self {
            cutoff,
            sample_rate,
            alpha,
            prev_output: 0.0,
        }
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff;
        let rc = 1.0 / (2.0 * core::f32::consts::PI * cutoff);
        let dt = 1.0 / self.sample_rate;
        self.alpha = dt / (rc + dt);
    }
}

impl AudioEffect for LowPassFilter {
    fn process(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            self.prev_output = self.prev_output + self.alpha * (*sample - self.prev_output);
            *sample = self.prev_output;
        }
    }

    fn reset(&mut self) {
        self.prev_output = 0.0;
    }
}

/// Simple high-pass filter.
pub struct HighPassFilter {
    cutoff: f32,
    sample_rate: f32,
    alpha: f32,
    prev_input: f32,
    prev_output: f32,
}

impl HighPassFilter {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * core::f32::consts::PI * cutoff);
        let dt = 1.0 / sample_rate;
        let alpha = rc / (rc + dt);

        Self {
            cutoff,
            sample_rate,
            alpha,
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }
}

impl AudioEffect for HighPassFilter {
    fn process(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            let input = *sample;
            self.prev_output = self.alpha * (self.prev_output + input - self.prev_input);
            self.prev_input = input;
            *sample = self.prev_output;
        }
    }

    fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}

/// Soft clipper / limiter.
pub struct SoftClipper {
    threshold: f32,
}

impl SoftClipper {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold: threshold.clamp(0.1, 1.0),
        }
    }
}

impl AudioEffect for SoftClipper {
    fn process(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            let x = *sample;
            if x.abs() < self.threshold {
                // Below threshold: pass through
            } else {
                // Soft clip using tanh-like curve
                let sign = x.signum();
                let excess = (x.abs() - self.threshold) / (1.0 - self.threshold);
                let clipped = self.threshold + (1.0 - self.threshold) * (1.0 - 1.0 / (1.0 + excess));
                *sample = sign * clipped;
            }
        }
    }

    fn reset(&mut self) {}
}

// =============================================================================
// Audio Mixer
// =============================================================================

/// Mixer configuration.
#[derive(Debug, Clone)]
pub struct MixerConfig {
    /// Output format.
    pub output_format: AudioFormat,
    /// Buffer size in frames.
    pub buffer_frames: usize,
    /// Maximum number of streams.
    pub max_streams: usize,
}

impl Default for MixerConfig {
    fn default() -> Self {
        Self {
            output_format: AudioFormat::standard(),
            buffer_frames: 1024,
            max_streams: 32,
        }
    }
}

/// The audio mixer.
pub struct AudioMixer {
    /// Mixer configuration.
    config: MixerConfig,
    /// Active streams.
    streams: Vec<AudioStream>,
    /// Next stream ID.
    next_stream_id: AtomicU32,
    /// Master volume (0.0 to 1.0).
    master_volume: f32,
    /// Master mute flag.
    master_muted: AtomicBool,
    /// Mixing buffer (F32 format).
    mix_buffer: Vec<f32>,
    /// Output buffer.
    output_buffer: Vec<u8>,
    /// Master effects chain.
    effects: Vec<Box<dyn AudioEffect>>,
}

impl AudioMixer {
    /// Create a new mixer with default configuration.
    pub fn new() -> Self {
        Self::with_config(MixerConfig::default())
    }

    /// Create a new mixer with custom configuration.
    pub fn with_config(config: MixerConfig) -> Self {
        let channels = config.output_format.channels as usize;
        let mix_buffer_size = config.buffer_frames * channels;
        let output_buffer_size = config.buffer_frames * config.output_format.bytes_per_frame();

        Self {
            config,
            streams: Vec::new(),
            next_stream_id: AtomicU32::new(1),
            master_volume: 1.0,
            master_muted: AtomicBool::new(false),
            mix_buffer: vec![0.0; mix_buffer_size],
            output_buffer: vec![0; output_buffer_size],
            effects: Vec::new(),
        }
    }

    /// Get output format.
    pub fn output_format(&self) -> AudioFormat {
        self.config.output_format
    }

    /// Set master volume.
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    /// Get master volume.
    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    /// Set master mute.
    pub fn set_master_muted(&self, muted: bool) {
        self.master_muted.store(muted, Ordering::Relaxed);
    }

    /// Check if master is muted.
    pub fn is_master_muted(&self) -> bool {
        self.master_muted.load(Ordering::Relaxed)
    }

    /// Add a master effect.
    pub fn add_effect(&mut self, effect: Box<dyn AudioEffect>) {
        self.effects.push(effect);
    }

    /// Create a new stream.
    pub fn create_stream(&mut self, name: String, format: AudioFormat) -> Option<StreamId> {
        if self.streams.len() >= self.config.max_streams {
            return None;
        }

        let id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let stream = AudioStream::new(id, name, format);
        self.streams.push(stream);
        Some(id)
    }

    /// Get stream by ID.
    pub fn get_stream(&self, id: StreamId) -> Option<&AudioStream> {
        self.streams.iter().find(|s| s.id == id)
    }

    /// Get mutable stream by ID.
    pub fn get_stream_mut(&mut self, id: StreamId) -> Option<&mut AudioStream> {
        self.streams.iter_mut().find(|s| s.id == id)
    }

    /// Remove a stream.
    pub fn remove_stream(&mut self, id: StreamId) -> bool {
        if let Some(pos) = self.streams.iter().position(|s| s.id == id) {
            self.streams.remove(pos);
            true
        } else {
            false
        }
    }

    /// Mix all streams and produce output.
    pub fn mix(&mut self, output: &mut [u8]) -> usize {
        let channels = self.config.output_format.channels as usize;
        let frames = output.len() / self.config.output_format.bytes_per_frame();

        // Clear mix buffer
        for sample in self.mix_buffer.iter_mut().take(frames * channels) {
            *sample = 0.0;
        }

        // Mix each stream
        for stream in self.streams.iter_mut() {
            if stream.state() != StreamState::Playing || stream.is_muted() {
                continue;
            }

            // Read samples from stream
            let bytes_per_frame = stream.format.bytes_per_frame();
            let stream_bytes = frames * bytes_per_frame;
            let mut stream_buffer = vec![0u8; stream_bytes];
            stream.read(&mut stream_buffer);

            // Convert to F32
            let stream_samples = read_s16_samples(&stream_buffer);

            // Apply volume and pan
            let volume = stream.volume();
            let pan = stream.pan();
            let left_gain = volume * (1.0 - pan.max(0.0));
            let right_gain = volume * (1.0 + pan.min(0.0));

            // Mix into output buffer
            for (i, chunk) in stream_samples.chunks(2).enumerate() {
                if i * 2 + 1 >= self.mix_buffer.len() {
                    break;
                }
                let left = if chunk.len() > 0 { chunk[0] } else { 0.0 };
                let right = if chunk.len() > 1 { chunk[1] } else { left };

                self.mix_buffer[i * 2] += left * left_gain;
                self.mix_buffer[i * 2 + 1] += right * right_gain;
            }
        }

        // Apply master effects
        for effect in self.effects.iter_mut() {
            effect.process(&mut self.mix_buffer[..frames * channels]);
        }

        // Apply master volume
        let master_vol = if self.is_master_muted() { 0.0 } else { self.master_volume };
        for sample in self.mix_buffer.iter_mut().take(frames * channels) {
            *sample *= master_vol;
        }

        // Convert to output format and write
        write_s16_samples(&self.mix_buffer[..frames * channels], output);

        frames * self.config.output_format.bytes_per_frame()
    }

    /// Remove finished streams.
    pub fn cleanup_finished(&mut self) {
        self.streams.retain(|s| s.state() != StreamState::Finished);
    }

    /// Get number of active streams.
    pub fn active_stream_count(&self) -> usize {
        self.streams.iter().filter(|s| s.state() == StreamState::Playing).count()
    }

    /// Get total stream count.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Audio Device Interface
// =============================================================================

/// Audio device trait.
pub trait AudioDevice {
    /// Start audio playback.
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stop audio playback.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Get the native format.
    fn format(&self) -> AudioFormat;

    /// Write audio data to the device.
    fn write(&mut self, data: &[u8]) -> Result<usize, AudioError>;

    /// Get available buffer space.
    fn available(&self) -> usize;
}

/// Audio errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    /// Device not found.
    DeviceNotFound,
    /// Device busy.
    DeviceBusy,
    /// Invalid format.
    InvalidFormat,
    /// Buffer underrun.
    Underrun,
    /// Buffer overrun.
    Overrun,
    /// I/O error.
    IoError,
    /// Not supported.
    NotSupported,
}

// =============================================================================
// Audio Engine
// =============================================================================

/// High-level audio engine.
pub struct AudioEngine {
    /// The mixer.
    mixer: AudioMixer,
    /// Output buffer.
    output_buffer: Vec<u8>,
    /// Running flag.
    running: AtomicBool,
}

impl AudioEngine {
    /// Create a new audio engine.
    pub fn new(config: MixerConfig) -> Self {
        let buffer_size = config.buffer_frames * config.output_format.bytes_per_frame();

        Self {
            mixer: AudioMixer::with_config(config),
            output_buffer: vec![0; buffer_size],
            running: AtomicBool::new(false),
        }
    }

    /// Start the audio engine.
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Stop the audio engine.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get the mixer.
    pub fn mixer(&self) -> &AudioMixer {
        &self.mixer
    }

    /// Get mutable mixer.
    pub fn mixer_mut(&mut self) -> &mut AudioMixer {
        &mut self.mixer
    }

    /// Process audio (called by audio callback).
    pub fn process(&mut self, output: &mut [u8]) -> usize {
        if !self.is_running() {
            output.fill(0);
            return output.len();
        }

        self.mixer.mix(output)
    }

    /// Create a new stream.
    pub fn create_stream(&mut self, name: &str, format: AudioFormat) -> Option<StreamId> {
        self.mixer.create_stream(String::from(name), format)
    }

    /// Play sound data (convenience method).
    pub fn play_sound(&mut self, name: &str, data: &[u8], format: AudioFormat) -> Option<StreamId> {
        let id = self.create_stream(name, format)?;
        if let Some(stream) = self.mixer.get_stream_mut(id) {
            stream.write(data);
            stream.play();
        }
        Some(id)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_format() {
        let format = AudioFormat::cd_quality();
        assert_eq!(format.sample_rate, 44100);
        assert_eq!(format.channels, 2);
        assert_eq!(format.bytes_per_frame(), 4);
    }

    #[test]
    fn test_sample_conversion() {
        assert!((s16_to_f32(0) - 0.0).abs() < 0.001);
        assert!((s16_to_f32(16384) - 0.5).abs() < 0.01);
        assert!((s16_to_f32(-16384) + 0.5).abs() < 0.01);

        assert_eq!(f32_to_s16(0.0), 0);
        assert!(f32_to_s16(1.0) > 32000);
        assert!(f32_to_s16(-1.0) < -32000);
    }

    #[test]
    fn test_stream_volume() {
        let mut stream = AudioStream::new(1, String::from("test"), AudioFormat::cd_quality());

        stream.set_volume(0.5);
        assert!((stream.volume() - 0.5).abs() < 0.001);

        stream.set_volume(2.0); // Should clamp
        assert!((stream.volume() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_stream_state() {
        let mut stream = AudioStream::new(1, String::from("test"), AudioFormat::cd_quality());

        assert_eq!(stream.state(), StreamState::Stopped);
        stream.play();
        assert_eq!(stream.state(), StreamState::Playing);
        stream.pause();
        assert_eq!(stream.state(), StreamState::Paused);
        stream.stop();
        assert_eq!(stream.state(), StreamState::Stopped);
    }

    #[test]
    fn test_mixer_creation() {
        let mixer = AudioMixer::new();
        assert_eq!(mixer.stream_count(), 0);
        assert!((mixer.master_volume() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mixer_stream_management() {
        let mut mixer = AudioMixer::new();

        let id = mixer.create_stream(String::from("test"), AudioFormat::cd_quality()).unwrap();
        assert_eq!(mixer.stream_count(), 1);

        mixer.remove_stream(id);
        assert_eq!(mixer.stream_count(), 0);
    }

    #[test]
    fn test_resampler() {
        let mut resampler = LinearResampler::new(44100, 48000);
        let input = vec![0.0, 1.0, 0.0, -1.0, 0.0];
        let output = resampler.resample(&input);

        // Output should be longer due to upsampling
        assert!(output.len() >= input.len());
    }

    #[test]
    fn test_gain_effect() {
        let mut gain = GainEffect::new(6.0); // +6 dB
        let mut samples = vec![0.5, -0.5];

        gain.process(&mut samples);

        // +6 dB ~ 2x gain
        assert!(samples[0] > 0.9);
        assert!(samples[1] < -0.9);
    }

    #[test]
    fn test_soft_clipper() {
        let mut clipper = SoftClipper::new(0.8);
        let mut samples = vec![0.5, 1.5, -1.5];

        clipper.process(&mut samples);

        assert!((samples[0] - 0.5).abs() < 0.01); // Below threshold, unchanged
        assert!(samples[1] < 1.0); // Clipped
        assert!(samples[2] > -1.0); // Clipped
    }
}
