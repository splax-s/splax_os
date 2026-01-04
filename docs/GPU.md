# GPU and Graphics Subsystem

> Comprehensive documentation for Splax OS graphics and display functionality.

## Overview

The Splax OS GPU subsystem provides framebuffer-based graphics, text console rendering, GPU acceleration (Intel/AMD), and a Wayland-compatible compositor. It supports UEFI GOP (Graphics Output Protocol) framebuffers, native GPU drivers, and VGA text mode for maximum compatibility.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Applications                               │
│                  (GUI, terminal, games)                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Wayland Compositor                            │
│                (kernel/src/gpu/wayland.rs)                      │
│   XDG Shell, SHM buffers, DMA-BUF, input handling               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Graphics Console                             │
│                 (kernel/src/gpu/console.rs)                     │
│         Text rendering, scrolling, cursor                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Drawing Primitives                            │
│                (kernel/src/gpu/primitives.rs)                   │
│      Lines, rectangles, circles, fills, blits                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Framebuffer                                 │
│                (kernel/src/gpu/framebuffer.rs)                  │
│           Direct pixel access, double buffering                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────┬─────────────────────────┬───────────────────────┐
│  Intel GPU    │      AMD GPU            │    VirtIO GPU         │
│ (intel.rs)    │     (amd.rs)            │   (virtio.rs)         │
│  Gen9+, CET   │  RDNA/RDNA2/RDNA3       │   Para-virtualized    │
└───────────────┴─────────────────────────┴───────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Hardware Layer                               │
│      Intel iGPU / AMD dGPU / UEFI GOP / VBE / VGA Text          │
└─────────────────────────────────────────────────────────────────┘
```

## GPU Drivers

### Intel Integrated Graphics (Gen9+)

**Location:** `kernel/src/gpu/intel.rs`

Supports Intel HD Graphics, UHD Graphics, and Iris series:

- **Ring Buffer**: Command submission via ELSP
- **GTT Management**: Graphics Translation Table for GPU address space
- **Display Pipes**: Multi-monitor support with HDMI, DP, eDP
- **Power States**: D0-D3 power management

### AMD GPU (RDNA/RDNA2/RDNA3)

**Location:** `kernel/src/gpu/amd.rs`

Supports Radeon RX 5000/6000/7000 series:

- **SDMA Ring**: System DMA for buffer copies
- **GART**: Graphics Address Remapping Table
- **DCN Display**: Display Core Next controller
- **Multi-monitor**: Up to 4 simultaneous displays

### Wayland Compositor

**Location:** `kernel/src/gpu/wayland.rs`

Full Wayland protocol implementation:

- **XDG Shell**: Desktop window management
- **SHM Buffers**: CPU-rendered client buffers
- **DMA-BUF**: Zero-copy GPU buffer sharing
- **Input Handling**: Keyboard, pointer, touch events
- **Subsurfaces**: Window composition hierarchy

---

## Framebuffer

### Framebuffer Structure

```rust
// kernel/src/gpu/framebuffer.rs

pub struct Framebuffer {
    /// Base address of framebuffer memory
    base: *mut u8,
    /// Width in pixels
    width: u32,
    /// Height in pixels
    height: u32,
    /// Bytes per scanline (may include padding)
    stride: u32,
    /// Bits per pixel (typically 32)
    bpp: u8,
    /// Pixel format
    format: PixelFormat,
    /// Double buffer (if enabled)
    back_buffer: Option<Vec<u32>>,
}

#[derive(Clone, Copy)]
pub enum PixelFormat {
    /// BGRA (common for UEFI GOP)
    Bgra32,
    /// RGBA
    Rgba32,
    /// RGB565 (16-bit)
    Rgb565,
    /// 8-bit indexed color
    Indexed8,
}
```

### Initialization

In **monolithic mode** (default), the kernel initializes GPU directly during boot.

In **microkernel mode**, GPU initialization is conditional:
- Without `monolithic_gpu` feature: S-GPU service handles graphics
- With `monolithic_gpu` feature: Kernel initializes framebuffer directly

```rust
// In kernel/src/lib.rs
#[cfg(any(not(feature = "microkernel"), feature = "monolithic_gpu"))]
gpu::init();
```

```rust
impl Framebuffer {
    /// Create framebuffer from bootloader info
    pub fn from_bootinfo(info: &FramebufferInfo) -> Self {
        Self {
            base: info.address as *mut u8,
            width: info.width,
            height: info.height,
            stride: info.pitch,
            bpp: info.bpp,
            format: PixelFormat::from_masks(
                info.red_mask,
                info.green_mask, 
                info.blue_mask
            ),
            back_buffer: None,
        }
    }
    
