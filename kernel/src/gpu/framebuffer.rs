//! # Framebuffer Driver
//!
//! Provides low-level framebuffer access and pixel operations.
//! Supports multiple pixel formats and double buffering.

use spin::Mutex;
use super::color::Color;

/// Pixel format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit ARGB (alpha in high byte)
    Argb32,
    /// 32-bit BGRA (blue in low byte)
    Bgra32,
    /// 32-bit RGBA
    Rgba32,
    /// 24-bit RGB (no alpha)
    Rgb24,
    /// 24-bit BGR
    Bgr24,
    /// 16-bit RGB565
    Rgb565,
    /// 8-bit indexed color
    Indexed8,
}

impl PixelFormat {
    /// Returns bytes per pixel for this format
    pub const fn bytes_per_pixel(&self) -> usize {
        match self {
            PixelFormat::Argb32 | PixelFormat::Bgra32 | PixelFormat::Rgba32 => 4,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
            PixelFormat::Rgb565 => 2,
            PixelFormat::Indexed8 => 1,
        }
    }
}

/// Display mode information
#[derive(Debug, Clone, Copy)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,  // Bytes per row (may include padding)
    pub bpp: u8,     // Bits per pixel
    pub format: PixelFormat,
}

impl DisplayMode {
    /// Returns the total framebuffer size in bytes
    pub const fn size(&self) -> usize {
        self.pitch as usize * self.height as usize
    }
}

/// Framebuffer info from bootloader
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub base_addr: usize,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bits_per_pixel: u8,
    pub pixel_format: PixelFormat,
}

/// Framebuffer instance
pub struct Framebuffer {
    /// Base address of the framebuffer
    base: usize,
    /// Display mode
    mode: DisplayMode,
    /// Framebuffer info (for external access)
    pub info: FramebufferInfo,
    /// Back buffer for double buffering (optional)
    back_buffer: Option<&'static mut [u8]>,
    /// Whether double buffering is enabled
    double_buffered: bool,
}

impl Framebuffer {
    /// Creates a new framebuffer from bootloader-provided info
    pub unsafe fn new(info: FramebufferInfo) -> Self {
        let mode = DisplayMode {
            width: info.width,
            height: info.height,
            pitch: info.pitch,
            bpp: info.bits_per_pixel,
            format: info.pixel_format,
        };
        Self {
            base: info.base_addr,
            mode,
            info,
            back_buffer: None,
            double_buffered: false,
        }
    }
    
    /// Returns the display mode
    pub fn mode(&self) -> DisplayMode {
        self.mode
    }
    
