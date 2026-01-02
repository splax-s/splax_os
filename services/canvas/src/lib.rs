//! S-CANVAS - Userspace Windowing and Compositor Service
//!
//! This service provides the windowing system and compositor for Splax OS.
//! It manages windows, surfaces, input events, and screen composition.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Applications                                 │
//! │  ┌───────────┐  ┌───────────┐  ┌───────────┐                    │
//! │  │  Terminal │  │   Editor  │  │  Browser  │  ...               │
//! │  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘                    │
//! │        └──────────────┴──────────────┘                          │
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────────────────────┐│
//! │  │                   S-CANVAS                                   ││
//! │  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  ││
//! │  │  │   Window    │  │  Compositor │  │    Input Router     │  ││
//! │  │  │   Manager   │  │   (vsync)   │  │  (keyboard/mouse)   │  ││
//! │  │  └─────────────┘  └─────────────┘  └─────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────────┘│
//! │                       │                                          │
//! │                       ▼                                          │
//! │  ┌─────────────────────────────────────────────────────────────┐│
//! │  │                    S-GPU                                     ││
//! │  │               (framebuffer access)                           ││
//! │  └─────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────┘
//! ```

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub mod window;
pub mod compositor;
pub mod surface;
pub mod input;
pub mod theme;

/// Canvas service configuration
#[derive(Debug, Clone)]
pub struct CanvasConfig {
    /// Enable hardware-accelerated compositing
    pub hw_accel: bool,
    /// Enable VSync for tear-free rendering
    pub vsync: bool,
    /// Enable window animations
    pub animations: bool,
    /// Default window decoration theme
    pub theme: Theme,
    /// Maximum number of windows
    pub max_windows: usize,
    /// Enable transparency/alpha blending
    pub transparency: bool,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            hw_accel: true,
            vsync: true,
            animations: true,
            theme: Theme::default(),
            max_windows: 256,
            transparency: true,
        }
    }
}

/// Window decoration theme
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Light theme with white backgrounds
    Light,
    /// Dark theme with dark backgrounds
    Dark,
    /// System default (follows system preference)
    System,
    /// No decorations (for fullscreen/custom)
    None,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

/// Window identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// Surface identifier (for multi-surface windows)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceId(pub u64);

/// Display/monitor identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(pub u32);

/// Point in screen coordinates
#[derive(Debug, Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Rectangle in screen coordinates
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x < self.x + self.width as i32
            && point.y >= self.y
            && point.y < self.y + self.height as i32
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width as i32
            && self.x + self.width as i32 > other.x
            && self.y < other.y + other.height as i32
            && self.y + self.height as i32 > other.y
    }
}

/// RGBA color
#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn to_u32(&self) -> u32 {
        (self.a as u32) << 24 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }

    // Common colors
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const RED: Self = Self::rgb(255, 0, 0);
    pub const GREEN: Self = Self::rgb(0, 255, 0);
    pub const BLUE: Self = Self::rgb(0, 0, 255);
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
}

/// Window state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Normal windowed mode
    Normal,
    /// Minimized to taskbar
    Minimized,
    /// Maximized to fill screen
    Maximized,
    /// Fullscreen (no decorations)
    Fullscreen,
    /// Hidden but not destroyed
    Hidden,
}

/// Window properties
#[derive(Debug, Clone)]
pub struct WindowProperties {
    /// Window title
    pub title: String,
    /// Window class (for grouping)
    pub class: String,
    /// Window geometry
    pub geometry: Rect,
    /// Window state
    pub state: WindowState,
    /// Is window resizable
    pub resizable: bool,
    /// Has window decorations
    pub decorated: bool,
    /// Is window always on top
    pub always_on_top: bool,
    /// Window opacity (0.0 - 1.0)
    pub opacity: f32,
    /// Minimum window size
    pub min_size: Option<(u32, u32)>,
    /// Maximum window size
    pub max_size: Option<(u32, u32)>,
}

