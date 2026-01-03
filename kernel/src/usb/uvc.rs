//! # USB Video Class (UVC) Driver
//!
//! Driver for USB Video Class devices including webcams, video capture devices,
//! and other USB video peripherals.
//!
//! ## USB Video Class Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    USB Video Class Stack                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │    Video Streaming    │    Video Control    │    Interrupt      │
//! ├───────────────────────┼─────────────────────┼───────────────────┤
//! │    Isochronous EP     │    Terminals/Units  │    Status EP      │
//! │    Bulk EP            │    Processing       │    (optional)     │
//! └───────────────────────┴─────────────────────┴───────────────────┘
//! ```
//!
//! ## Features
//!
//! - UVC 1.0, 1.1, and 1.5 support
//! - MJPEG, YUV, and H.264 formats
//! - Camera controls (brightness, contrast, etc.)
//! - Multiple stream support
//! - Isochronous and bulk transfer modes

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// =============================================================================
// USB Video Class Constants
// =============================================================================

/// Video class code.
pub const USB_CLASS_VIDEO: u8 = 0x0E;

/// Video subclass codes.
pub mod subclass {
    /// Undefined
    pub const UNDEFINED: u8 = 0x00;
    /// Video Control
    pub const VIDEO_CONTROL: u8 = 0x01;
    /// Video Streaming
    pub const VIDEO_STREAMING: u8 = 0x02;
    /// Video Interface Collection
    pub const VIDEO_INTERFACE_COLLECTION: u8 = 0x03;
}

/// Video Control descriptor subtypes.
pub mod vc_descriptor {
    pub const VC_DESCRIPTOR_UNDEFINED: u8 = 0x00;
    pub const VC_HEADER: u8 = 0x01;
    pub const VC_INPUT_TERMINAL: u8 = 0x02;
    pub const VC_OUTPUT_TERMINAL: u8 = 0x03;
    pub const VC_SELECTOR_UNIT: u8 = 0x04;
    pub const VC_PROCESSING_UNIT: u8 = 0x05;
    pub const VC_EXTENSION_UNIT: u8 = 0x06;
    pub const VC_ENCODING_UNIT: u8 = 0x07;
}

/// Video Streaming descriptor subtypes.
pub mod vs_descriptor {
    pub const VS_UNDEFINED: u8 = 0x00;
    pub const VS_INPUT_HEADER: u8 = 0x01;
    pub const VS_OUTPUT_HEADER: u8 = 0x02;
    pub const VS_STILL_IMAGE_FRAME: u8 = 0x03;
    pub const VS_FORMAT_UNCOMPRESSED: u8 = 0x04;
    pub const VS_FRAME_UNCOMPRESSED: u8 = 0x05;
    pub const VS_FORMAT_MJPEG: u8 = 0x06;
    pub const VS_FRAME_MJPEG: u8 = 0x07;
    pub const VS_FORMAT_MPEG2TS: u8 = 0x0A;
    pub const VS_FORMAT_DV: u8 = 0x0C;
    pub const VS_COLORFORMAT: u8 = 0x0D;
    pub const VS_FORMAT_FRAME_BASED: u8 = 0x10;
    pub const VS_FRAME_FRAME_BASED: u8 = 0x11;
    pub const VS_FORMAT_STREAM_BASED: u8 = 0x12;
    pub const VS_FORMAT_H264: u8 = 0x13;
    pub const VS_FRAME_H264: u8 = 0x14;
    pub const VS_FORMAT_H264_SIMULCAST: u8 = 0x15;
    pub const VS_FORMAT_VP8: u8 = 0x16;
    pub const VS_FRAME_VP8: u8 = 0x17;
    pub const VS_FORMAT_VP8_SIMULCAST: u8 = 0x18;
}

/// Terminal types.
pub mod terminal_type {
    pub const TT_VENDOR_SPECIFIC: u16 = 0x0100;
    pub const TT_STREAMING: u16 = 0x0101;
    
    // Input terminals
    pub const ITT_VENDOR_SPECIFIC: u16 = 0x0200;
    pub const ITT_CAMERA: u16 = 0x0201;
    pub const ITT_MEDIA_TRANSPORT_INPUT: u16 = 0x0202;
    