    /// Returns framebuffer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.mode.width, self.mode.height)
    }
    
    /// Gets the raw framebuffer slice
    pub fn buffer(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.base as *const u8,
                self.mode.size(),
            )
        }
    }
    
    /// Gets the raw framebuffer slice mutably
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.base as *mut u8,
                self.mode.size(),
            )
        }
    }
    
    /// Gets the active drawing buffer (back buffer if double buffered, else front)
    fn draw_buffer(&mut self) -> &mut [u8] {
        if self.double_buffered {
            if let Some(ref mut back) = self.back_buffer {
                return back;
            }
        }
        self.buffer_mut()
    }
    
    /// Calculates pixel offset for given coordinates
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.mode.width || y >= self.mode.height {
            return None;
        }
        let offset = (y as usize * self.mode.pitch as usize) 
            + (x as usize * self.mode.format.bytes_per_pixel());
        Some(offset)
    }
    
    /// Sets a pixel at the given coordinates
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if let Some(offset) = self.pixel_offset(x, y) {
            let format = self.mode.format;
            let buffer = self.draw_buffer();
            Self::write_pixel_to_buffer(buffer, offset, color, format);
        }
    }
    
    /// Gets a pixel at the given coordinates
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        let offset = self.pixel_offset(x, y)?;
        let buffer = self.buffer();
        Some(Self::read_pixel_from_buffer(buffer, offset, self.mode.format))
    }
    
    /// Writes a pixel to the buffer at the given offset
    fn write_pixel_to_buffer(buffer: &mut [u8], offset: usize, color: Color, format: PixelFormat) {
        match format {
            PixelFormat::Argb32 => {
                let value = color.to_argb32();
                buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            PixelFormat::Bgra32 => {
                buffer[offset] = color.b;
                buffer[offset + 1] = color.g;
                buffer[offset + 2] = color.r;
                buffer[offset + 3] = color.a;
            }
            PixelFormat::Rgba32 => {
                buffer[offset] = color.r;
                buffer[offset + 1] = color.g;
                buffer[offset + 2] = color.b;
                buffer[offset + 3] = color.a;
            }
            PixelFormat::Rgb24 => {
                buffer[offset] = color.r;
                buffer[offset + 1] = color.g;
                buffer[offset + 2] = color.b;
            }
            PixelFormat::Bgr24 => {
                buffer[offset] = color.b;
                buffer[offset + 1] = color.g;
                buffer[offset + 2] = color.r;
            }
            PixelFormat::Rgb565 => {
                let value = color.to_rgb565();
                buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
            PixelFormat::Indexed8 => {
                // For indexed mode, use grayscale approximation
                buffer[offset] = ((color.r as u16 + color.g as u16 + color.b as u16) / 3) as u8;
            }
        }
    }
    
    /// Reads a pixel from the buffer at the given offset
    fn read_pixel_from_buffer(buffer: &[u8], offset: usize, format: PixelFormat) -> Color {
        match format {
            PixelFormat::Argb32 => {
                let value = u32::from_le_bytes([
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                ]);
                Color::from_argb32(value)
            }
            PixelFormat::Bgra32 => {
                Color::rgba(
                    buffer[offset + 2],
                    buffer[offset + 1],
                    buffer[offset],
                    buffer[offset + 3],
                )
            }
            PixelFormat::Rgba32 => {
                Color::rgba(
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                )
            }
            PixelFormat::Rgb24 => {
                Color::rgb(
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                )
            }
            PixelFormat::Bgr24 => {
                Color::rgb(
                    buffer[offset + 2],
                    buffer[offset + 1],
                    buffer[offset],
                )
            }
            PixelFormat::Rgb565 => {
                let value = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                Color::from_rgb565(value)
            }
            PixelFormat::Indexed8 => {
                let gray = buffer[offset];
                Color::rgb(gray, gray, gray)
            }
        }
    }
    
    /// Clears the entire framebuffer with a color
    pub fn clear(&mut self, color: Color) {
        let width = self.mode.width;
        let height = self.mode.height;
        let pitch = self.mode.pitch as usize;
        let bpp = self.mode.format.bytes_per_pixel();
        
        // Prepare pixel data
        let mut pixel_data = [0u8; 4];
        match self.mode.format {
            PixelFormat::Argb32 => {
                pixel_data = color.to_argb32().to_le_bytes();
            }
            PixelFormat::Bgra32 => {
                pixel_data = [color.b, color.g, color.r, color.a];
            }
            PixelFormat::Rgba32 => {
                pixel_data = [color.r, color.g, color.b, color.a];
            }
            _ => {}
        }
        
        let buffer = self.draw_buffer();
        
        for y in 0..height as usize {
            let row_start = y * pitch;
            for x in 0..width as usize {
                let offset = row_start + x * bpp;
                buffer[offset..offset + bpp].copy_from_slice(&pixel_data[..bpp]);
            }
        }
    }
    
    /// Fills a rectangle with a color
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let x_end = (x + width).min(self.mode.width);
        let y_end = (y + height).min(self.mode.height);
        
        for py in y..y_end {
            for px in x..x_end {
                self.set_pixel(px, py, color);
            }
        }
    }
    
    /// Copies a region of pixels
    pub fn copy_rect(&mut self, src_x: u32, src_y: u32, dst_x: u32, dst_y: u32, width: u32, height: u32) {
        let bpp = self.mode.format.bytes_per_pixel();
        let pitch = self.mode.pitch as usize;
        
        // Handle overlapping regions
        let copy_up = dst_y < src_y;
        
        let buffer = self.draw_buffer();
        
        for i in 0..height {
            let y_offset = if copy_up { i } else { height - 1 - i };
            let src_row = ((src_y + y_offset) as usize) * pitch;
            let dst_row = ((dst_y + y_offset) as usize) * pitch;
            
            let src_start = src_row + (src_x as usize) * bpp;
            let dst_start = dst_row + (dst_x as usize) * bpp;
            let len = (width as usize) * bpp;
            
            // Use copy_within for potentially overlapping copies
            if src_start < dst_start {
                for j in (0..len).rev() {
                    buffer[dst_start + j] = buffer[src_start + j];
                }
            } else {
                for j in 0..len {
                    buffer[dst_start + j] = buffer[src_start + j];
                }
            }
        }
    }
    
    /// Scrolls the framebuffer up by the specified number of lines
    pub fn scroll_up(&mut self, lines: u32, fill_color: Color) {
        if lines == 0 || lines >= self.mode.height {
            self.clear(fill_color);
            return;
        }
        
        let width = self.mode.width;
        let height = self.mode.height;
        
        // Copy lines up
        self.copy_rect(0, lines, 0, 0, width, height - lines);
        
        // Fill bottom with fill color
        self.fill_rect(0, height - lines, width, lines, fill_color);
    }
    
    /// Swaps front and back buffers (if double buffered)
    pub fn swap_buffers(&mut self) {
        if !self.double_buffered {
            return;
        }
        
        // Get the back buffer data first
        let back_data: Option<(*const u8, usize)> = self.back_buffer.as_ref().map(|b| (b.as_ptr(), b.len()));
        
        if let Some((src, len)) = back_data {
            let front = self.buffer_mut();
            if front.len() >= len {
                // SAFETY: We're copying from back buffer to front buffer
                // Both buffers are valid and non-overlapping
                unsafe {
                    core::ptr::copy_nonoverlapping(src, front.as_mut_ptr(), len);
                }
            }
        }
    }
}