    /// Enable double buffering
    pub fn enable_double_buffer(&mut self) {
        let size = (self.width * self.height) as usize;
        self.back_buffer = Some(vec![0u32; size]);
    }
}
```

### Pixel Operations

```rust
impl Framebuffer {
    /// Set a single pixel
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        
        let offset = (y * self.stride + x * 4) as usize;
        let pixel = color.to_format(self.format);
        
        unsafe {
            let ptr = self.base.add(offset) as *mut u32;
            ptr.write_volatile(pixel);
        }
    }
    
    /// Get a pixel
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::BLACK;
        }
        
        let offset = (y * self.stride + x * 4) as usize;
        
        unsafe {
            let ptr = self.base.add(offset) as *const u32;
            Color::from_format(ptr.read_volatile(), self.format)
        }
    }
    
    /// Clear entire framebuffer
    pub fn clear(&mut self, color: Color) {
        let pixel = color.to_format(self.format);
        let size = (self.height * self.stride) as usize / 4;
        
        unsafe {
            let ptr = self.base as *mut u32;
            for i in 0..size {
                ptr.add(i).write_volatile(pixel);
            }
        }
    }
    
    /// Swap buffers (if double buffering enabled)
    pub fn swap_buffers(&mut self) {
        if let Some(ref back) = self.back_buffer {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    back.as_ptr(),
                    self.base as *mut u32,
                    back.len()
                );
            }
        }
    }
}
```

---

## Color System

### Color Type

```rust
// kernel/src/gpu/color.rs

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create color from RGB values
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    
    /// Create color from RGBA values
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    
    /// Create from 32-bit packed value (0xAARRGGBB)
    pub const fn from_u32(value: u32) -> Self {
        Self {
            a: ((value >> 24) & 0xFF) as u8,
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }
    
    /// Convert to packed format for framebuffer
    pub fn to_format(&self, format: PixelFormat) -> u32 {
        match format {
            PixelFormat::Bgra32 => {
                (self.a as u32) << 24 |
                (self.r as u32) << 16 |
                (self.g as u32) << 8 |
                (self.b as u32)
            }
            PixelFormat::Rgba32 => {
                (self.a as u32) << 24 |
                (self.b as u32) << 16 |
                (self.g as u32) << 8 |
                (self.r as u32)
            }
            PixelFormat::Rgb565 => {
                ((self.r as u32 >> 3) << 11) |
                ((self.g as u32 >> 2) << 5) |
                (self.b as u32 >> 3)
            }
            PixelFormat::Indexed8 => {
                // Approximate to 8-bit palette
                ((self.r as u32 >> 5) << 5) |
                ((self.g as u32 >> 5) << 2) |
                (self.b as u32 >> 6)
            }
        }
    }
    
    /// Blend two colors (alpha compositing)
    pub fn blend(&self, other: Color) -> Color {
        let alpha = other.a as u32;
        let inv_alpha = 255 - alpha;
        
        Color {
            r: ((self.r as u32 * inv_alpha + other.r as u32 * alpha) / 255) as u8,
            g: ((self.g as u32 * inv_alpha + other.g as u32 * alpha) / 255) as u8,
            b: ((self.b as u32 * inv_alpha + other.b as u32 * alpha) / 255) as u8,
            a: 255,
        }
    }
}
```

### Predefined Colors

```rust
impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const ORANGE: Color = Color::rgb(255, 165, 0);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);
    
    // Terminal colors (ANSI)
    pub const ANSI_BLACK: Color = Color::rgb(0, 0, 0);
    pub const ANSI_RED: Color = Color::rgb(205, 49, 49);
    pub const ANSI_GREEN: Color = Color::rgb(13, 188, 121);
    pub const ANSI_YELLOW: Color = Color::rgb(229, 229, 16);
    pub const ANSI_BLUE: Color = Color::rgb(36, 114, 200);
    pub const ANSI_MAGENTA: Color = Color::rgb(188, 63, 188);
    pub const ANSI_CYAN: Color = Color::rgb(17, 168, 205);
    pub const ANSI_WHITE: Color = Color::rgb(229, 229, 229);
}
```

---

## Drawing Primitives

### Primitives Module

```rust
// kernel/src/gpu/primitives.rs

pub struct Graphics<'a> {
    fb: &'a mut Framebuffer,
}