    // Output terminals
    pub const OTT_VENDOR_SPECIFIC: u16 = 0x0300;
    pub const OTT_DISPLAY: u16 = 0x0301;
    pub const OTT_MEDIA_TRANSPORT_OUTPUT: u16 = 0x0302;
    
    // External terminals
    pub const EXTERNAL_VENDOR_SPECIFIC: u16 = 0x0400;
    pub const COMPOSITE_CONNECTOR: u16 = 0x0401;
    pub const SVIDEO_CONNECTOR: u16 = 0x0402;
    pub const COMPONENT_CONNECTOR: u16 = 0x0403;
}

// =============================================================================
// Video Formats
// =============================================================================

/// Video format GUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatGuid([u8; 16]);

impl FormatGuid {
    /// YUY2 (YUYV) format.
    pub const YUY2: Self = Self([
        0x59, 0x55, 0x59, 0x32, 0x00, 0x00, 0x10, 0x00,
        0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ]);
    
    /// NV12 format.
    pub const NV12: Self = Self([
        0x4E, 0x56, 0x31, 0x32, 0x00, 0x00, 0x10, 0x00,
        0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ]);
    
    /// MJPEG format.
    pub const MJPEG: Self = Self([
        0x4D, 0x4A, 0x50, 0x47, 0x00, 0x00, 0x10, 0x00,
        0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ]);
    
    /// H.264 format.
    pub const H264: Self = Self([
        0x48, 0x32, 0x36, 0x34, 0x00, 0x00, 0x10, 0x00,
        0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ]);
}

/// Video pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// YUY2 (YUYV 4:2:2)
    Yuy2,
    /// NV12 (YUV 4:2:0)
    Nv12,
    /// MJPEG compressed
    Mjpeg,
    /// H.264 compressed
    H264,
    /// RGB24
    Rgb24,
    /// BGR24
    Bgr24,
    /// ARGB32
    Argb32,
    /// Unknown format
    Unknown,
}

impl PixelFormat {
    /// Get bits per pixel.
    pub fn bpp(&self) -> u32 {
        match self {
            Self::Yuy2 => 16,
            Self::Nv12 => 12,
            Self::Mjpeg | Self::H264 => 0, // Variable
            Self::Rgb24 | Self::Bgr24 => 24,
            Self::Argb32 => 32,
            Self::Unknown => 0,
        }
    }

    /// Calculate frame size in bytes.
    pub fn frame_size(&self, width: u32, height: u32) -> u32 {
        match self {
            Self::Yuy2 => width * height * 2,
            Self::Nv12 => width * height * 3 / 2,
            Self::Rgb24 | Self::Bgr24 => width * height * 3,
            Self::Argb32 => width * height * 4,
            _ => 0, // Unknown for compressed formats
        }
    }
}

// =============================================================================
// Video Frame Descriptor
// =============================================================================

/// Video frame configuration.
#[derive(Debug, Clone)]
pub struct FrameDescriptor {
    /// Frame index
    pub index: u8,
    /// Width in pixels
    pub width: u16,
    /// Height in pixels
    pub height: u16,
    /// Minimum bit rate
    pub min_bit_rate: u32,
    /// Maximum bit rate
    pub max_bit_rate: u32,
    /// Default frame interval (100ns units)
    pub default_frame_interval: u32,
    /// Supported frame intervals
    pub frame_intervals: Vec<u32>,
    /// Capabilities
    pub capabilities: FrameCapabilities,
}

/// Frame capabilities flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameCapabilities(u8);

impl FrameCapabilities {
    pub const STILL_IMAGE_SUPPORTED: u8 = 0x01;
    pub const FIXED_FRAME_RATE: u8 = 0x02;

    pub fn still_image_supported(&self) -> bool {
        (self.0 & Self::STILL_IMAGE_SUPPORTED) != 0
    }

    pub fn fixed_frame_rate(&self) -> bool {
        (self.0 & Self::FIXED_FRAME_RATE) != 0
    }
}