impl Default for WindowProperties {
    fn default() -> Self {
        Self {
            title: String::new(),
            class: String::new(),
            geometry: Rect::new(0, 0, 800, 600),
            state: WindowState::Normal,
            resizable: true,
            decorated: true,
            always_on_top: false,
            opacity: 1.0,
            min_size: Some((100, 50)),
            max_size: None,
        }
    }
}

/// Canvas service state
pub struct CanvasService {
    /// Configuration
    config: CanvasConfig,
    /// All windows
    windows: BTreeMap<WindowId, WindowProperties>,
    /// Window Z-order (back to front)
    z_order: Vec<WindowId>,
    /// Focused window
    focused: Option<WindowId>,
    /// Next window ID
    next_window_id: u64,
    /// Screen dimensions
    screen_width: u32,
    screen_height: u32,
}

impl CanvasService {
    /// Create new canvas service
    pub fn new(config: CanvasConfig, screen_width: u32, screen_height: u32) -> Self {
        Self {
            config,
            windows: BTreeMap::new(),
            z_order: Vec::new(),
            focused: None,
            next_window_id: 1,
            screen_width,
            screen_height,
        }
    }

    /// Create a new window
    pub fn create_window(&mut self, props: WindowProperties) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        self.windows.insert(id, props);
        self.z_order.push(id);
        self.focused = Some(id);
        id
    }

    /// Destroy a window
    pub fn destroy_window(&mut self, id: WindowId) -> bool {
        if self.windows.remove(&id).is_some() {
            self.z_order.retain(|&w| w != id);
            if self.focused == Some(id) {
                self.focused = self.z_order.last().copied();
            }
            true
        } else {
            false
        }
    }

    /// Get window properties
    pub fn get_window(&self, id: WindowId) -> Option<&WindowProperties> {
        self.windows.get(&id)
    }

    /// Update window properties
    pub fn update_window(&mut self, id: WindowId, props: WindowProperties) -> bool {
        if let Some(window) = self.windows.get_mut(&id) {
            *window = props;
            true
        } else {
            false
        }
    }

    /// Focus a window
    pub fn focus_window(&mut self, id: WindowId) -> bool {
        if self.windows.contains_key(&id) {
            // Move to top of Z-order
            self.z_order.retain(|&w| w != id);
            self.z_order.push(id);
            self.focused = Some(id);
            true
        } else {
            false
        }
    }

    /// Get focused window
    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused
    }

    /// Get window at screen position
    pub fn window_at(&self, point: Point) -> Option<WindowId> {
        // Check from top to bottom
        for &id in self.z_order.iter().rev() {
            if let Some(props) = self.windows.get(&id) {
                if props.state != WindowState::Minimized && props.geometry.contains(point) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Get all windows in Z-order (back to front)
    pub fn windows_z_order(&self) -> &[WindowId] {
        &self.z_order
    }

    /// Set window state
    pub fn set_window_state(&mut self, id: WindowId, state: WindowState) -> bool {
        if let Some(window) = self.windows.get_mut(&id) {
            window.state = state;
            true
        } else {
            false
        }
    }

    /// Move window to position
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) -> bool {
        if let Some(window) = self.windows.get_mut(&id) {
            window.geometry.x = x;
            window.geometry.y = y;
            true
        } else {
            false
        }
    }

    /// Resize window
    pub fn resize_window(&mut self, id: WindowId, width: u32, height: u32) -> bool {
        if let Some(window) = self.windows.get_mut(&id) {
            if !window.resizable {
                return false;
            }
            
            // Clamp to min/max
            let (min_w, min_h) = window.min_size.unwrap_or((1, 1));
            let (max_w, max_h) = window.max_size.unwrap_or((u32::MAX, u32::MAX));
            
            window.geometry.width = width.clamp(min_w, max_w);
            window.geometry.height = height.clamp(min_h, max_h);
            true
        } else {
            false
        }
    }

    /// Get screen dimensions
    pub fn screen_size(&self) -> (u32, u32) {
        (self.screen_width, self.screen_height)
    }
}

/// Initialize the canvas service
pub fn init(config: CanvasConfig, width: u32, height: u32) -> CanvasService {
    CanvasService::new(config, width, height)
}
