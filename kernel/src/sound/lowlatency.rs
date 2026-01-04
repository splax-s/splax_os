//! # Low-Latency Audio Engine
//!
//! High-performance, low-latency audio processing for professional audio
//! applications and real-time audio synthesis.
//!
//! ## Features
//!
//! - Lock-free audio buffers
//! - Priority-based audio scheduling
//! - Sub-millisecond latency support
//! - Real-time safe memory allocation
//! - Audio graph processing
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                  Low-Latency Audio Engine                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │   Audio Graph   │   RT Scheduler   │   Lock-Free Buffers       │
//! ├─────────────────┼──────────────────┼────────────────────────────┤
//! │   Processing    │   Priority Mgmt  │   Ring Buffers             │
//! │   Nodes         │   CPU Affinity   │   Zero-Copy Transfer       │
//! └─────────────────┴──────────────────┴────────────────────────────┘
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// =============================================================================
// Lock-Free Ring Buffer
// =============================================================================

/// Cache line size for padding.
const CACHE_LINE_SIZE: usize = 64;

/// Lock-free single-producer single-consumer ring buffer.
/// Optimized for real-time audio with no locks or allocations.
#[repr(C)]
pub struct LockFreeRingBuffer<T: Copy + Default> {
    /// Buffer data
    buffer: Box<[T]>,
    /// Buffer capacity (power of 2)
    capacity: usize,
    /// Write position (producer)
    write_pos: AtomicUsize,
    /// Read position (consumer)
    read_pos: AtomicUsize,
    /// Padding to prevent false sharing
    _pad: [u8; CACHE_LINE_SIZE],
}

impl<T: Copy + Default> LockFreeRingBuffer<T> {
    /// Create a new ring buffer with the given capacity (rounded up to power of 2).
    pub fn new(min_capacity: usize) -> Self {
        let capacity = min_capacity.next_power_of_two();
        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize_with(capacity, T::default);
        
        Self {
            buffer: buffer.into_boxed_slice(),
            capacity,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
            _pad: [0; CACHE_LINE_SIZE],
        }
    }

    /// Get available space for writing.
    #[inline]
    pub fn write_available(&self) -> usize {
        let write = self.write_pos.load(Ordering::Relaxed);
        let read = self.read_pos.load(Ordering::Acquire);
        self.capacity - (write.wrapping_sub(read))
    }

    /// Get available data for reading.
    #[inline]
    pub fn read_available(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Relaxed);
        write.wrapping_sub(read)
    }

    /// Write samples to the buffer.
    /// Returns number of samples actually written.
    pub fn write(&self, data: &[T]) -> usize {
        let available = self.write_available();
        let to_write = data.len().min(available);
        
        if to_write == 0 {
            return 0;
        }
        
        let write_pos = self.write_pos.load(Ordering::Relaxed);
        let mask = self.capacity - 1;
        
        for i in 0..to_write {
            let idx = (write_pos + i) & mask;
            // Safety: we're the only writer and idx is within bounds
            unsafe {
                let ptr = self.buffer.as_ptr().add(idx) as *mut T;
                core::ptr::write_volatile(ptr, data[i]);
            }
        }
        
        // Memory barrier before updating write position
        core::sync::atomic::fence(Ordering::Release);
        self.write_pos.store(write_pos.wrapping_add(to_write), Ordering::Release);
        
        to_write
    }

    /// Read samples from the buffer.
    /// Returns number of samples actually read.
    pub fn read(&self, data: &mut [T]) -> usize {
        let available = self.read_available();
        let to_read = data.len().min(available);
        
        if to_read == 0 {
            return 0;
        }
        
        let read_pos = self.read_pos.load(Ordering::Relaxed);
        let mask = self.capacity - 1;
        
        for i in 0..to_read {
            let idx = (read_pos + i) & mask;
            data[i] = unsafe { core::ptr::read_volatile(self.buffer.as_ptr().add(idx)) };
        }
        
        // Memory barrier before updating read position
        core::sync::atomic::fence(Ordering::Release);
        self.read_pos.store(read_pos.wrapping_add(to_read), Ordering::Release);
        
        to_read
    }

    /// Peek at data without consuming it.
    pub fn peek(&self, data: &mut [T]) -> usize {
        let available = self.read_available();
        let to_peek = data.len().min(available);
        
        let read_pos = self.read_pos.load(Ordering::Relaxed);
        let mask = self.capacity - 1;
        
        for i in 0..to_peek {
            let idx = (read_pos + i) & mask;
            data[i] = unsafe { core::ptr::read_volatile(self.buffer.as_ptr().add(idx)) };
        }
        
        to_peek
    }

    /// Reset the buffer.
    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::SeqCst);
        self.read_pos.store(0, Ordering::SeqCst);
    }
}