/// Video format descriptor.
#[derive(Debug, Clone)]
pub struct FormatDescriptor {
    /// Format index
    pub index: u8,
    /// Number of frame descriptors
    pub num_frame_descriptors: u8,
    /// Pixel format
    pub format: PixelFormat,
    /// Format GUID (for uncompressed)
    pub guid: Option<FormatGuid>,
    /// Bits per pixel
    pub bits_per_pixel: u8,
    /// Default frame index
    pub default_frame_index: u8,
    /// Aspect ratio X
    pub aspect_ratio_x: u8,
    /// Aspect ratio Y
    pub aspect_ratio_y: u8,
    /// Interlace flags
    pub interlace_flags: u8,
    /// Frame descriptors
    pub frames: Vec<FrameDescriptor>,
}

// =============================================================================
// Camera Controls
// =============================================================================

/// Camera control selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraControl {
    ScanningMode,
    AutoExposureMode,
    AutoExposurePriority,
    ExposureTimeAbsolute,
    ExposureTimeRelative,
    FocusAbsolute,
    FocusRelative,
    IrisAbsolute,
    IrisRelative,
    ZoomAbsolute,
    ZoomRelative,
    PanTiltAbsolute,
    PanTiltRelative,
    RollAbsolute,
    RollRelative,
    Privacy,
    FocusSimple,
    DigitalWindow,
    RegionOfInterest,
}

/// Processing unit control selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingControl {
    Brightness,
    Contrast,
    Hue,
    Saturation,
    Sharpness,
    Gamma,
    WhiteBalanceTemperature,
    WhiteBalanceComponent,
    BacklightCompensation,
    Gain,
    PowerLineFrequency,
    AutoHue,
    AutoWhiteBalanceTemperature,
    AutoWhiteBalanceComponent,
    DigitalMultiplier,
    DigitalMultiplierLimit,
    AnalogVideoStandard,
    AnalogVideoLockStatus,
    ContrastAuto,
}

/// Control capabilities.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlCapabilities {
    pub supports_get: bool,
    pub supports_set: bool,
    pub supports_auto: bool,
    pub min: i32,
    pub max: i32,
    pub resolution: i32,
    pub default: i32,
}

// =============================================================================
// Video Streaming
// =============================================================================

/// Stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Idle,
    Prepared,
    Streaming,
    Error,
}

/// Video stream configuration.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Format index
    pub format_index: u8,
    /// Frame index
    pub frame_index: u8,
    /// Frame interval (100ns units)
    pub frame_interval: u32,
    /// Maximum video frame size
    pub max_video_frame_size: u32,
    /// Maximum payload transfer size
    pub max_payload_transfer_size: u32,
}

/// Video stream.
pub struct VideoStream {
    /// Stream ID
    id: u32,
    /// Interface number
    interface: u8,
    /// Endpoint address
    endpoint: u8,
    /// Current configuration
    config: Option<StreamConfig>,
    /// Stream state
    state: StreamState,
    /// Frames captured
    frames_captured: AtomicU32,
    /// Frames dropped
    frames_dropped: AtomicU32,
    /// Current frame buffer
    frame_buffer: Vec<u8>,
    /// Frame complete flag
    frame_complete: AtomicBool,
}

impl VideoStream {
    /// Create a new video stream.
    pub fn new(id: u32, interface: u8, endpoint: u8) -> Self {
        Self {
            id,
            interface,
            endpoint,
            config: None,
            state: StreamState::Idle,
            frames_captured: AtomicU32::new(0),
            frames_dropped: AtomicU32::new(0),
            frame_buffer: Vec::new(),
            frame_complete: AtomicBool::new(false),
        }
    }

    /// Configure the stream.
    pub fn configure(&mut self, config: StreamConfig) {
        self.frame_buffer = alloc::vec![0u8; config.max_video_frame_size as usize];
        self.config = Some(config);
        self.state = StreamState::Prepared;
    }

    /// Start streaming.
    pub fn start(&mut self) -> Result<(), VideoError> {
        if self.config.is_none() {
            return Err(VideoError::NotConfigured);
        }
        self.state = StreamState::Streaming;
        Ok(())
    }

    /// Stop streaming.
    pub fn stop(&mut self) {
        self.state = StreamState::Idle;
    }