impl<'a> Graphics<'a> {
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self { fb }
    }
}
```

### Line Drawing

```rust
impl<'a> Graphics<'a> {
    /// Draw line using Bresenham's algorithm
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        
        let mut x = x0;
        let mut y = y0;
        
        loop {
            self.fb.set_pixel(x as u32, y as u32, color);
            
            if x == x1 && y == y1 {
                break;
            }
            
            let e2 = 2 * err;
            if e2 >= dy {
                if x == x1 { break; }
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                if y == y1 { break; }
                err += dx;
                y += sy;
            }
        }
    }
    
    /// Draw horizontal line (optimized)
    pub fn draw_hline(&mut self, x: u32, y: u32, width: u32, color: Color) {
        for i in 0..width {
            self.fb.set_pixel(x + i, y, color);
        }
    }
    
    /// Draw vertical line (optimized)
    pub fn draw_vline(&mut self, x: u32, y: u32, height: u32, color: Color) {
        for i in 0..height {
            self.fb.set_pixel(x, y + i, color);
        }
    }
}
```

### Rectangle Drawing

```rust
impl<'a> Graphics<'a> {
    /// Draw rectangle outline
    pub fn draw_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        self.draw_hline(x, y, w, color);           // Top
        self.draw_hline(x, y + h - 1, w, color);   // Bottom
        self.draw_vline(x, y, h, color);           // Left
        self.draw_vline(x + w - 1, y, h, color);   // Right
    }
    
    /// Draw filled rectangle
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        for row in y..(y + h) {
            for col in x..(x + w) {
                self.fb.set_pixel(col, row, color);
            }
        }
    }
    
    /// Draw rounded rectangle
    pub fn draw_rounded_rect(
        &mut self,
        x: u32, y: u32, w: u32, h: u32,
        radius: u32,
        color: Color
    ) {
        // Draw corners as arcs
        self.draw_arc(x + radius, y + radius, radius, 180, 270, color);
        self.draw_arc(x + w - radius - 1, y + radius, radius, 270, 360, color);
        self.draw_arc(x + radius, y + h - radius - 1, radius, 90, 180, color);
        self.draw_arc(x + w - radius - 1, y + h - radius - 1, radius, 0, 90, color);
        
        // Draw straight edges
        self.draw_hline(x + radius, y, w - 2 * radius, color);
        self.draw_hline(x + radius, y + h - 1, w - 2 * radius, color);
        self.draw_vline(x, y + radius, h - 2 * radius, color);
        self.draw_vline(x + w - 1, y + radius, h - 2 * radius, color);
    }
}
```

### Circle Drawing

```rust
impl<'a> Graphics<'a> {
    /// Draw circle outline using midpoint algorithm
    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;
        
        while x >= y {
            self.fb.set_pixel((cx + x) as u32, (cy + y) as u32, color);
            self.fb.set_pixel((cx + y) as u32, (cy + x) as u32, color);
            self.fb.set_pixel((cx - y) as u32, (cy + x) as u32, color);
            self.fb.set_pixel((cx - x) as u32, (cy + y) as u32, color);
            self.fb.set_pixel((cx - x) as u32, (cy - y) as u32, color);
            self.fb.set_pixel((cx - y) as u32, (cy - x) as u32, color);
            self.fb.set_pixel((cx + y) as u32, (cy - x) as u32, color);
            self.fb.set_pixel((cx + x) as u32, (cy - y) as u32, color);
            
            y += 1;
            err += 1 + 2 * y;
            if 2 * (err - x) + 1 > 0 {
                x -= 1;
                err += 1 - 2 * x;
            }
        }
    }
    
    /// Draw filled circle
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;
        
        while x >= y {
            self.draw_hline((cx - x) as u32, (cy + y) as u32, (2 * x) as u32, color);
            self.draw_hline((cx - x) as u32, (cy - y) as u32, (2 * x) as u32, color);
            self.draw_hline((cx - y) as u32, (cy + x) as u32, (2 * y) as u32, color);
            self.draw_hline((cx - y) as u32, (cy - x) as u32, (2 * y) as u32, color);
            
            y += 1;
            err += 1 + 2 * y;
            if 2 * (err - x) + 1 > 0 {
                x -= 1;
                err += 1 - 2 * x;
            }
        }
    }
}
```

### Bitmap Blitting

```rust
impl<'a> Graphics<'a> {
    /// Blit raw pixel data to framebuffer
    pub fn blit(
        &mut self,
        x: u32, y: u32,
        width: u32, height: u32,
        data: &[u32],
    ) {
        for row in 0..height {
            for col in 0..width {
                let pixel = data[(row * width + col) as usize];
                let color = Color::from_u32(pixel);
                self.fb.set_pixel(x + col, y + row, color);
            }
        }
    }
    