// =============================================================================
// Audio Processing Node
// =============================================================================

/// Audio sample type.
pub type Sample = f32;

/// Audio buffer for processing.
pub struct AudioBuffer {
    /// Interleaved sample data
    data: Vec<Sample>,
    /// Number of channels
    channels: u32,
    /// Number of frames
    frames: u32,
}

impl AudioBuffer {
    /// Create a new audio buffer.
    pub fn new(channels: u32, frames: u32) -> Self {
        let size = (channels * frames) as usize;
        Self {
            data: alloc::vec![0.0; size],
            channels,
            frames,
        }
    }

    /// Get a sample at the given frame and channel.
    #[inline]
    pub fn get(&self, frame: u32, channel: u32) -> Sample {
        let idx = (frame * self.channels + channel) as usize;
        self.data.get(idx).copied().unwrap_or(0.0)
    }

    /// Set a sample at the given frame and channel.
    #[inline]
    pub fn set(&mut self, frame: u32, channel: u32, value: Sample) {
        let idx = (frame * self.channels + channel) as usize;
        if let Some(sample) = self.data.get_mut(idx) {
            *sample = value;
        }
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Get raw data.
    pub fn data(&self) -> &[Sample] {
        &self.data
    }

    /// Get mutable raw data.
    pub fn data_mut(&mut self) -> &mut [Sample] {
        &mut self.data
    }

    /// Number of channels.
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Number of frames.
    pub fn frames(&self) -> u32 {
        self.frames
    }
}

/// Node ID.
pub type NodeId = u32;

/// Audio processing node trait.
pub trait AudioNode: Send {
    /// Process audio data.
    fn process(&mut self, inputs: &[&AudioBuffer], output: &mut AudioBuffer);
    
    /// Get the number of input ports.
    fn num_inputs(&self) -> u32;
    
    /// Get the number of output ports.
    fn num_outputs(&self) -> u32;
    
    /// Reset the node state.
    fn reset(&mut self);
    
    /// Get latency in samples.
    fn latency(&self) -> u32 {
        0
    }
}

// =============================================================================
// Built-in Audio Nodes
// =============================================================================

/// Gain node - adjusts volume.
pub struct GainNode {
    gain: f32,
}

impl GainNode {
    pub fn new(gain: f32) -> Self {
        Self { gain }
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }
}

impl AudioNode for GainNode {
    fn process(&mut self, inputs: &[&AudioBuffer], output: &mut AudioBuffer) {
        if inputs.is_empty() {
            output.clear();
            return;
        }
        
        let input = inputs[0];
        for frame in 0..output.frames() {
            for channel in 0..output.channels() {
                let sample = input.get(frame, channel) * self.gain;
                output.set(frame, channel, sample);
            }
        }
    }

    fn num_inputs(&self) -> u32 { 1 }
    fn num_outputs(&self) -> u32 { 1 }
    fn reset(&mut self) {}
}

/// Mixer node - mixes multiple inputs.
pub struct MixerNode {
    num_inputs: u32,
}

impl MixerNode {
    pub fn new(num_inputs: u32) -> Self {
        Self { num_inputs }
    }
}

impl AudioNode for MixerNode {
    fn process(&mut self, inputs: &[&AudioBuffer], output: &mut AudioBuffer) {
        output.clear();
        
        for input in inputs {
            for frame in 0..output.frames() {
                for channel in 0..output.channels() {
                    let current = output.get(frame, channel);
                    let sample = input.get(frame, channel);
                    output.set(frame, channel, current + sample);
                }
            }
        }
    }

