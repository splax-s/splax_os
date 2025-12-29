//! Framebuffer operations for S-GPU service

use alloc::vec::Vec;
use super::{Color, Rect, Point, GpuError, DisplayMode};

/// Framebuffer structure
pub struct Framebuffer {
    /// Raw pixel buffer
    buffer: Vec<u8>,
    /// Back buffer for double buffering
    back_buffer: Option<Vec<u8>>,
    /// Display mode
    mode: DisplayMode,
    /// Dirty regions that need updating
    dirty_rects: Vec<Rect>,
    /// Whether double buffering is enabled
    double_buffered: bool,
}

impl Framebuffer {
    /// Create a new framebuffer
    pub fn new(mode: DisplayMode, double_buffered: bool) -> Self {
        let size = mode.pitch as usize * mode.height as usize;
        
        let back_buffer = if double_buffered {
            Some(alloc::vec![0u8; size])
        } else {
            None
        };
        
        Self {
            buffer: alloc::vec![0u8; size],
            back_buffer,
            mode,
            dirty_rects: Vec::new(),
            double_buffered,
        }
    }

    /// Get framebuffer width
    pub fn width(&self) -> u32 {
        self.mode.width
    }

    /// Get framebuffer height
    pub fn height(&self) -> u32 {
        self.mode.height
    }

    /// Get bits per pixel
    pub fn bpp(&self) -> u8 {
        self.mode.bpp
    }

    /// Get bytes per pixel
    pub fn bytes_per_pixel(&self) -> usize {
        (self.mode.bpp / 8) as usize
    }

    /// Get pitch (bytes per scanline)
    pub fn pitch(&self) -> u32 {
        self.mode.pitch
    }

