//! # Color Types and Utilities
//!
//! Provides color representation and manipulation for the graphics subsystem.

use core::fmt;

/// RGBA color (8 bits per channel)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Creates a new color with full opacity
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    
    /// Creates a new color with alpha
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    
    /// Creates a color from a 24-bit RGB value
    pub const fn from_rgb24(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as u8,
            g: ((rgb >> 8) & 0xFF) as u8,
            b: (rgb & 0xFF) as u8,
            a: 255,
        }
    }
    
    /// Creates a color from a 32-bit ARGB value
    pub const fn from_argb32(argb: u32) -> Self {
        Self {
            a: ((argb >> 24) & 0xFF) as u8,
            r: ((argb >> 16) & 0xFF) as u8,
            g: ((argb >> 8) & 0xFF) as u8,
            b: (argb & 0xFF) as u8,
        }
    }
    
    /// Converts to 32-bit ARGB
    pub const fn to_argb32(&self) -> u32 {
        ((self.a as u32) << 24) |
        ((self.r as u32) << 16) |
        ((self.g as u32) << 8) |
        (self.b as u32)
    }
    
    /// Converts to 32-bit BGRA
    pub const fn to_bgra32(&self) -> u32 {
        ((self.a as u32) << 24) |
        ((self.b as u32) << 16) |
        ((self.g as u32) << 8) |
        (self.r as u32)
    }
    
    /// Converts to 24-bit RGB
    pub const fn to_rgb24(&self) -> u32 {
        ((self.r as u32) << 16) |
        ((self.g as u32) << 8) |
        (self.b as u32)
    }
    
    /// Converts to 16-bit RGB565
    pub const fn to_rgb565(&self) -> u16 {
        (((self.r as u16) & 0xF8) << 8) |
        (((self.g as u16) & 0xFC) << 3) |
        ((self.b as u16) >> 3)
    }
    
    /// Creates from 16-bit RGB565
    pub const fn from_rgb565(rgb565: u16) -> Self {
        Self {
            r: ((rgb565 >> 8) & 0xF8) as u8,
            g: ((rgb565 >> 3) & 0xFC) as u8,
            b: ((rgb565 << 3) & 0xF8) as u8,
            a: 255,
        }
    }
    
    /// Blends this color over another using alpha compositing
    pub fn blend_over(&self, bg: Color) -> Color {
        if self.a == 255 {
            return *self;
        }
        if self.a == 0 {
            return bg;
        }
        
        let alpha = self.a as u32;
        let inv_alpha = 255 - alpha;
        
        Color {
            r: ((self.r as u32 * alpha + bg.r as u32 * inv_alpha) / 255) as u8,
            g: ((self.g as u32 * alpha + bg.g as u32 * inv_alpha) / 255) as u8,
            b: ((self.b as u32 * alpha + bg.b as u32 * inv_alpha) / 255) as u8,
            a: 255,
        }
    }
    
    /// Interpolates between two colors
    pub fn lerp(&self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let inv_t = 1.0 - t;
        
        Color {
            r: (self.r as f32 * inv_t + other.r as f32 * t) as u8,
            g: (self.g as f32 * inv_t + other.g as f32 * t) as u8,
            b: (self.b as f32 * inv_t + other.b as f32 * t) as u8,
            a: (self.a as f32 * inv_t + other.a as f32 * t) as u8,
        }
    }
    
    /// Returns a darkened version of this color
    pub fn darken(&self, amount: f32) -> Color {
        let factor = (1.0 - amount).clamp(0.0, 1.0);
        Color {
            r: (self.r as f32 * factor) as u8,
            g: (self.g as f32 * factor) as u8,
            b: (self.b as f32 * factor) as u8,
            a: self.a,
        }
    }
    
    /// Returns a lightened version of this color
    pub fn lighten(&self, amount: f32) -> Color {
        let factor = amount.clamp(0.0, 1.0);
        Color {
            r: (self.r as f32 + (255.0 - self.r as f32) * factor) as u8,
            g: (self.g as f32 + (255.0 - self.g as f32) * factor) as u8,
            b: (self.b as f32 + (255.0 - self.b as f32) * factor) as u8,
            a: self.a,
        }
    }
    
    // Common colors
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);
    pub const ORANGE: Color = Color::rgb(255, 165, 0);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    
    // Splax theme colors
    pub const SPLAX_BG: Color = Color::rgb(30, 30, 46);
    pub const SPLAX_FG: Color = Color::rgb(205, 214, 244);
    pub const SPLAX_ACCENT: Color = Color::rgb(137, 180, 250);
    pub const SPLAX_SUCCESS: Color = Color::rgb(166, 227, 161);
    pub const SPLAX_WARNING: Color = Color::rgb(249, 226, 175);
    pub const SPLAX_ERROR: Color = Color::rgb(243, 139, 168);
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }
}

/// Color palette for indexed color modes
#[derive(Clone)]
pub struct Palette {
    colors: [Color; 256],
}

impl Palette {
    /// Creates a new palette with all black colors
    pub const fn new() -> Self {
        Self {
            colors: [Color::BLACK; 256],
        }
    }
    
    /// Creates the standard VGA 16-color palette
    pub fn vga16() -> Self {
        let mut palette = Self::new();
        palette.colors[0] = Color::rgb(0, 0, 0);       // Black
        palette.colors[1] = Color::rgb(0, 0, 170);     // Blue
        palette.colors[2] = Color::rgb(0, 170, 0);     // Green
        palette.colors[3] = Color::rgb(0, 170, 170);   // Cyan
        palette.colors[4] = Color::rgb(170, 0, 0);     // Red
        palette.colors[5] = Color::rgb(170, 0, 170);   // Magenta
        palette.colors[6] = Color::rgb(170, 85, 0);    // Brown
        palette.colors[7] = Color::rgb(170, 170, 170); // Light Gray
        palette.colors[8] = Color::rgb(85, 85, 85);    // Dark Gray
        palette.colors[9] = Color::rgb(85, 85, 255);   // Light Blue
        palette.colors[10] = Color::rgb(85, 255, 85);  // Light Green
        palette.colors[11] = Color::rgb(85, 255, 255); // Light Cyan
        palette.colors[12] = Color::rgb(255, 85, 85);  // Light Red
        palette.colors[13] = Color::rgb(255, 85, 255); // Light Magenta
        palette.colors[14] = Color::rgb(255, 255, 85); // Yellow
        palette.colors[15] = Color::rgb(255, 255, 255);// White
        palette
    }
    
    /// Gets a color from the palette
    pub fn get(&self, index: u8) -> Color {
        self.colors[index as usize]
    }
    
    /// Sets a color in the palette
    pub fn set(&mut self, index: u8, color: Color) {
        self.colors[index as usize] = color;
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::vga16()
    }
}