    fn num_inputs(&self) -> u32 { self.num_inputs }
    fn num_outputs(&self) -> u32 { 1 }
    fn reset(&mut self) {}
}

/// Simple oscillator node.
pub struct OscillatorNode {
    frequency: f32,
    phase: f32,
    sample_rate: f32,
    waveform: Waveform,
}

/// Oscillator waveform type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl OscillatorNode {
    pub fn new(frequency: f32, sample_rate: f32, waveform: Waveform) -> Self {
        Self {
            frequency,
            phase: 0.0,
            sample_rate,
            waveform,
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        self.frequency = frequency;
    }
}

impl AudioNode for OscillatorNode {
    fn process(&mut self, _inputs: &[&AudioBuffer], output: &mut AudioBuffer) {
        let phase_inc = self.frequency / self.sample_rate;
        
        for frame in 0..output.frames() {
            let sample = match self.waveform {
                Waveform::Sine => {
                    #[allow(unused)]
                    const PI: f32 = 3.14159265359;
                    // Approximate sine using polynomial
                    let x = self.phase * 2.0 - 1.0;
                    let x2 = x * x;
                    x * (1.0 - x2 * (0.16666667 - x2 * 0.00833333))
                }
                Waveform::Square => {
                    if self.phase < 0.5 { 1.0 } else { -1.0 }
                }
                Waveform::Sawtooth => {
                    2.0 * self.phase - 1.0
                }
                Waveform::Triangle => {
                    if self.phase < 0.5 {
                        4.0 * self.phase - 1.0
                    } else {
                        3.0 - 4.0 * self.phase
                    }
                }
            };
            
            for channel in 0..output.channels() {
                output.set(frame, channel, sample);
            }
            
            self.phase += phase_inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }

    fn num_inputs(&self) -> u32 { 0 }
    fn num_outputs(&self) -> u32 { 1 }
    
    fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// Low-pass filter node.
pub struct LowPassFilterNode {
    cutoff: f32,
    sample_rate: f32,
    state: [f32; 2],
}

impl LowPassFilterNode {
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        Self {
            cutoff,
            sample_rate,
            state: [0.0; 2],
        }
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff;
    }
}

impl AudioNode for LowPassFilterNode {
    fn process(&mut self, inputs: &[&AudioBuffer], output: &mut AudioBuffer) {
        if inputs.is_empty() {
            output.clear();
            return;
        }
        
        let input = inputs[0];
        let alpha = self.cutoff / (self.cutoff + self.sample_rate);
        
        for frame in 0..output.frames() {
            for channel in 0..output.channels().min(2) {
                let x = input.get(frame, channel);
                self.state[channel as usize] = self.state[channel as usize] + alpha * (x - self.state[channel as usize]);
                output.set(frame, channel, self.state[channel as usize]);
            }
        }
    }

    fn num_inputs(&self) -> u32 { 1 }
    fn num_outputs(&self) -> u32 { 1 }
    
    fn reset(&mut self) {
        self.state = [0.0; 2];
    }
    