/// Global framebuffer instance
pub static FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);

/// Detects framebuffer from bootloader info
/// Returns None if no framebuffer is available
pub fn detect_framebuffer() -> Option<FramebufferInfo> {
    // Try to detect framebuffer from Limine bootloader
    #[cfg(feature = "limine")]
    {
        use crate::boot::limine;
        if let Some(fb_response) = limine::framebuffer_response() {
            if fb_response.framebuffer_count > 0 {
                let fb = &fb_response.framebuffers[0];
                return Some(FramebufferInfo {
                    base_addr: fb.address as usize,
                    width: fb.width as u32,
                    height: fb.height as u32,
                    pitch: fb.pitch as u32,
                    bits_per_pixel: fb.bpp as u8,
                    pixel_format: match (fb.red_mask_shift, fb.blue_mask_shift) {
                        (0, 16) => PixelFormat::Rgb,
                        (16, 0) => PixelFormat::Bgr,
                        _ => PixelFormat::Bgr, // Default
                    },
                });
            }
        }
    }
    
    // Try to detect from Multiboot2 info
    #[cfg(feature = "multiboot2")]
    {
        use crate::boot::multiboot2;
        if let Some(fb_tag) = multiboot2::framebuffer_tag() {
            return Some(FramebufferInfo {
                base_addr: fb_tag.address as usize,
                width: fb_tag.width,
                height: fb_tag.height,
                pitch: fb_tag.pitch,
                bits_per_pixel: fb_tag.bpp,
                pixel_format: match fb_tag.framebuffer_type {
                    1 => PixelFormat::Rgb, // Direct RGB
                    _ => PixelFormat::Bgr,
                },
            });
        }
    }
    
    // Fallback: Check for VGA text mode address (no graphics)
    None
}

/// Initializes the framebuffer from bootloader-provided info
pub fn init(info: FramebufferInfo) {
    let fb = unsafe { Framebuffer::new(info) };
    *FRAMEBUFFER.lock() = Some(fb);
}

/// Initializes the framebuffer with individual parameters (legacy)
pub fn init_with_params(base: usize, width: u32, height: u32, pitch: u32, bpp: u8, format: PixelFormat) {
    let info = FramebufferInfo {
        base_addr: base,
        width,
        height,
        pitch,
        bits_per_pixel: bpp,
        pixel_format: format,
    };
    init(info);
}

/// Gets the current display mode
pub fn display_mode() -> Option<DisplayMode> {
    FRAMEBUFFER.lock().as_ref().map(|fb| fb.mode())
}

/// Clears the screen with a color
pub fn clear(color: Color) {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.clear(color);
    }
}

/// Sets a pixel
pub fn set_pixel(x: u32, y: u32, color: Color) {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.set_pixel(x, y, color);
    }
}

/// Fills a rectangle
pub fn fill_rect(x: u32, y: u32, width: u32, height: u32, color: Color) {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.fill_rect(x, y, width, height, color);
    }
}

/// Scrolls the screen up
pub fn scroll_up(lines: u32, fill_color: Color) {
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.scroll_up(lines, fill_color);
    }
}