    /// Blit with transparency (alpha blending)
    pub fn blit_transparent(
        &mut self,
        x: u32, y: u32,
        width: u32, height: u32,
        data: &[u32],
    ) {
        for row in 0..height {
            for col in 0..width {
                let pixel = data[(row * width + col) as usize];
                let src_color = Color::from_u32(pixel);
                
                if src_color.a == 0 {
                    continue;  // Fully transparent
                } else if src_color.a == 255 {
                    self.fb.set_pixel(x + col, y + row, src_color);
                } else {
                    let dst_color = self.fb.get_pixel(x + col, y + row);
                    let blended = dst_color.blend(src_color);
                    self.fb.set_pixel(x + col, y + row, blended);
                }
            }
        }
    }
}
```

---

## Font System

### Built-in Font

```rust
// kernel/src/gpu/font.rs

/// 8x16 bitmap font (ASCII 32-126)
pub struct BitmapFont {
    /// Glyph width in pixels
    pub width: u8,
    /// Glyph height in pixels
    pub height: u8,
    /// Bitmap data (1 bit per pixel, row-major)
    pub glyphs: &'static [[u8; 16]; 95],
}

impl BitmapFont {
    /// Get built-in 8x16 VGA font
    pub const fn vga_font() -> Self {
        Self {
            width: 8,
            height: 16,
            glyphs: &VGA_FONT_8X16,
        }
    }
    
    /// Get glyph bitmap for character
    pub fn get_glyph(&self, c: char) -> Option<&[u8; 16]> {
        if c < ' ' || c > '~' {
            return None;
        }
        let index = (c as usize) - 32;
        self.glyphs.get(index)
    }
}
```

### Text Rendering

```rust
impl<'a> Graphics<'a> {
    /// Draw a single character
    pub fn draw_char(
        &mut self,
        x: u32, y: u32,
        c: char,
        font: &BitmapFont,
        fg: Color,
        bg: Option<Color>,
    ) {
        let glyph = match font.get_glyph(c) {
            Some(g) => g,
            None => return,
        };
        
        for row in 0..font.height as u32 {
            let bits = glyph[row as usize];
            for col in 0..font.width as u32 {
                let bit = (bits >> (7 - col)) & 1;
                let color = if bit == 1 {
                    fg
                } else if let Some(bg_color) = bg {
                    bg_color
                } else {
                    continue;  // Transparent background
                };
                self.fb.set_pixel(x + col, y + row, color);
            }
        }
    }
    
    /// Draw a string
    pub fn draw_string(
        &mut self,
        x: u32, y: u32,
        s: &str,
        font: &BitmapFont,
        fg: Color,
        bg: Option<Color>,
    ) {
        let mut cursor_x = x;
        
        for c in s.chars() {
            if c == '\n' {
                // Newline not handled here, use Console for multi-line
                continue;
            }
            
            self.draw_char(cursor_x, y, c, font, fg, bg);
            cursor_x += font.width as u32;
        }
    }
}
```

---

## Graphics Console

### Console Structure

```rust
// kernel/src/gpu/console.rs

pub struct Console {
    /// Underlying framebuffer
    fb: Framebuffer,
    /// Font for text rendering
    font: BitmapFont,
    /// Current cursor column
    cursor_x: u32,
    /// Current cursor row
    cursor_y: u32,
    /// Console width in characters
    cols: u32,
    /// Console height in characters
    rows: u32,
    /// Foreground color
    fg_color: Color,
    /// Background color
    bg_color: Color,
    /// Character buffer for scrollback
    buffer: Vec<Vec<char>>,
    /// Whether cursor is visible
    cursor_visible: bool,
}