    fn latency(&self) -> u32 { 1 }
}

// =============================================================================
// Audio Graph
// =============================================================================

/// Connection between nodes.
#[derive(Clone)]
struct Connection {
    source_node: NodeId,
    source_port: u32,
    dest_node: NodeId,
    dest_port: u32,
}

/// Audio processing graph.
pub struct AudioGraph {
    /// Nodes in the graph
    nodes: BTreeMap<NodeId, Box<dyn AudioNode>>,
    /// Connections between nodes
    connections: Vec<Connection>,
    /// Processing order (topologically sorted)
    processing_order: Vec<NodeId>,
    /// Output node
    output_node: Option<NodeId>,
    /// Next node ID
    next_id: AtomicU32,
    /// Sample rate
    sample_rate: u32,
    /// Buffer size
    buffer_size: u32,
    /// Temporary buffers for processing
    temp_buffers: BTreeMap<NodeId, AudioBuffer>,
}

impl AudioGraph {
    /// Create a new audio graph.
    pub fn new(sample_rate: u32, buffer_size: u32) -> Self {
        Self {
            nodes: BTreeMap::new(),
            connections: Vec::new(),
            processing_order: Vec::new(),
            output_node: None,
            next_id: AtomicU32::new(1),
            sample_rate,
            buffer_size,
            temp_buffers: BTreeMap::new(),
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: Box<dyn AudioNode>) -> NodeId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        
        // Create temp buffer for this node
        let buffer = AudioBuffer::new(2, self.buffer_size);
        self.temp_buffers.insert(id, buffer);
        
        self.nodes.insert(id, node);
        id
    }

    /// Remove a node from the graph.
    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.remove(&id);
        self.temp_buffers.remove(&id);
        self.connections.retain(|c| c.source_node != id && c.dest_node != id);
        self.update_processing_order();
    }

    /// Connect two nodes.
    pub fn connect(&mut self, source: NodeId, source_port: u32, dest: NodeId, dest_port: u32) -> Result<(), AudioError> {
        if !self.nodes.contains_key(&source) || !self.nodes.contains_key(&dest) {
            return Err(AudioError::InvalidNode);
        }
        
        self.connections.push(Connection {
            source_node: source,
            source_port,
            dest_node: dest,
            dest_port,
        });
        
        self.update_processing_order();
        Ok(())
    }

    /// Disconnect nodes.
    pub fn disconnect(&mut self, source: NodeId, dest: NodeId) {
        self.connections.retain(|c| !(c.source_node == source && c.dest_node == dest));
        self.update_processing_order();
    }

    /// Set the output node.
    pub fn set_output(&mut self, node: NodeId) {
        self.output_node = Some(node);
        self.update_processing_order();
    }

    /// Update processing order using topological sort.
    fn update_processing_order(&mut self) {
        self.processing_order.clear();
        
        let mut visited = BTreeMap::new();
        let mut order = Vec::new();
        
        // DFS to find processing order
        fn visit(
            node: NodeId,
            connections: &[Connection],
            visited: &mut BTreeMap<NodeId, bool>,
            order: &mut Vec<NodeId>,
        ) {
            if visited.get(&node).copied().unwrap_or(false) {
                return;
            }
            
            visited.insert(node, true);
            
            // Visit all sources first
            for conn in connections {
                if conn.dest_node == node {
                    visit(conn.source_node, connections, visited, order);
                }
            }
            
            order.push(node);
        }
        
        // Visit all nodes reachable from output
        if let Some(output) = self.output_node {
            visit(output, &self.connections, &mut visited, &mut order);
        }
        
        self.processing_order = order;
    }

    /// Process the audio graph.
    pub fn process(&mut self, output: &mut AudioBuffer) {
        // Process nodes in order
        for &node_id in &self.processing_order.clone() {
            // Collect inputs
            let input_ids: Vec<NodeId> = self.connections.iter()
                .filter(|c| c.dest_node == node_id)
                .map(|c| c.source_node)
                .collect();
            
            // Get input buffers (using temporary ownership swap)
            let inputs: Vec<&AudioBuffer> = input_ids.iter()
                .filter_map(|id| self.temp_buffers.get(id))
                .collect();
            
            // Process node
            if let Some(node) = self.nodes.get_mut(&node_id) {
                if let Some(buffer) = self.temp_buffers.get_mut(&node_id) {
                    node.process(&inputs, buffer);
                }
            }
        }
        
        // Copy output node buffer to output
        if let Some(output_id) = self.output_node {
            if let Some(buffer) = self.temp_buffers.get(&output_id) {
                let len = output.data_mut().len().min(buffer.data().len());
                output.data_mut()[..len].copy_from_slice(&buffer.data()[..len]);
            }
        }
    }