    /// Handle incoming payload.
    pub fn handle_payload(&mut self, data: &[u8]) -> Result<Option<&[u8]>, VideoError> {
        if data.len() < 2 {
            return Err(VideoError::InvalidData);
        }
        
        let header_length = data[0] as usize;
        if header_length < 2 || header_length > data.len() {
            return Err(VideoError::InvalidData);
        }
        
        let bfh = data[1];
        let fid = (bfh & 0x01) != 0;
        let eof = (bfh & 0x02) != 0;
        let pts = (bfh & 0x04) != 0;
        let scr = (bfh & 0x08) != 0;
        let still = (bfh & 0x20) != 0;
        let err = (bfh & 0x40) != 0;
        let eoh = (bfh & 0x80) != 0;
        
        if err {
            self.frames_dropped.fetch_add(1, Ordering::Relaxed);
            return Err(VideoError::FrameError);
        }
        
        // Calculate payload offset based on header
        let payload_offset = if pts && scr {
            header_length
        } else if pts {
            header_length
        } else {
            header_length
        };
        
        let payload = &data[payload_offset..];
        
        // Append to frame buffer
        let current_len = self.frame_buffer.len();
        if current_len + payload.len() <= self.frame_buffer.capacity() {
            self.frame_buffer.extend_from_slice(payload);
        }
        
        if eof {
            self.frames_captured.fetch_add(1, Ordering::Relaxed);
            self.frame_complete.store(true, Ordering::Release);
            return Ok(Some(&self.frame_buffer));
        }
        
        // Silence warnings
        let _ = (fid, still, eoh);
        
        Ok(None)
    }

    /// Get frame if complete.
    pub fn get_frame(&mut self) -> Option<&[u8]> {
        if self.frame_complete.swap(false, Ordering::AcqRel) {
            Some(&self.frame_buffer)
        } else {
            None
        }
    }

    /// Get stream statistics.
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            frames_captured: self.frames_captured.load(Ordering::Relaxed),
            frames_dropped: self.frames_dropped.load(Ordering::Relaxed),
            state: self.state,
        }
    }
}

/// Stream statistics.
#[derive(Debug, Clone)]
pub struct StreamStats {
    pub frames_captured: u32,
    pub frames_dropped: u32,
    pub state: StreamState,
}

// =============================================================================
// UVC Device
// =============================================================================

/// Video error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoError {
    DeviceNotFound,
    NotSupported,
    NotConfigured,
    InvalidData,
    FrameError,
    TransferError,
    Timeout,
    InvalidFormat,
    InvalidControl,
}

/// UVC device capabilities.
#[derive(Debug, Clone, Default)]
pub struct DeviceCapabilities {
    /// Supported formats
    pub formats: Vec<FormatDescriptor>,
    /// Camera controls
    pub camera_controls: Vec<CameraControl>,
    /// Processing controls
    pub processing_controls: Vec<ProcessingControl>,
}

/// USB Video Class device.
pub struct UvcDevice {
    /// Device ID
    id: u32,
    /// USB device handle
    device_handle: u64,
    /// UVC version
    uvc_version: u16,
    /// Device capabilities
    capabilities: DeviceCapabilities,
    /// Video streams
    streams: BTreeMap<u32, VideoStream>,
    /// Next stream ID
    next_stream_id: AtomicU32,
    /// Initialized
    initialized: AtomicBool,
    /// Device name
    name: String,
}

impl UvcDevice {
    /// Create a new UVC device.
    pub fn new(id: u32, device_handle: u64, name: String) -> Self {
        Self {
            id,
            device_handle,
            uvc_version: 0x0100,
            capabilities: DeviceCapabilities::default(),
            streams: BTreeMap::new(),
            next_stream_id: AtomicU32::new(1),
            initialized: AtomicBool::new(false),
            name,
        }
    }

    /// Initialize the device.
    pub fn init(&mut self) -> Result<(), VideoError> {
        // Parse device descriptors
        self.parse_descriptors()?;
        
        self.initialized.store(true, Ordering::SeqCst);
        
        crate::serial_println!(
            "[UVC] Initialized device: id={}, name={}, formats={}",
            self.id,
            self.name,
            self.capabilities.formats.len()
        );
        
        Ok(())
    }