impl Console {
    pub fn new(fb: Framebuffer) -> Self {
        let font = BitmapFont::vga_font();
        let cols = fb.width / font.width as u32;
        let rows = fb.height / font.height as u32;
        
        Self {
            fb,
            font,
            cursor_x: 0,
            cursor_y: 0,
            cols,
            rows,
            fg_color: Color::WHITE,
            bg_color: Color::BLACK,
            buffer: vec![vec![' '; cols as usize]; rows as usize],
            cursor_visible: true,
        }
    }
}
```

### Console Operations

```rust
impl Console {
    /// Write a character
    pub fn putchar(&mut self, c: char) {
        match c {
            '\n' => {
                self.cursor_x = 0;
                self.newline();
            }
            '\r' => {
                self.cursor_x = 0;
            }
            '\t' => {
                let spaces = 4 - (self.cursor_x % 4);
                for _ in 0..spaces {
                    self.putchar(' ');
                }
            }
            '\x08' => {  // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    self.putchar_at(self.cursor_x, self.cursor_y, ' ');
                }
            }
            c if c >= ' ' => {
                self.putchar_at(self.cursor_x, self.cursor_y, c);
                self.cursor_x += 1;
                
                if self.cursor_x >= self.cols {
                    self.cursor_x = 0;
                    self.newline();
                }
            }
            _ => {}
        }
    }
    
    /// Write character at specific position
    fn putchar_at(&mut self, x: u32, y: u32, c: char) {
        self.buffer[y as usize][x as usize] = c;
        
        let px = x * self.font.width as u32;
        let py = y * self.font.height as u32;
        
        let mut gfx = Graphics::new(&mut self.fb);
        gfx.draw_char(px, py, c, &self.font, self.fg_color, Some(self.bg_color));
    }
    
    /// Move to new line, scrolling if needed
    fn newline(&mut self) {
        self.cursor_y += 1;
        
        if self.cursor_y >= self.rows {
            self.scroll_up();
            self.cursor_y = self.rows - 1;
        }
    }
    
    /// Scroll console up by one line
    fn scroll_up(&mut self) {
        // Shift buffer up
        for y in 1..self.rows as usize {
            self.buffer.swap(y - 1, y);
        }
        
        // Clear last line
        let last = self.rows as usize - 1;
        for x in 0..self.cols as usize {
            self.buffer[last][x] = ' ';
        }
        
        // Redraw entire screen
        self.redraw();
    }
    
    /// Redraw entire console from buffer
    fn redraw(&mut self) {
        let mut gfx = Graphics::new(&mut self.fb);
        
        for y in 0..self.rows {
            for x in 0..self.cols {
                let c = self.buffer[y as usize][x as usize];
                let px = x * self.font.width as u32;
                let py = y * self.font.height as u32;
                gfx.draw_char(px, py, c, &self.font, self.fg_color, Some(self.bg_color));
            }
        }
    }
    
    /// Write string
    pub fn puts(&mut self, s: &str) {
        for c in s.chars() {
            self.putchar(c);
        }
    }
    
    /// Clear console
    pub fn clear(&mut self) {
        self.fb.clear(self.bg_color);
        self.cursor_x = 0;
        self.cursor_y = 0;
        
        for row in &mut self.buffer {
            for cell in row.iter_mut() {
                *cell = ' ';
            }
        }
    }
    
    /// Set foreground color
    pub fn set_fg(&mut self, color: Color) {
        self.fg_color = color;
    }
    
    /// Set background color
    pub fn set_bg(&mut self, color: Color) {
        self.bg_color = color;
    }
}
```

### ANSI Escape Sequence Support

```rust
impl Console {
    /// Process ANSI escape sequences
    pub fn process_ansi(&mut self, seq: &str) {
        if seq.starts_with("\x1b[") {
            let params = &seq[2..seq.len()-1];
            let cmd = seq.chars().last().unwrap();
            
            match cmd {
                'm' => self.process_sgr(params),  // Select Graphic Rendition
                'H' => self.process_cursor_pos(params),
                'J' => self.process_erase(params),
                'K' => self.process_erase_line(params),
                'A' => self.cursor_up(params.parse().unwrap_or(1)),
                'B' => self.cursor_down(params.parse().unwrap_or(1)),
                'C' => self.cursor_forward(params.parse().unwrap_or(1)),
                'D' => self.cursor_back(params.parse().unwrap_or(1)),
                _ => {}
            }
        }
    }
    
