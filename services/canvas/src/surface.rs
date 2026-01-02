//! Surface management for S-CANVAS
//!
//! Surfaces are shared memory buffers that applications draw to.

use alloc::vec::Vec;
use super::{SurfaceId, WindowId, Rect, Color};

/// Pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit ARGB (alpha, red, green, blue)
    Argb8888,
    /// 32-bit XRGB (no alpha)
    Xrgb8888,
    /// 24-bit RGB
    Rgb888,
    /// 16-bit RGB (5-6-5)
    Rgb565,
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            PixelFormat::Argb8888 | PixelFormat::Xrgb8888 => 4,
            PixelFormat::Rgb888 => 3,
            PixelFormat::Rgb565 => 2,
        }
    }
}

/// Surface buffer state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    /// Buffer is free for client to draw
    Free,
    /// Client is drawing to buffer
    Drawing,
    /// Buffer is ready for composition
    Ready,
    /// Compositor is using buffer
    InUse,
}

/// Surface buffer
pub struct SurfaceBuffer {
    /// Buffer ID
    pub id: u32,
    /// Buffer state
    pub state: BufferState,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format
    pub format: PixelFormat,
    /// Stride (bytes per row)
    pub stride: u32,
    /// Shared memory offset (in shared buffer)
    pub shm_offset: usize,
    /// Damage since last present
    pub damage: Vec<Rect>,
}

/// Surface (drawable area within a window)
pub struct Surface {
    /// Surface ID
    pub id: SurfaceId,
    /// Owner window
    pub window: WindowId,
    /// Position within window
    pub x: i32,
    pub y: i32,
    /// Size
    pub width: u32,
    pub height: u32,
    /// Pixel format
    pub format: PixelFormat,
    /// Double/triple buffering
    pub buffers: Vec<SurfaceBuffer>,
    /// Current front buffer index
    pub front_buffer: usize,
    /// Current back buffer index
    pub back_buffer: usize,
    /// Opacity
    pub opacity: f32,
    /// Is surface visible
    pub visible: bool,
}

impl Surface {
    /// Create new surface
    pub fn new(
        id: SurfaceId,
        window: WindowId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Self {
        Self {
            id,
            window,
            x: 0,
            y: 0,
            width,
            height,
            format,
            buffers: Vec::new(),
            front_buffer: 0,
            back_buffer: 0,
            opacity: 1.0,
            visible: true,
        }
    }

    /// Attach buffer to surface
    pub fn attach_buffer(&mut self, buffer: SurfaceBuffer) {
        self.buffers.push(buffer);
        if self.buffers.len() == 1 {
            self.front_buffer = 0;
            self.back_buffer = 0;
        } else if self.buffers.len() == 2 {
            self.back_buffer = 1;
        }
    }

    /// Get back buffer for drawing
    pub fn get_back_buffer(&mut self) -> Option<&mut SurfaceBuffer> {
        self.buffers.get_mut(self.back_buffer)
    }

    /// Get front buffer for composition
    pub fn get_front_buffer(&self) -> Option<&SurfaceBuffer> {
        self.buffers.get(self.front_buffer)
    }

    /// Swap buffers (present)
    pub fn swap_buffers(&mut self) {
        if self.buffers.len() >= 2 {
            core::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
        }
    }

    /// Calculate stride
    pub fn calculate_stride(width: u32, format: PixelFormat) -> u32 {
        let bpp = format.bytes_per_pixel() as u32;
        // Align to 4 bytes
        (width * bpp + 3) & !3
    }

    /// Calculate buffer size
    pub fn calculate_size(width: u32, height: u32, format: PixelFormat) -> usize {
        Self::calculate_stride(width, format) as usize * height as usize
    }
}

/// Surface manager
pub struct SurfaceManager {
    /// All surfaces
    surfaces: Vec<Surface>,
    /// Next surface ID
    next_id: u64,
}

impl SurfaceManager {
    pub fn new() -> Self {
        Self {
            surfaces: Vec::new(),
            next_id: 1,
        }
    }

    /// Create new surface
    pub fn create_surface(
        &mut self,
        window: WindowId,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> SurfaceId {
        let id = SurfaceId(self.next_id);
        self.next_id += 1;
        
        let surface = Surface::new(id, window, width, height, format);
        self.surfaces.push(surface);
        id
    }

    /// Destroy surface
    pub fn destroy_surface(&mut self, id: SurfaceId) -> bool {
        if let Some(pos) = self.surfaces.iter().position(|s| s.id == id) {
            self.surfaces.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get surface
    pub fn get_surface(&self, id: SurfaceId) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.id == id)
    }

    /// Get mutable surface
    pub fn get_surface_mut(&mut self, id: SurfaceId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    /// Get all surfaces for a window
    pub fn surfaces_for_window(&self, window: WindowId) -> Vec<&Surface> {
        self.surfaces.iter().filter(|s| s.window == window).collect()
    }

    /// Destroy all surfaces for a window
    pub fn destroy_surfaces_for_window(&mut self, window: WindowId) {
        self.surfaces.retain(|s| s.window != window);
    }
}

impl Default for SurfaceManager {
    fn default() -> Self {
        Self::new()
    }
}