    /// Get sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get buffer size.
    pub fn buffer_size(&self) -> u32 {
        self.buffer_size
    }
}

// =============================================================================
// Real-Time Audio Scheduler
// =============================================================================

/// Audio thread priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    /// Normal priority - for non-critical audio
    Normal = 0,
    /// High priority - for low-latency audio
    High = 1,
    /// Real-time priority - for professional audio
    RealTime = 2,
}

/// Audio callback function type.
pub type AudioCallback = fn(&mut AudioBuffer, &mut AudioBuffer);

/// Audio stream handle.
pub struct AudioStream {
    /// Stream ID
    id: u32,
    /// Sample rate
    sample_rate: u32,
    /// Buffer size in frames
    buffer_size: u32,
    /// Channels
    channels: u32,
    /// Priority
    priority: AudioPriority,
    /// Running flag
    running: AtomicBool,
    /// Underrun count
    underruns: AtomicU32,
    /// Overrun count
    overruns: AtomicU32,
    /// Total frames processed
    frames_processed: AtomicU64,
}

impl AudioStream {
    /// Create a new audio stream.
    pub fn new(
        id: u32,
        sample_rate: u32,
        buffer_size: u32,
        channels: u32,
        priority: AudioPriority,
    ) -> Self {
        Self {
            id,
            sample_rate,
            buffer_size,
            channels,
            priority,
            running: AtomicBool::new(false),
            underruns: AtomicU32::new(0),
            overruns: AtomicU32::new(0),
            frames_processed: AtomicU64::new(0),
        }
    }

    /// Start the stream.
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Stop the stream.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get latency in microseconds.
    pub fn latency_us(&self) -> u64 {
        (self.buffer_size as u64 * 1_000_000) / self.sample_rate as u64
    }

    /// Get underrun count.
    pub fn underruns(&self) -> u32 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Get overrun count.
    pub fn overruns(&self) -> u32 {
        self.overruns.load(Ordering::Relaxed)
    }

    /// Record an underrun.
    pub fn record_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an overrun.
    pub fn record_overrun(&self) {
        self.overruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Get stream info.
    pub fn info(&self) -> StreamInfo {
        StreamInfo {
            id: self.id,
            sample_rate: self.sample_rate,
            buffer_size: self.buffer_size,
            channels: self.channels,
            latency_us: self.latency_us(),
            priority: self.priority,
        }
    }
}

/// Stream information.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub id: u32,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub channels: u32,
    pub latency_us: u64,
    pub priority: AudioPriority,
}

// =============================================================================
// Low-Latency Audio Engine
// =============================================================================

/// Audio error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    DeviceNotFound,
    InvalidConfig,
    InvalidNode,
    BufferOverrun,
    BufferUnderrun,
    StreamError,
    NotInitialized,
}

/// Low-latency audio engine.
pub struct LowLatencyEngine {
    /// Audio graph
    graph: AudioGraph,
    /// Active streams
    streams: BTreeMap<u32, AudioStream>,
    /// Output ring buffer
    output_buffer: LockFreeRingBuffer<Sample>,
    /// Input ring buffer
    input_buffer: LockFreeRingBuffer<Sample>,
    /// Engine sample rate
    sample_rate: u32,
    /// Engine buffer size
    buffer_size: u32,
    /// Channels
    channels: u32,
    /// Running
    running: AtomicBool,
    /// Next stream ID
    next_stream_id: AtomicU32,
}

impl LowLatencyEngine {
    /// Create a new low-latency audio engine.
    pub fn new(sample_rate: u32, buffer_size: u32, channels: u32) -> Self {
        // Create ring buffers sized for about 100ms of audio
        let ring_size = (sample_rate / 10 * channels) as usize;
        
        Self {
            graph: AudioGraph::new(sample_rate, buffer_size),
            streams: BTreeMap::new(),
            output_buffer: LockFreeRingBuffer::new(ring_size),
            input_buffer: LockFreeRingBuffer::new(ring_size),
            sample_rate,
            buffer_size,
            channels,
            running: AtomicBool::new(false),
            next_stream_id: AtomicU32::new(1),
        }
    }

