//! 2D renderer for S-GPU service

use alloc::vec::Vec;
use alloc::boxed::Box;
use super::{Color, Rect, Point, GpuError, framebuffer::Framebuffer};

/// Integer square root (for no_std environment)
fn isqrt(n: i32) -> i32 {
    if n < 0 {
        return 0;
    }
    if n < 2 {
        return n;
    }
    
    let mut x = n;
    let mut y = (x + 1) / 2;
    
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    
    x
}

/// Blend mode for drawing operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// No blending, overwrite destination
    None,
    /// Standard alpha blending
    Alpha,
    /// Additive blending
    Additive,
    /// Multiply blending
    Multiply,
}

/// Texture/Image data
#[derive(Debug, Clone)]
pub struct Texture {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Bits per pixel (8, 16, 24, 32)
    pub bpp: u8,
    /// Pixel data
    pub data: Vec<u8>,
}

impl Texture {
    /// Create a new texture
    pub fn new(width: u32, height: u32, bpp: u8) -> Self {
        let size = width as usize * height as usize * (bpp as usize / 8);
        Self {
            width,
            height,
            bpp,
            data: alloc::vec![0u8; size],
        }
    }

    /// Create texture from raw data
    pub fn from_data(width: u32, height: u32, bpp: u8, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            bpp,
            data,
        }
    }

    /// Get bytes per pixel
    pub fn bytes_per_pixel(&self) -> usize {
        (self.bpp / 8) as usize
    }

    /// Get pixel color at position
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y * self.width + x) as usize * self.bytes_per_pixel();

        match self.bpp {
            32 => {
                if offset + 4 <= self.data.len() {
                    Some(Color::rgba(
                        self.data[offset + 2],
                        self.data[offset + 1],
                        self.data[offset],
                        self.data[offset + 3],
                    ))
                } else {
                    None
                }
            }
            24 => {
                if offset + 3 <= self.data.len() {
                    Some(Color::rgb(
                        self.data[offset + 2],
                        self.data[offset + 1],
                        self.data[offset],
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Set pixel color at position
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y * self.width + x) as usize * self.bytes_per_pixel();

        match self.bpp {
            32 => {
                if offset + 4 <= self.data.len() {
                    self.data[offset] = color.b;
                    self.data[offset + 1] = color.g;
                    self.data[offset + 2] = color.r;
                    self.data[offset + 3] = color.a;
                }
            }
            24 => {
                if offset + 3 <= self.data.len() {
                    self.data[offset] = color.b;
                    self.data[offset + 1] = color.g;
                    self.data[offset + 2] = color.r;
                }
            }
            _ => {}
        }
    }
}

/// Sprite for rendering
#[derive(Debug, Clone)]
pub struct Sprite {
    /// Texture ID
    pub texture_id: u32,
    /// Source rectangle in texture
    pub src_rect: Rect,
    /// Position on screen
    pub x: i32,
    pub y: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Scale factors
    pub scale_x: f32,
    pub scale_y: f32,
    /// Tint color (modulates texture color)
    pub tint: Color,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Z-order (higher = on top)
    pub z_order: i32,
    /// Visibility
    pub visible: bool,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            texture_id: 0,
            src_rect: Rect::new(0, 0, 0, 0),
            x: 0,
            y: 0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            tint: Color::WHITE,
            blend_mode: BlendMode::Alpha,
            z_order: 0,
            visible: true,
        }
    }
}

/// 2D renderer
pub struct Renderer {
    /// Textures storage
    textures: Vec<Option<Texture>>,
    /// Next texture ID
    next_texture_id: u32,
    /// Clip rectangle
    clip_rect: Option<Rect>,
    /// Current blend mode
    blend_mode: BlendMode,
    /// Drawing color
    draw_color: Color,
}

impl Renderer {
    /// Create a new renderer
    pub fn new() -> Self {
        Self {
            textures: Vec::new(),
            next_texture_id: 1,
            clip_rect: None,
            blend_mode: BlendMode::None,
            draw_color: Color::WHITE,
        }
    }

    /// Set clip rectangle
    pub fn set_clip(&mut self, rect: Option<Rect>) {
        self.clip_rect = rect;
    }

