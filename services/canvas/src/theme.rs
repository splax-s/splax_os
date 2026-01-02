//! Theme definitions for S-CANVAS
//!
//! Defines colors and styles for window decorations.

use super::Color;

/// Theme colors
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    /// Window background
    pub window_bg: Color,
    /// Title bar background (active)
    pub title_bar_active: Color,
    /// Title bar background (inactive)
    pub title_bar_inactive: Color,
    /// Title text color (active)
    pub title_text_active: Color,
    /// Title text color (inactive)
    pub title_text_inactive: Color,
    /// Border color
    pub border: Color,
    /// Close button
    pub close_button: Color,
    /// Close button hover
    pub close_button_hover: Color,
    /// Button normal
    pub button: Color,
    /// Button hover
    pub button_hover: Color,
    /// Shadow color
    pub shadow: Color,
    /// Desktop background
    pub desktop_bg: Color,
}

impl ThemeColors {
    /// Dark theme
    pub const DARK: Self = Self {
        window_bg: Color::rgb(30, 30, 30),
        title_bar_active: Color::rgb(50, 50, 60),
        title_bar_inactive: Color::rgb(40, 40, 45),
        title_text_active: Color::rgb(255, 255, 255),
        title_text_inactive: Color::rgb(160, 160, 160),
        border: Color::rgb(60, 60, 70),
        close_button: Color::rgb(200, 60, 60),
        close_button_hover: Color::rgb(240, 70, 70),
        button: Color::rgb(60, 60, 70),
        button_hover: Color::rgb(80, 80, 90),
        shadow: Color::new(0, 0, 0, 80),
        desktop_bg: Color::rgb(40, 44, 52),
    };

    /// Light theme
    pub const LIGHT: Self = Self {
        window_bg: Color::rgb(255, 255, 255),
        title_bar_active: Color::rgb(220, 220, 220),
        title_bar_inactive: Color::rgb(240, 240, 240),
        title_text_active: Color::rgb(0, 0, 0),
        title_text_inactive: Color::rgb(120, 120, 120),
        border: Color::rgb(180, 180, 180),
        close_button: Color::rgb(200, 60, 60),
        close_button_hover: Color::rgb(240, 70, 70),
        button: Color::rgb(200, 200, 200),
        button_hover: Color::rgb(180, 180, 180),
        shadow: Color::new(0, 0, 0, 40),
        desktop_bg: Color::rgb(240, 240, 245),
    };
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self::DARK
    }
}

/// Font configuration
#[derive(Debug, Clone)]
pub struct FontConfig {
    /// Font name
    pub name: &'static str,
    /// Title bar font size
    pub title_size: u8,
    /// Regular font size
    pub regular_size: u8,
    /// Small font size
    pub small_size: u8,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            name: "System",
            title_size: 12,
            regular_size: 11,
            small_size: 9,
        }
    }
}

/// Complete theme definition
#[derive(Debug, Clone)]
pub struct ThemeDefinition {
    /// Theme name
    pub name: &'static str,
    /// Colors
    pub colors: ThemeColors,
    /// Font configuration
    pub fonts: FontConfig,
    /// Window corner radius
    pub corner_radius: u8,
    /// Shadow size
    pub shadow_size: u8,
    /// Title bar height
    pub title_bar_height: u8,
    /// Border width
    pub border_width: u8,
}

impl Default for ThemeDefinition {
    fn default() -> Self {
        Self {
            name: "Dark",
            colors: ThemeColors::DARK,
            fonts: FontConfig::default(),
            corner_radius: 0, // No rounded corners (simpler to render)
            shadow_size: 4,
            title_bar_height: 30,
            border_width: 1,
        }
    }
}

/// Built-in themes
pub mod builtin {
    use super::*;

    pub const DARK: ThemeDefinition = ThemeDefinition {
        name: "Dark",
        colors: ThemeColors::DARK,
        fonts: FontConfig {
            name: "System",
            title_size: 12,
            regular_size: 11,
            small_size: 9,
        },
        corner_radius: 0,
        shadow_size: 4,
        title_bar_height: 30,
        border_width: 1,
    };

    pub const LIGHT: ThemeDefinition = ThemeDefinition {
        name: "Light",
        colors: ThemeColors::LIGHT,
        fonts: FontConfig {
            name: "System",
            title_size: 12,
            regular_size: 11,
            small_size: 9,
        },
        corner_radius: 0,
        shadow_size: 4,
        title_bar_height: 30,
        border_width: 1,
    };
}
