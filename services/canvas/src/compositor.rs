//! Compositor for S-CANVAS
//!
//! Handles window composition and screen rendering.

use alloc::vec;
use alloc::vec::Vec;
use super::{CanvasService, WindowId, WindowState, Color, Rect};

/// Damage region for partial repaints
#[derive(Debug, Clone)]
pub struct DamageRegion {
    /// Damaged rectangles
    pub rects: Vec<Rect>,
}

impl DamageRegion {
    pub fn new() -> Self {
        Self { rects: Vec::new() }
    }

    pub fn add(&mut self, rect: Rect) {
        // Could merge overlapping rectangles for optimization
        self.rects.push(rect);
    }

    pub fn clear(&mut self) {
        self.rects.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn full_screen(width: u32, height: u32) -> Self {
        Self {
            rects: vec![Rect::new(0, 0, width, height)],
        }
    }
}

impl Default for DamageRegion {
    fn default() -> Self {
        Self::new()
    }
}

/// Compositor state
pub struct Compositor {
    /// Screen width
    width: u32,
    /// Screen height
    height: u32,
    /// Damage since last composite
    damage: DamageRegion,
    /// Background color
    background: Color,
    /// Enable VSync
    vsync: bool,
    /// Frame count
    frame_count: u64,
}

impl Compositor {
    /// Create new compositor
    pub fn new(width: u32, height: u32, vsync: bool) -> Self {
        Self {
            width,
            height,
            damage: DamageRegion::full_screen(width, height),
            background: Color::rgb(40, 44, 52), // Dark background
            vsync,
            frame_count: 0,
        }
    }

    /// Mark region as damaged
    pub fn damage(&mut self, rect: Rect) {
        self.damage.add(rect);
    }

    /// Mark entire screen as damaged
    pub fn damage_all(&mut self) {
        self.damage = DamageRegion::full_screen(self.width, self.height);
    }

    /// Check if recomposition is needed
    pub fn needs_composite(&self) -> bool {
        !self.damage.is_empty()
    }

    /// Composite all windows to framebuffer
    /// Returns true if anything was drawn
    pub fn composite(&mut self, canvas: &CanvasService, framebuffer: &mut [u32]) -> bool {
        if !self.needs_composite() {
            return false;
        }

        // Clear damaged regions with background
        for rect in &self.damage.rects {
            self.fill_rect(framebuffer, rect, self.background);
        }

        // Draw windows back to front
        for &window_id in canvas.windows_z_order() {
            if let Some(props) = canvas.get_window(window_id) {
                if props.state != WindowState::Minimized && props.state != WindowState::Hidden {
                    self.draw_window(framebuffer, window_id, props);
                }
            }
        }

        self.damage.clear();
        self.frame_count += 1;
        true
    }

    /// Fill rectangle with color
    fn fill_rect(&self, fb: &mut [u32], rect: &Rect, color: Color) {
        let color_u32 = color.to_u32();
        let pitch = self.width as usize;

        let x_start = rect.x.max(0) as usize;
        let x_end = ((rect.x + rect.width as i32) as usize).min(self.width as usize);
        let y_start = rect.y.max(0) as usize;
        let y_end = ((rect.y + rect.height as i32) as usize).min(self.height as usize);

        for y in y_start..y_end {
            let row_start = y * pitch + x_start;
            let row_end = y * pitch + x_end;
            if row_end <= fb.len() {
                for pixel in &mut fb[row_start..row_end] {
                    *pixel = color_u32;
                }
            }
        }
    }

    /// Draw window to framebuffer
    fn draw_window(&self, fb: &mut [u32], _id: WindowId, props: &super::WindowProperties) {
        let geom = &props.geometry;
        
        // Draw shadow (if decorated)
        if props.decorated {
            let shadow = Rect::new(geom.x + 4, geom.y + 4, geom.width, geom.height);
            self.fill_rect(fb, &shadow, Color::new(0, 0, 0, 80));
        }

        // Draw window background
        let bg_color = if props.decorated {
            Color::rgb(30, 30, 30) // Dark window background
        } else {
            Color::rgb(20, 20, 20)
        };
        self.fill_rect(fb, &Rect::new(geom.x, geom.y, geom.width, geom.height), bg_color);

        // Draw decorations
        if props.decorated {
            // Title bar
            let title_bar = Rect::new(geom.x, geom.y, geom.width, 30);
            let title_color = if Some(_id) == super::WindowId(0).into() {
                // Would check focused status
                Color::rgb(50, 50, 60)
            } else {
                Color::rgb(40, 40, 50)
            };
            self.fill_rect(fb, &title_bar, title_color);

            // Close button (red)
            let close_btn = Rect::new(geom.x + geom.width as i32 - 46, geom.y, 46, 30);
            self.fill_rect(fb, &close_btn, Color::rgb(200, 60, 60));

            // Maximize button (green-ish)
            let max_btn = Rect::new(geom.x + geom.width as i32 - 92, geom.y, 46, 30);
            self.fill_rect(fb, &max_btn, Color::rgb(60, 60, 70));

            // Minimize button
            let min_btn = Rect::new(geom.x + geom.width as i32 - 138, geom.y, 46, 30);
            self.fill_rect(fb, &min_btn, Color::rgb(60, 60, 70));

            // Border
            let border_color = Color::rgb(60, 60, 70);
            // Top
            self.fill_rect(fb, &Rect::new(geom.x, geom.y, geom.width, 1), border_color);
            // Bottom
            self.fill_rect(fb, &Rect::new(geom.x, geom.y + geom.height as i32 - 1, geom.width, 1), border_color);
            // Left
            self.fill_rect(fb, &Rect::new(geom.x, geom.y, 1, geom.height), border_color);
            // Right
            self.fill_rect(fb, &Rect::new(geom.x + geom.width as i32 - 1, geom.y, 1, geom.height), border_color);
        }
    }

    /// Get frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Set background color
    pub fn set_background(&mut self, color: Color) {
        self.background = color;
        self.damage_all();
    }
}