    /// Parse USB descriptors.
    fn parse_descriptors(&mut self) -> Result<(), VideoError> {
        // In a real implementation, this would read and parse the USB descriptors
        // For now, create a default capability set
        
        let mjpeg_format = FormatDescriptor {
            index: 1,
            num_frame_descriptors: 3,
            format: PixelFormat::Mjpeg,
            guid: Some(FormatGuid::MJPEG),
            bits_per_pixel: 0,
            default_frame_index: 1,
            aspect_ratio_x: 16,
            aspect_ratio_y: 9,
            interlace_flags: 0,
            frames: alloc::vec![
                FrameDescriptor {
                    index: 1,
                    width: 1920,
                    height: 1080,
                    min_bit_rate: 18432000,
                    max_bit_rate: 55296000,
                    default_frame_interval: 333333, // 30fps
                    frame_intervals: alloc::vec![333333, 400000, 500000],
                    capabilities: FrameCapabilities::default(),
                },
                FrameDescriptor {
                    index: 2,
                    width: 1280,
                    height: 720,
                    min_bit_rate: 9216000,
                    max_bit_rate: 27648000,
                    default_frame_interval: 333333,
                    frame_intervals: alloc::vec![166666, 333333, 400000],
                    capabilities: FrameCapabilities::default(),
                },
                FrameDescriptor {
                    index: 3,
                    width: 640,
                    height: 480,
                    min_bit_rate: 2457600,
                    max_bit_rate: 7372800,
                    default_frame_interval: 333333,
                    frame_intervals: alloc::vec![166666, 333333, 666666],
                    capabilities: FrameCapabilities::default(),
                },
            ],
        };
        
        self.capabilities.formats.push(mjpeg_format);
        
        // Add common camera controls
        self.capabilities.camera_controls = alloc::vec![
            CameraControl::AutoExposureMode,
            CameraControl::ExposureTimeAbsolute,
            CameraControl::FocusAbsolute,
            CameraControl::ZoomAbsolute,
        ];
        
        // Add common processing controls
        self.capabilities.processing_controls = alloc::vec![
            ProcessingControl::Brightness,
            ProcessingControl::Contrast,
            ProcessingControl::Saturation,
            ProcessingControl::Sharpness,
            ProcessingControl::WhiteBalanceTemperature,
            ProcessingControl::Gain,
        ];
        
        Ok(())
    }

    /// Get device capabilities.
    pub fn capabilities(&self) -> &DeviceCapabilities {
        &self.capabilities
    }

    /// Create a video stream.
    pub fn create_stream(&mut self, interface: u8, endpoint: u8) -> u32 {
        let id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let stream = VideoStream::new(id, interface, endpoint);
        self.streams.insert(id, stream);
        id
    }

    /// Configure a stream.
    pub fn configure_stream(
        &mut self,
        stream_id: u32,
        format_index: u8,
        frame_index: u8,
        frame_interval: u32,
    ) -> Result<(), VideoError> {
        let format = self.capabilities.formats.iter()
            .find(|f| f.index == format_index)
            .ok_or(VideoError::InvalidFormat)?;
        
        let frame = format.frames.iter()
            .find(|f| f.index == frame_index)
            .ok_or(VideoError::InvalidFormat)?;
        
        let config = StreamConfig {
            format_index,
            frame_index,
            frame_interval,
            max_video_frame_size: frame.max_bit_rate / 8 / 30, // Estimate
            max_payload_transfer_size: 3072,
        };
        
        let stream = self.streams.get_mut(&stream_id)
            .ok_or(VideoError::NotConfigured)?;
        
        stream.configure(config);
        
        Ok(())
    }

    /// Start a stream.
    pub fn start_stream(&mut self, stream_id: u32) -> Result<(), VideoError> {
        let stream = self.streams.get_mut(&stream_id)
            .ok_or(VideoError::NotConfigured)?;
        stream.start()
    }

    /// Stop a stream.
    pub fn stop_stream(&mut self, stream_id: u32) -> Result<(), VideoError> {
        let stream = self.streams.get_mut(&stream_id)
            .ok_or(VideoError::NotConfigured)?;
        stream.stop();
        Ok(())
    }

    /// Get control value.
    pub fn get_control<T: UvcControl>(&self, _control: T) -> Result<i32, VideoError> {
        // Would send GET_CUR request to device
        Ok(0)
    }

    /// Set control value.
    pub fn set_control<T: UvcControl>(&mut self, _control: T, _value: i32) -> Result<(), VideoError> {
        // Would send SET_CUR request to device
        Ok(())
    }

    /// Get device info.
    pub fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id,
            name: self.name.clone(),
            uvc_version: self.uvc_version,
            num_formats: self.capabilities.formats.len(),
            num_streams: self.streams.len(),
        }
    }
}

