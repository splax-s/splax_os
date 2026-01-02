//! Window management for S-CANVAS
//!
//! Handles window creation, destruction, and lifecycle.

use alloc::string::String;
use super::{WindowId, WindowProperties, WindowState, Rect};

/// Window creation request
#[derive(Debug, Clone)]
pub struct CreateWindowRequest {
    /// Window title
    pub title: String,
    /// Initial X position (None = center)
    pub x: Option<i32>,
    /// Initial Y position (None = center)
    pub y: Option<i32>,
    /// Initial width
    pub width: u32,
    /// Initial height
    pub height: u32,
    /// Is window resizable
    pub resizable: bool,
    /// Has window decorations
    pub decorated: bool,
    /// Is window modal (blocks parent)
    pub modal: bool,
    /// Parent window (for modal/transient)
    pub parent: Option<WindowId>,
}

impl Default for CreateWindowRequest {
    fn default() -> Self {
        Self {
            title: String::new(),
            x: None,
            y: None,
            width: 800,
            height: 600,
            resizable: true,
            decorated: true,
            modal: false,
            parent: None,
        }
    }
}

/// Window decoration areas
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationHitTest {
    /// Client area (application content)
    Client,
    /// Title bar (drag to move)
    TitleBar,
    /// Close button
    Close,
    /// Minimize button
    Minimize,
    /// Maximize button
    Maximize,
    /// Resize edges
    ResizeTop,
    ResizeBottom,
    ResizeLeft,
    ResizeRight,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
    /// Outside window
    None,
}

/// Decoration layout configuration
#[derive(Debug, Clone, Copy)]
pub struct DecorationLayout {
    /// Title bar height
    pub title_bar_height: u32,
    /// Border width
    pub border_width: u32,
    /// Resize handle size
    pub resize_handle_size: u32,
    /// Button width
    pub button_width: u32,
    /// Button padding
    pub button_padding: u32,
}

impl Default for DecorationLayout {
    fn default() -> Self {
        Self {
            title_bar_height: 30,
            border_width: 1,
            resize_handle_size: 8,
            button_width: 46,
            button_padding: 4,
        }
    }
}

/// Perform hit test on window decorations
pub fn decoration_hit_test(
    window: &WindowProperties,
    point_x: i32,
    point_y: i32,
    layout: &DecorationLayout,
) -> DecorationHitTest {
    let geom = &window.geometry;
    
    // Check if point is outside window
    if point_x < geom.x
        || point_x >= geom.x + geom.width as i32
        || point_y < geom.y
        || point_y >= geom.y + geom.height as i32
    {
        return DecorationHitTest::None;
    }

    if !window.decorated {
        return DecorationHitTest::Client;
    }

    let local_x = point_x - geom.x;
    let local_y = point_y - geom.y;
    let w = geom.width as i32;
    let h = geom.height as i32;
    let resize = layout.resize_handle_size as i32;
    let title_h = layout.title_bar_height as i32;
    let btn_w = layout.button_width as i32;

    // Check resize corners first
    if window.resizable {
        if local_x < resize && local_y < resize {
            return DecorationHitTest::ResizeTopLeft;
        }
        if local_x >= w - resize && local_y < resize {
            return DecorationHitTest::ResizeTopRight;
        }
        if local_x < resize && local_y >= h - resize {
            return DecorationHitTest::ResizeBottomLeft;
        }
        if local_x >= w - resize && local_y >= h - resize {
            return DecorationHitTest::ResizeBottomRight;
        }
        if local_y < resize {
            return DecorationHitTest::ResizeTop;
        }
        if local_y >= h - resize {
            return DecorationHitTest::ResizeBottom;
        }
        if local_x < resize {
            return DecorationHitTest::ResizeLeft;
        }
        if local_x >= w - resize {
            return DecorationHitTest::ResizeRight;
        }
    }

    // Check title bar
    if local_y < title_h {
        // Check buttons (right side)
        let buttons_start = w - btn_w * 3;
        if local_x >= buttons_start {
            let btn_idx = (local_x - buttons_start) / btn_w;
            return match btn_idx {
                0 => DecorationHitTest::Minimize,
                1 => DecorationHitTest::Maximize,
                2 => DecorationHitTest::Close,
                _ => DecorationHitTest::TitleBar,
            };
        }
        return DecorationHitTest::TitleBar;
    }

    DecorationHitTest::Client
}