    /// Set blend mode
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        self.blend_mode = mode;
    }

    /// Set draw color
    pub fn set_draw_color(&mut self, color: Color) {
        self.draw_color = color;
    }

    /// Create a texture
    pub fn create_texture(&mut self, width: u32, height: u32, bpp: u8) -> u32 {
        let texture = Texture::new(width, height, bpp);
        let id = self.next_texture_id;
        self.next_texture_id += 1;

        // Find empty slot or push
        if let Some(slot) = self.textures.iter_mut().find(|t| t.is_none()) {
            *slot = Some(texture);
        } else {
            self.textures.push(Some(texture));
        }

        id
    }

    /// Load texture data
    pub fn load_texture(&mut self, id: u32, data: &[u8]) -> Result<(), GpuError> {
        let texture = self.get_texture_mut(id).ok_or(GpuError::InvalidMode)?;
        
        if data.len() <= texture.data.len() {
            texture.data[..data.len()].copy_from_slice(data);
            Ok(())
        } else {
            Err(GpuError::OutOfMemory)
        }
    }

    /// Get texture reference
    pub fn get_texture(&self, id: u32) -> Option<&Texture> {
        if id == 0 || id as usize > self.textures.len() {
            return None;
        }
        self.textures.get(id as usize - 1)?.as_ref()
    }

    /// Get mutable texture reference
    pub fn get_texture_mut(&mut self, id: u32) -> Option<&mut Texture> {
        if id == 0 || id as usize > self.textures.len() {
            return None;
        }
        self.textures.get_mut(id as usize - 1)?.as_mut()
    }

    /// Delete a texture
    pub fn delete_texture(&mut self, id: u32) {
        if id > 0 && (id as usize) <= self.textures.len() {
            self.textures[id as usize - 1] = None;
        }
    }

    /// Clear framebuffer
    pub fn clear(&self, fb: &mut Framebuffer) {
        fb.clear(self.draw_color);
    }

    /// Draw point
    pub fn draw_point(&self, fb: &mut Framebuffer, x: i32, y: i32) {
        if self.in_clip(x, y) {
            fb.set_pixel(x, y, self.draw_color);
        }
    }

    /// Draw line
    pub fn draw_line(&self, fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32) {
        // Bresenham's line algorithm
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let mut x = x0;
        let mut y = y0;

        loop {
            if self.in_clip(x, y) {
                self.blend_pixel(fb, x, y, self.draw_color);
            }

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
    }

    /// Draw rectangle outline
    pub fn draw_rect(&self, fb: &mut Framebuffer, rect: Rect) {
        let x0 = rect.x;
        let y0 = rect.y;
        let x1 = rect.x + rect.width as i32 - 1;
        let y1 = rect.y + rect.height as i32 - 1;

        self.draw_line(fb, x0, y0, x1, y0); // Top
        self.draw_line(fb, x1, y0, x1, y1); // Right
        self.draw_line(fb, x1, y1, x0, y1); // Bottom
        self.draw_line(fb, x0, y1, x0, y0); // Left
    }

    /// Fill rectangle
    pub fn fill_rect(&self, fb: &mut Framebuffer, rect: Rect) {
        for y in rect.y..(rect.y + rect.height as i32) {
            for x in rect.x..(rect.x + rect.width as i32) {
                if self.in_clip(x, y) {
                    self.blend_pixel(fb, x, y, self.draw_color);
                }
            }
        }
    }

    /// Draw circle outline
    pub fn draw_circle(&self, fb: &mut Framebuffer, cx: i32, cy: i32, radius: i32) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;

        while x >= y {
            self.draw_circle_points(fb, cx, cy, x, y);
            y += 1;
            if err <= 0 {
                err += 2 * y + 1;
            }
            if err > 0 {
                x -= 1;
                err -= 2 * x + 1;
            }
        }
    }

    /// Draw the 8 symmetrical points of a circle
    fn draw_circle_points(&self, fb: &mut Framebuffer, cx: i32, cy: i32, x: i32, y: i32) {
        let points = [
            (cx + x, cy + y),
            (cx + y, cy + x),
            (cx - y, cy + x),
            (cx - x, cy + y),
            (cx - x, cy - y),
            (cx - y, cy - x),
            (cx + y, cy - x),
            (cx + x, cy - y),
        ];

        for (px, py) in points {
            if self.in_clip(px, py) {
                self.blend_pixel(fb, px, py, self.draw_color);
            }
        }
    }

    /// Fill circle
    pub fn fill_circle(&self, fb: &mut Framebuffer, cx: i32, cy: i32, radius: i32) {
        for y in -radius..=radius {
            let width = isqrt(radius * radius - y * y);
            for x in -width..=width {
                let px = cx + x;
                let py = cy + y;
                if self.in_clip(px, py) {
                    self.blend_pixel(fb, px, py, self.draw_color);
                }
            }
        }
    }

    /// Draw a triangle outline
    pub fn draw_triangle(
        &self,
        fb: &mut Framebuffer,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    ) {
        self.draw_line(fb, x0, y0, x1, y1);
        self.draw_line(fb, x1, y1, x2, y2);
        self.draw_line(fb, x2, y2, x0, y0);
    }

    /// Fill a triangle using scanline algorithm
    pub fn fill_triangle(
        &self,
        fb: &mut Framebuffer,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
    ) {
        // Sort vertices by y-coordinate
        let mut vertices = [(x0, y0), (x1, y1), (x2, y2)];
        vertices.sort_by_key(|v| v.1);

        let (x0, y0) = vertices[0];
        let (x1, y1) = vertices[1];
        let (x2, y2) = vertices[2];

        // Calculate slopes
        let dx01 = if y1 != y0 {
            (x1 - x0) as f32 / (y1 - y0) as f32
        } else {
            0.0
        };
        let dx02 = if y2 != y0 {
            (x2 - x0) as f32 / (y2 - y0) as f32
        } else {
            0.0
        };
        let dx12 = if y2 != y1 {
            (x2 - x1) as f32 / (y2 - y1) as f32
        } else {
            0.0
        };

        // Rasterize
        let mut sx0 = x0 as f32;
        let mut sx1 = x0 as f32;

        for y in y0..y2 {
            if y < y1 {
                let left = sx0.min(sx1) as i32;
                let right = sx0.max(sx1) as i32;
                for x in left..=right {
                    if self.in_clip(x, y) {
                        self.blend_pixel(fb, x, y, self.draw_color);
                    }
                }
                sx0 += dx02;
                sx1 += dx01;
            } else {
                let left = sx0.min(sx1) as i32;
                let right = sx0.max(sx1) as i32;
                for x in left..=right {
                    if self.in_clip(x, y) {
                        self.blend_pixel(fb, x, y, self.draw_color);
                    }
                }
                sx0 += dx02;
                sx1 += dx12;
            }
        }
    }

    /// Draw a texture
    pub fn draw_texture(
        &self,
        fb: &mut Framebuffer,
        texture: &Texture,
        x: i32,
        y: i32,
    ) {
        for ty in 0..texture.height {
            for tx in 0..texture.width {
                if let Some(color) = texture.get_pixel(tx, ty) {
                    let px = x + tx as i32;
                    let py = y + ty as i32;
                    if self.in_clip(px, py) {
                        self.blend_pixel(fb, px, py, color);
                    }
                }
            }
        }
    }

    /// Draw a portion of a texture
    pub fn draw_texture_rect(
        &self,
        fb: &mut Framebuffer,
        texture: &Texture,
        src: Rect,
        dest_x: i32,
        dest_y: i32,
    ) {
        for ty in 0..src.height {
            for tx in 0..src.width {
                let tex_x = (src.x + tx as i32) as u32;
                let tex_y = (src.y + ty as i32) as u32;
                if let Some(color) = texture.get_pixel(tex_x, tex_y) {
                    let px = dest_x + tx as i32;
                    let py = dest_y + ty as i32;
                    if self.in_clip(px, py) {
                        self.blend_pixel(fb, px, py, color);
                    }
                }
            }
        }
    }

    /// Draw sprite
    pub fn draw_sprite(&self, fb: &mut Framebuffer, sprite: &Sprite) {
        if !sprite.visible {
            return;
        }

        if let Some(texture) = self.get_texture(sprite.texture_id) {
            // Simple case: no rotation or scaling
            if sprite.rotation == 0.0 && sprite.scale_x == 1.0 && sprite.scale_y == 1.0 {
                self.draw_texture_rect(fb, texture, sprite.src_rect, sprite.x, sprite.y);
            } else {
                // TODO: Implement rotation and scaling
                self.draw_texture_rect(fb, texture, sprite.src_rect, sprite.x, sprite.y);
            }
        }
    }

    /// Check if point is within clip rectangle
    fn in_clip(&self, x: i32, y: i32) -> bool {
        if let Some(clip) = self.clip_rect {
            x >= clip.x
                && x < clip.x + clip.width as i32
                && y >= clip.y
                && y < clip.y + clip.height as i32
        } else {
            true
        }
    }

    /// Blend a pixel with the destination according to blend mode
    fn blend_pixel(&self, fb: &mut Framebuffer, x: i32, y: i32, color: Color) {
        match self.blend_mode {
            BlendMode::None => {
                fb.set_pixel(x, y, color);
            }
            BlendMode::Alpha => {
                if color.a == 255 {
                    fb.set_pixel(x, y, color);
                } else if color.a > 0 {
                    if let Some(dest) = fb.get_pixel(x, y) {
                        let src_a = color.a as u32;
                        let inv_a = 255 - src_a;

                        let r = ((color.r as u32 * src_a + dest.r as u32 * inv_a) / 255) as u8;
                        let g = ((color.g as u32 * src_a + dest.g as u32 * inv_a) / 255) as u8;
                        let b = ((color.b as u32 * src_a + dest.b as u32 * inv_a) / 255) as u8;

                        fb.set_pixel(x, y, Color::rgb(r, g, b));
                    }
                }
            }
            BlendMode::Additive => {
                if let Some(dest) = fb.get_pixel(x, y) {
                    let r = (dest.r as u16 + color.r as u16).min(255) as u8;
                    let g = (dest.g as u16 + color.g as u16).min(255) as u8;
                    let b = (dest.b as u16 + color.b as u16).min(255) as u8;
                    fb.set_pixel(x, y, Color::rgb(r, g, b));
                }
            }
            BlendMode::Multiply => {
                if let Some(dest) = fb.get_pixel(x, y) {
                    let r = ((dest.r as u16 * color.r as u16) / 255) as u8;
                    let g = ((dest.g as u16 * color.g as u16) / 255) as u8;
                    let b = ((dest.b as u16 * color.b as u16) / 255) as u8;
                    fb.set_pixel(x, y, Color::rgb(r, g, b));
                }
            }
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