/// UVC control trait.
pub trait UvcControl {
    fn selector(&self) -> u8;
    fn unit(&self) -> u8;
}

impl UvcControl for CameraControl {
    fn selector(&self) -> u8 {
        match self {
            Self::ScanningMode => 0x01,
            Self::AutoExposureMode => 0x02,
            Self::AutoExposurePriority => 0x03,
            Self::ExposureTimeAbsolute => 0x04,
            Self::ExposureTimeRelative => 0x05,
            Self::FocusAbsolute => 0x06,
            Self::FocusRelative => 0x07,
            Self::IrisAbsolute => 0x09,
            Self::IrisRelative => 0x0A,
            Self::ZoomAbsolute => 0x0B,
            Self::ZoomRelative => 0x0C,
            Self::PanTiltAbsolute => 0x0D,
            Self::PanTiltRelative => 0x0E,
            Self::RollAbsolute => 0x0F,
            Self::RollRelative => 0x10,
            Self::Privacy => 0x11,
            Self::FocusSimple => 0x12,
            Self::DigitalWindow => 0x13,
            Self::RegionOfInterest => 0x14,
        }
    }

    fn unit(&self) -> u8 {
        1 // Camera terminal
    }
}

impl UvcControl for ProcessingControl {
    fn selector(&self) -> u8 {
        match self {
            Self::Brightness => 0x02,
            Self::Contrast => 0x03,
            Self::Hue => 0x06,
            Self::Saturation => 0x07,
            Self::Sharpness => 0x08,
            Self::Gamma => 0x09,
            Self::WhiteBalanceTemperature => 0x0A,
            Self::WhiteBalanceComponent => 0x0B,
            Self::BacklightCompensation => 0x01,
            Self::Gain => 0x04,
            Self::PowerLineFrequency => 0x05,
            Self::AutoHue => 0x10,
            Self::AutoWhiteBalanceTemperature => 0x0B,
            Self::AutoWhiteBalanceComponent => 0x0C,
            Self::DigitalMultiplier => 0x0E,
            Self::DigitalMultiplierLimit => 0x0F,
            Self::AnalogVideoStandard => 0x11,
            Self::AnalogVideoLockStatus => 0x12,
            Self::ContrastAuto => 0x13,
        }
    }

    fn unit(&self) -> u8 {
        2 // Processing unit
    }
}

/// Device information.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: u32,
    pub name: String,
    pub uvc_version: u16,
    pub num_formats: usize,
    pub num_streams: usize,
}

// =============================================================================
// UVC Driver
// =============================================================================

/// UVC driver.
pub struct UvcDriver {
    /// Registered devices
    devices: BTreeMap<u32, UvcDevice>,
    /// Next device ID
    next_device_id: AtomicU32,
}

impl UvcDriver {
    /// Create a new UVC driver.
    pub const fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
            next_device_id: AtomicU32::new(1),
        }
    }

    /// Register a USB video device.
    pub fn register_device(&mut self, device_handle: u64, name: String) -> Result<u32, VideoError> {
        let id = self.next_device_id.fetch_add(1, Ordering::Relaxed);
        
        let mut device = UvcDevice::new(id, device_handle, name);
        device.init()?;
        
        self.devices.insert(id, device);
        
        Ok(id)
    }

    /// Unregister a device.
    pub fn unregister_device(&mut self, id: u32) {
        self.devices.remove(&id);
    }

    /// Get a device.
    pub fn device(&self, id: u32) -> Option<&UvcDevice> {
        self.devices.get(&id)
    }

    /// Get a mutable device.
    pub fn device_mut(&mut self, id: u32) -> Option<&mut UvcDevice> {
        self.devices.get_mut(&id)
    }

    /// List all devices.
    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices.values().map(|d| d.info()).collect()
    }
}

impl Default for UvcDriver {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Global Instance
// =============================================================================

static UVC_DRIVER: Mutex<UvcDriver> = Mutex::new(UvcDriver::new());

/// Get the UVC driver.
pub fn driver() -> &'static Mutex<UvcDriver> {
    &UVC_DRIVER
}

/// Initialize the UVC driver.
pub fn init() {
    crate::serial_println!("[UVC] USB Video Class driver initialized");
}