    /// Process SGR (colors/attributes)
    fn process_sgr(&mut self, params: &str) {
        for param in params.split(';') {
            match param.parse::<u32>() {
                Ok(0) => {  // Reset
                    self.fg_color = Color::WHITE;
                    self.bg_color = Color::BLACK;
                }
                Ok(30) => self.fg_color = Color::ANSI_BLACK,
                Ok(31) => self.fg_color = Color::ANSI_RED,
                Ok(32) => self.fg_color = Color::ANSI_GREEN,
                Ok(33) => self.fg_color = Color::ANSI_YELLOW,
                Ok(34) => self.fg_color = Color::ANSI_BLUE,
                Ok(35) => self.fg_color = Color::ANSI_MAGENTA,
                Ok(36) => self.fg_color = Color::ANSI_CYAN,
                Ok(37) => self.fg_color = Color::ANSI_WHITE,
                Ok(40) => self.bg_color = Color::ANSI_BLACK,
                Ok(41) => self.bg_color = Color::ANSI_RED,
                Ok(42) => self.bg_color = Color::ANSI_GREEN,
                Ok(43) => self.bg_color = Color::ANSI_YELLOW,
                Ok(44) => self.bg_color = Color::ANSI_BLUE,
                Ok(45) => self.bg_color = Color::ANSI_MAGENTA,
                Ok(46) => self.bg_color = Color::ANSI_CYAN,
                Ok(47) => self.bg_color = Color::ANSI_WHITE,
                _ => {}
            }
        }
    }
}
```

---

## VGA Text Mode

### Fallback Text Mode

```rust
// kernel/src/gpu/vga.rs

const VGA_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;

pub struct VgaTextMode {
    cursor_x: usize,
    cursor_y: usize,
    color: u8,
}

impl VgaTextMode {
    pub fn new() -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
            color: 0x0F,  // White on black
        }
    }
    
    pub fn putchar(&mut self, c: u8) {
        if c == b'\n' {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y >= VGA_HEIGHT {
                self.scroll();
            }
            return;
        }
        
        let offset = self.cursor_y * VGA_WIDTH + self.cursor_x;
        let entry = (self.color as u16) << 8 | (c as u16);
        
        unsafe {
            VGA_BUFFER.add(offset).write_volatile(entry);
        }
        
        self.cursor_x += 1;
        if self.cursor_x >= VGA_WIDTH {
            self.cursor_x = 0;
            self.cursor_y += 1;
            if self.cursor_y >= VGA_HEIGHT {
                self.scroll();
            }
        }
    }
    
    fn scroll(&mut self) {
        unsafe {
            // Move lines up
            core::ptr::copy(
                VGA_BUFFER.add(VGA_WIDTH),
                VGA_BUFFER,
                VGA_WIDTH * (VGA_HEIGHT - 1)
            );
            
            // Clear last line
            for x in 0..VGA_WIDTH {
                VGA_BUFFER.add((VGA_HEIGHT - 1) * VGA_WIDTH + x)
                    .write_volatile((self.color as u16) << 8 | b' ' as u16);
            }
        }
        self.cursor_y = VGA_HEIGHT - 1;
    }
}
```

---

## Shell Commands

### Graphics Commands

| Command | Description | Example |
|---------|-------------|---------|
| `clear` | Clear screen | `clear` |
| `fbinfo` | Show framebuffer info | `fbinfo` |
| `color <fg> [bg]` | Set terminal colors | `color green black` |
| `demo graphics` | Run graphics demo | `demo graphics` |

### Example Output

```
splax> fbinfo
Framebuffer Information:
  Resolution: 1024x768
  Bits per pixel: 32
  Stride: 4096 bytes
  Format: BGRA32
  Base address: 0xFD000000
```

---

## Performance Considerations

### Optimization Techniques

1. **Dirty Rectangles**: Only redraw changed regions
2. **Double Buffering**: Eliminate tearing
3. **Batch Updates**: Group multiple draws
4. **Hardware Scrolling**: Use CRTC for fast scroll (if available)
5. **SIMD**: Use vector instructions for fills/copies

### Benchmarks

| Operation | Time (1024x768) |
|-----------|-----------------|
| Clear screen | ~2ms |
| Draw 1000 pixels | ~0.5ms |
| Draw 100 lines | ~1ms |
| Scroll (software) | ~3ms |
| Text (80x25) | ~1ms |

---

## File Structure

```
kernel/src/gpu/
├── mod.rs          # Module exports
├── framebuffer.rs  # Framebuffer abstraction
├── color.rs        # Color type and utilities
├── font.rs         # Bitmap font data
├── console.rs      # Text console
├── primitives.rs   # Drawing primitives
└── vga.rs          # VGA text mode fallback
```

---

## Future Work

1. **Hardware Acceleration**: GPU driver for 2D/3D
2. **Compositor**: Window manager with compositing
3. **Resolution Switching**: Change mode at runtime
4. **Font Loading**: TTF/OTF font support
5. **Anti-aliasing**: Sub-pixel rendering
6. **Image Decoding**: PNG, JPEG support
7. **Video Playback**: Frame-by-frame rendering
