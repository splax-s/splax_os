//! # Graphics Subsystem
//!
//! This module provides graphics and display support for Splax OS,
//! including framebuffer management, basic 2D drawing primitives,
//! and text rendering.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Graphics Layer                        │
//! ├─────────────────────────────────────────────────────────┤
//! │  Console  │  Canvas  │  Window Manager  │  Compositor   │
//! ├───────────┴──────────┴──────────────────┴───────────────┤
//! │                   Framebuffer Driver                     │
//! ├─────────────────────────────────────────────────────────┤
//! │  VGA Text  │  VESA/VBE  │  GOP (UEFI)  │  VirtIO-GPU    │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod color;
pub mod font;
pub mod framebuffer;
pub mod console;
pub mod primitives;

pub use color::Color;
pub use framebuffer::{DisplayMode, FramebufferInfo, PixelFormat, FRAMEBUFFER};

// =============================================================================
// Re-exports for convenience
// =============================================================================

/// Initialize the graphics subsystem
pub fn init() {
    // Try to detect and initialize framebuffer
    if let Some(fb_info) = framebuffer::detect_framebuffer() {
        framebuffer::init(fb_info);
        
        // Initialize console on top of framebuffer
        console::init();
    }
}

/// Returns the current display mode
pub fn display_mode() -> Option<DisplayMode> {
    framebuffer::display_mode()
}