    /// Start the engine.
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[LOWLAT AUDIO] Engine started: {}Hz, {} samples, {} channels",
            self.sample_rate, self.buffer_size, self.channels);
    }

    /// Stop the engine.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Create a new audio stream.
    pub fn create_stream(&mut self, priority: AudioPriority) -> u32 {
        let id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let stream = AudioStream::new(
            id,
            self.sample_rate,
            self.buffer_size,
            self.channels,
            priority,
        );
        self.streams.insert(id, stream);
        id
    }

    /// Start a stream.
    pub fn start_stream(&self, id: u32) -> Result<(), AudioError> {
        self.streams.get(&id)
            .ok_or(AudioError::StreamError)?
            .start();
        Ok(())
    }

    /// Stop a stream.
    pub fn stop_stream(&self, id: u32) -> Result<(), AudioError> {
        self.streams.get(&id)
            .ok_or(AudioError::StreamError)?
            .stop();
        Ok(())
    }

    /// Get stream info.
    pub fn stream_info(&self, id: u32) -> Option<StreamInfo> {
        self.streams.get(&id).map(|s| s.info())
    }

    /// Add a node to the audio graph.
    pub fn add_node(&mut self, node: Box<dyn AudioNode>) -> NodeId {
        self.graph.add_node(node)
    }

    /// Connect nodes in the audio graph.
    pub fn connect(&mut self, source: NodeId, dest: NodeId) -> Result<(), AudioError> {
        self.graph.connect(source, 0, dest, 0)
    }

    /// Set the output node.
    pub fn set_output(&mut self, node: NodeId) {
        self.graph.set_output(node);
    }

    /// Process one block of audio.
    pub fn process_block(&mut self) {
        if !self.is_running() {
            return;
        }
        
        let mut output = AudioBuffer::new(self.channels, self.buffer_size);
        
        // Process audio graph
        self.graph.process(&mut output);
        
        // Write to output ring buffer
        let written = self.output_buffer.write(output.data());
        if written < output.data().len() {
            // Buffer overrun - log but don't panic
            if let Some(stream) = self.streams.values().next() {
                stream.record_overrun();
            }
        }
    }

    /// Get output samples for playback.
    pub fn get_output(&self, buffer: &mut [Sample]) -> usize {
        self.output_buffer.read(buffer)
    }

    /// Write input samples from capture.
    pub fn put_input(&self, buffer: &[Sample]) -> usize {
        self.input_buffer.write(buffer)
    }

    /// Get engine latency in microseconds.
    pub fn latency_us(&self) -> u64 {
        (self.buffer_size as u64 * 1_000_000) / self.sample_rate as u64
    }

    /// Get engine sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get engine buffer size.
    pub fn buffer_size(&self) -> u32 {
        self.buffer_size
    }
}

// =============================================================================
// Global Instance
// =============================================================================

static LOW_LATENCY_ENGINE: Mutex<Option<LowLatencyEngine>> = Mutex::new(None);

/// Initialize the low-latency audio engine.
pub fn init(sample_rate: u32, buffer_size: u32, channels: u32) {
    let engine = LowLatencyEngine::new(sample_rate, buffer_size, channels);
    *LOW_LATENCY_ENGINE.lock() = Some(engine);
    crate::serial_println!("[LOWLAT AUDIO] Initialized: {}Hz, {} frames, {} ch, latency={}us",
        sample_rate, buffer_size, channels,
        (buffer_size as u64 * 1_000_000) / sample_rate as u64);
}

/// Get the low-latency audio engine.
pub fn engine() -> &'static Mutex<Option<LowLatencyEngine>> {
    &LOW_LATENCY_ENGINE
}

/// Default initialization with standard settings.
pub fn init_default() {
    // 48kHz, 128 frames buffer (~2.67ms latency), stereo
    init(48000, 128, 2);
}