    /// Get raw buffer
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Get mutable raw buffer
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        if self.double_buffered {
            self.back_buffer.as_mut().unwrap()
        } else {
            &mut self.buffer
        }
    }

    /// Clear the framebuffer with a color
    pub fn clear(&mut self, color: Color) {
        // Cache mode values before mutable borrow
        let bpp = self.mode.bpp;
        let width = self.mode.width;
        let height = self.mode.height;
        let buffer = self.buffer_mut();
        
        match bpp {
            32 => {
                let pixel = color.to_argb32();
                for chunk in buffer.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&pixel.to_le_bytes());
                }
            }
            24 => {
                for chunk in buffer.chunks_exact_mut(3) {
                    chunk[0] = color.b;
                    chunk[1] = color.g;
                    chunk[2] = color.r;
                }
            }
            16 => {
                let pixel = color.to_rgb565();
                for chunk in buffer.chunks_exact_mut(2) {
                    chunk.copy_from_slice(&pixel.to_le_bytes());
                }
            }
            _ => {}
        }
        
        self.mark_dirty(Rect::new(0, 0, width, height));
    }

    /// Set a pixel
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.mode.width as i32 || y >= self.mode.height as i32 {
            return;
        }
        
        // Cache mode values before mutable borrow
        let bpp = self.mode.bpp;
        let pitch = self.mode.pitch as usize;
        let bytes_per_pixel = self.bytes_per_pixel();
        let offset = y as usize * pitch + x as usize * bytes_per_pixel;
        let buffer = self.buffer_mut();
        
        match bpp {
            32 => {
                let pixel = color.to_argb32();
                if offset + 4 <= buffer.len() {
                    buffer[offset..offset + 4].copy_from_slice(&pixel.to_le_bytes());
                }
            }
            24 => {
                if offset + 3 <= buffer.len() {
                    buffer[offset] = color.b;
                    buffer[offset + 1] = color.g;
                    buffer[offset + 2] = color.r;
                }
            }
            16 => {
                let pixel = color.to_rgb565();
                if offset + 2 <= buffer.len() {
                    buffer[offset..offset + 2].copy_from_slice(&pixel.to_le_bytes());
                }
            }
            _ => {}
        }
    }

    /// Get a pixel
    pub fn get_pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.mode.width as i32 || y >= self.mode.height as i32 {
            return None;
        }
        
        let offset = y as usize * self.mode.pitch as usize 
            + x as usize * self.bytes_per_pixel();
        let buffer = &self.buffer;
        
        match self.mode.bpp {
            32 => {
                if offset + 4 <= buffer.len() {
                    let b = buffer[offset];
                    let g = buffer[offset + 1];
                    let r = buffer[offset + 2];
                    let a = buffer[offset + 3];
                    Some(Color::rgba(r, g, b, a))
                } else {
                    None
                }
            }
            24 => {
                if offset + 3 <= buffer.len() {
                    let b = buffer[offset];
                    let g = buffer[offset + 1];
                    let r = buffer[offset + 2];
                    Some(Color::rgb(r, g, b))
                } else {
                    None
                }
            }
            16 => {
                if offset + 2 <= buffer.len() {
                    let pixel = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                    let r = ((pixel >> 11) & 0x1F) as u8 * 8;
                    let g = ((pixel >> 5) & 0x3F) as u8 * 4;
                    let b = (pixel & 0x1F) as u8 * 8;
                    Some(Color::rgb(r, g, b))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Fill a rectangle with a color
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        // Clip to screen bounds
        let x_start = rect.x.max(0) as u32;
        let y_start = rect.y.max(0) as u32;
        let x_end = ((rect.x + rect.width as i32) as u32).min(self.mode.width);
        let y_end = ((rect.y + rect.height as i32) as u32).min(self.mode.height);
        
        if x_start >= x_end || y_start >= y_end {
            return;
        }
        
        // Cache mode values before mutable borrow
        let bpp = self.mode.bpp;
        let pitch = self.mode.pitch as usize;
        let buffer = self.buffer_mut();
        
        match bpp {
            32 => {
                let pixel = color.to_argb32().to_le_bytes();
                for y in y_start..y_end {
                    let row_offset = y as usize * pitch;
                    for x in x_start..x_end {
                        let offset = row_offset + x as usize * 4;
                        if offset + 4 <= buffer.len() {
                            buffer[offset..offset + 4].copy_from_slice(&pixel);
                        }
                    }
                }
            }
            24 => {
                for y in y_start..y_end {
                    let row_offset = y as usize * pitch;
                    for x in x_start..x_end {
                        let offset = row_offset + x as usize * 3;
                        if offset + 3 <= buffer.len() {
                            buffer[offset] = color.b;
                            buffer[offset + 1] = color.g;
                            buffer[offset + 2] = color.r;
                        }
                    }
                }
            }
            16 => {
                let pixel = color.to_rgb565().to_le_bytes();
                for y in y_start..y_end {
                    let row_offset = y as usize * pitch;
                    for x in x_start..x_end {
                        let offset = row_offset + x as usize * 2;
                        if offset + 2 <= buffer.len() {
                            buffer[offset..offset + 2].copy_from_slice(&pixel);
                        }
                    }
                }
            }
            _ => {}
        }
        
        self.mark_dirty(Rect::new(
            x_start as i32,
            y_start as i32,
            x_end - x_start,
            y_end - y_start,
        ));
    }

    /// Draw a rectangle outline
    pub fn draw_rect(&mut self, rect: Rect, color: Color, thickness: u32) {
        let t = thickness as i32;
        
        // Top edge
        self.fill_rect(
            Rect::new(rect.x, rect.y, rect.width, thickness),
            color,
        );
        // Bottom edge
        self.fill_rect(
            Rect::new(rect.x, rect.y + rect.height as i32 - t, rect.width, thickness),
            color,
        );
        // Left edge
        self.fill_rect(
            Rect::new(rect.x, rect.y + t, thickness, rect.height - thickness * 2),
            color,
        );
        // Right edge
        self.fill_rect(
            Rect::new(rect.x + rect.width as i32 - t, rect.y + t, thickness, rect.height - thickness * 2),
            color,
        );
    }

    /// Draw a line using Bresenham's algorithm
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        
        let mut x = x0;
        let mut y = y0;
        
        loop {
            self.set_pixel(x, y, color);
            
            if x == x1 && y == y1 {
                break;
            }
            
            let e2 = 2 * err;
            
            if e2 >= dy {
                if x == x1 {
                    break;
                }
                err += dy;
                x += sx;
            }
            
            if e2 <= dx {
                if y == y1 {
                    break;
                }
                err += dx;
                y += sy;
            }
        }
        
        // Mark line bounding box as dirty
        let min_x = x0.min(x1);
        let min_y = y0.min(y1);
        let max_x = x0.max(x1);
        let max_y = y0.max(y1);
        self.mark_dirty(Rect::new(
            min_x,
            min_y,
            (max_x - min_x + 1) as u32,
            (max_y - min_y + 1) as u32,
        ));
    }

    /// Draw a circle using midpoint algorithm
    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;
        
        while x >= y {
            self.set_pixel(cx + x, cy + y, color);
            self.set_pixel(cx + y, cy + x, color);
            self.set_pixel(cx - y, cy + x, color);
            self.set_pixel(cx - x, cy + y, color);
            self.set_pixel(cx - x, cy - y, color);
            self.set_pixel(cx - y, cy - x, color);
            self.set_pixel(cx + y, cy - x, color);
            self.set_pixel(cx + x, cy - y, color);
            
            y += 1;
            if err <= 0 {
                err += 2 * y + 1;
            }
            if err > 0 {
                x -= 1;
                err -= 2 * x + 1;
            }
        }
        
        self.mark_dirty(Rect::new(
            cx - radius,
            cy - radius,
            (radius * 2 + 1) as u32,
            (radius * 2 + 1) as u32,
        ));
    }

    /// Fill a circle
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let r2 = radius * radius;
        
        for y in -radius..=radius {
            let y2 = y * y;
            for x in -radius..=radius {
                if x * x + y2 <= r2 {
                    self.set_pixel(cx + x, cy + y, color);
                }
            }
        }
        
        self.mark_dirty(Rect::new(
            cx - radius,
            cy - radius,
            (radius * 2 + 1) as u32,
            (radius * 2 + 1) as u32,
        ));
    }

    /// Blit a source buffer to the framebuffer
    pub fn blit(
        &mut self,
        src: &[u8],
        src_width: u32,
        src_height: u32,
        src_pitch: u32,
        dest_x: i32,
        dest_y: i32,
    ) {
        let bytes_per_pixel = self.bytes_per_pixel();
        
        for y in 0..src_height {
            let dest_row = dest_y + y as i32;
            if dest_row < 0 || dest_row >= self.mode.height as i32 {
                continue;
            }
            
            for x in 0..src_width {
                let dest_col = dest_x + x as i32;
                if dest_col < 0 || dest_col >= self.mode.width as i32 {
                    continue;
                }
                
                let src_offset = y as usize * src_pitch as usize 
                    + x as usize * bytes_per_pixel;
                let dest_offset = dest_row as usize * self.mode.pitch as usize 
                    + dest_col as usize * bytes_per_pixel;
                
                if src_offset + bytes_per_pixel <= src.len() 
                    && dest_offset + bytes_per_pixel <= self.buffer_mut().len() 
                {
                    let buffer = self.buffer_mut();
                    buffer[dest_offset..dest_offset + bytes_per_pixel]
                        .copy_from_slice(&src[src_offset..src_offset + bytes_per_pixel]);
                }
            }
        }
        
        self.mark_dirty(Rect::new(dest_x, dest_y, src_width, src_height));
    }

    /// Alpha-blended blit
    pub fn blit_alpha(
        &mut self,
        src: &[u8],
        src_width: u32,
        src_height: u32,
        dest_x: i32,
        dest_y: i32,
    ) {
        // Assumes 32-bit ARGB source and destination
        if self.mode.bpp != 32 {
            return;
        }
        
        for y in 0..src_height {
            let dest_row = dest_y + y as i32;
            if dest_row < 0 || dest_row >= self.mode.height as i32 {
                continue;
            }
            
            for x in 0..src_width {
                let dest_col = dest_x + x as i32;
                if dest_col < 0 || dest_col >= self.mode.width as i32 {
                    continue;
                }
                
                let src_offset = (y * src_width + x) as usize * 4;
                if src_offset + 4 > src.len() {
                    continue;
                }
                
                let src_b = src[src_offset] as u32;
                let src_g = src[src_offset + 1] as u32;
                let src_r = src[src_offset + 2] as u32;
                let src_a = src[src_offset + 3] as u32;
                
                if src_a == 0 {
                    continue; // Fully transparent
                }
                
                let dest_offset = dest_row as usize * self.mode.pitch as usize 
                    + dest_col as usize * 4;
                let buffer = self.buffer_mut();
                
                if src_a == 255 {
                    // Fully opaque
                    buffer[dest_offset..dest_offset + 4]
                        .copy_from_slice(&src[src_offset..src_offset + 4]);
                } else {
                    // Alpha blend
                    let dest_b = buffer[dest_offset] as u32;
                    let dest_g = buffer[dest_offset + 1] as u32;
                    let dest_r = buffer[dest_offset + 2] as u32;
                    
                    let inv_a = 255 - src_a;
                    
                    buffer[dest_offset] = ((src_b * src_a + dest_b * inv_a) / 255) as u8;
                    buffer[dest_offset + 1] = ((src_g * src_a + dest_g * inv_a) / 255) as u8;
                    buffer[dest_offset + 2] = ((src_r * src_a + dest_r * inv_a) / 255) as u8;
                    buffer[dest_offset + 3] = 255;
                }
            }
        }
        
        self.mark_dirty(Rect::new(dest_x, dest_y, src_width, src_height));
    }

    /// Mark a region as dirty (needs updating on screen)
    pub fn mark_dirty(&mut self, rect: Rect) {
        self.dirty_rects.push(rect);
    }

    /// Get dirty regions
    pub fn dirty_rects(&self) -> &[Rect] {
        &self.dirty_rects
    }

    /// Clear dirty regions
    pub fn clear_dirty(&mut self) {
        self.dirty_rects.clear();
    }

    /// Swap buffers (for double buffering)
    pub fn swap_buffers(&mut self) {
        if let Some(ref mut back_buffer) = self.back_buffer {
            core::mem::swap(&mut self.buffer, back_buffer);
        }
        self.clear_dirty();
    }

    /// Resize the framebuffer
    pub fn resize(&mut self, mode: DisplayMode) {
        let size = mode.pitch as usize * mode.height as usize;
        
        self.buffer = alloc::vec![0u8; size];
        
        if self.double_buffered {
            self.back_buffer = Some(alloc::vec![0u8; size]);
        }
        
        self.mode = mode;
        self.dirty_rects.clear();
    }
}
