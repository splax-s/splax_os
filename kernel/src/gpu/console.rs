//! # Graphics Console
//!
//! Text console implementation using the framebuffer and bitmap font.
//! Provides terminal-like text output with colors and scrolling.

use spin::Mutex;
use super::{
    color::Color,
    font::{self, FONT_WIDTH, FONT_HEIGHT},
    framebuffer,
};

/// Text console state
pub struct Console {
    /// Current cursor column (in characters)
    cursor_x: u32,
    /// Current cursor row (in characters)
    cursor_y: u32,
    /// Number of character columns
    cols: u32,
    /// Number of character rows
    rows: u32,
    /// Foreground color
    fg_color: Color,
    /// Background color
    bg_color: Color,
    /// Whether cursor is visible
    cursor_visible: bool,
    /// Cursor blink state
    cursor_blink: bool,
    /// Tab stop width
    tab_width: u32,
}

impl Console {
    /// Creates a new console
    pub const fn new() -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
            cols: 0,
            rows: 0,
            fg_color: Color::SPLAX_FG,
            bg_color: Color::SPLAX_BG,
            cursor_visible: true,
            cursor_blink: false,
            tab_width: 4,
        }
    }
    
    /// Initializes the console with the current framebuffer dimensions
    pub fn init(&mut self) {
        if let Some(mode) = framebuffer::display_mode() {
            self.cols = mode.width / FONT_WIDTH;
            self.rows = mode.height / FONT_HEIGHT;
            self.cursor_x = 0;
            self.cursor_y = 0;
            
            // Clear screen with background color
            framebuffer::clear(self.bg_color);
        }
    }
    
    /// Returns the console dimensions in characters
    pub fn dimensions(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }
    
    /// Returns current cursor position
    pub fn cursor_position(&self) -> (u32, u32) {
        (self.cursor_x, self.cursor_y)
    }
    
    /// Sets cursor position
    pub fn set_cursor(&mut self, x: u32, y: u32) {
        if x < self.cols && y < self.rows {
            self.cursor_x = x;
            self.cursor_y = y;
        }
    }
    
    /// Sets the foreground color
    pub fn set_fg_color(&mut self, color: Color) {
        self.fg_color = color;
    }
    
    /// Sets the background color
    pub fn set_bg_color(&mut self, color: Color) {
        self.bg_color = color;
    }
    
    /// Sets both foreground and background colors
    pub fn set_colors(&mut self, fg: Color, bg: Color) {
        self.fg_color = fg;
        self.bg_color = bg;
    }
    
    /// Resets colors to default
    pub fn reset_colors(&mut self) {
        self.fg_color = Color::SPLAX_FG;
        self.bg_color = Color::SPLAX_BG;
    }
    
    /// Clears the entire console
    pub fn clear(&mut self) {
        framebuffer::clear(self.bg_color);
        self.cursor_x = 0;
        self.cursor_y = 0;
    }
    
    /// Clears from cursor to end of line
    pub fn clear_to_eol(&mut self) {
        for x in self.cursor_x..self.cols {
            self.draw_char_at(x, self.cursor_y, ' ');
        }
    }
    
    /// Clears from cursor to end of screen
    pub fn clear_to_eos(&mut self) {
        self.clear_to_eol();
        for y in (self.cursor_y + 1)..self.rows {
            for x in 0..self.cols {
                self.draw_char_at(x, y, ' ');
            }
        }
    }
    
    /// Scrolls the console up by one line
    pub fn scroll_up(&mut self) {
        framebuffer::scroll_up(FONT_HEIGHT, self.bg_color);
    }
    
    /// Draws a character at the specified position
    fn draw_char_at(&self, col: u32, row: u32, c: char) {
        let x = col * FONT_WIDTH;
        let y = row * FONT_HEIGHT;
        
        let glyph = font::get_glyph(c);
        
        if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
            for gy in 0..FONT_HEIGHT {
                for gx in 0..FONT_WIDTH {
                    let color = if font::glyph_pixel(glyph, gx, gy) {
                        self.fg_color
                    } else {
                        self.bg_color
                    };
                    fb.set_pixel(x + gx, y + gy, color);
                }
            }
        }
    }
    
    /// Draws the cursor
    fn draw_cursor(&self) {
        if !self.cursor_visible {
            return;
        }
        
        let x = self.cursor_x * FONT_WIDTH;
        let y = self.cursor_y * FONT_HEIGHT;
        
        // Draw underscore cursor
        if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
            for gx in 0..FONT_WIDTH {
                fb.set_pixel(x + gx, y + FONT_HEIGHT - 2, self.fg_color);
                fb.set_pixel(x + gx, y + FONT_HEIGHT - 1, self.fg_color);
            }
        }
    }
    
    /// Erases the cursor
    fn erase_cursor(&self) {
        let x = self.cursor_x * FONT_WIDTH;
        let y = self.cursor_y * FONT_HEIGHT;
        
        if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
            for gx in 0..FONT_WIDTH {
                fb.set_pixel(x + gx, y + FONT_HEIGHT - 2, self.bg_color);
                fb.set_pixel(x + gx, y + FONT_HEIGHT - 1, self.bg_color);
            }
        }
    }
    
    /// Advances cursor position, handling wrapping and scrolling
    fn advance_cursor(&mut self) {
        self.cursor_x += 1;
        if self.cursor_x >= self.cols {
            self.cursor_x = 0;
            self.cursor_y += 1;
        }
        if self.cursor_y >= self.rows {
            self.scroll_up();
            self.cursor_y = self.rows - 1;
        }
    }
    
    /// Writes a character to the console
    pub fn put_char(&mut self, c: char) {
        self.erase_cursor();
        
        match c {
            '\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
                if self.cursor_y >= self.rows {
                    self.scroll_up();
                    self.cursor_y = self.rows - 1;
                }
            }
            '\r' => {
                self.cursor_x = 0;
            }
            '\t' => {
                let spaces = self.tab_width - (self.cursor_x % self.tab_width);
                for _ in 0..spaces {
                    if self.cursor_x < self.cols {
                        self.draw_char_at(self.cursor_x, self.cursor_y, ' ');
                        self.advance_cursor();
                    }
                }
            }
            '\x08' => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    self.draw_char_at(self.cursor_x, self.cursor_y, ' ');
                }
            }
            c if c >= ' ' => {
                self.draw_char_at(self.cursor_x, self.cursor_y, c);
                self.advance_cursor();
            }
            _ => {}
        }
        
        self.draw_cursor();
    }
    
    /// Writes a string to the console
    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }
    
    /// Writes a string with a specific foreground color
    pub fn write_colored(&mut self, s: &str, fg: Color) {
        let old_fg = self.fg_color;
        self.fg_color = fg;
        self.write_str(s);
        self.fg_color = old_fg;
    }
    
    /// Writes a line (string followed by newline)
    pub fn write_line(&mut self, s: &str) {
        self.write_str(s);
        self.put_char('\n');
    }
    
    /// Toggles cursor visibility
    pub fn toggle_cursor(&mut self) {
        self.cursor_visible = !self.cursor_visible;
        if self.cursor_visible {
            self.draw_cursor();
        } else {
            self.erase_cursor();
        }
    }
    
    /// Updates cursor blink (call periodically)
    pub fn blink_cursor(&mut self) {
        if self.cursor_visible {
            self.cursor_blink = !self.cursor_blink;
            if self.cursor_blink {
                self.draw_cursor();
            } else {
                self.erase_cursor();
            }
        }
    }
}

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        Console::write_str(self, s);
        Ok(())
    }
}

/// Global console instance
pub static CONSOLE: Mutex<Console> = Mutex::new(Console::new());

/// Initializes the global console
pub fn init() {
    CONSOLE.lock().init();
}

/// Writes a string to the console
pub fn write_str(s: &str) {
    CONSOLE.lock().write_str(s);
}

/// Writes a character to the console
pub fn put_char(c: char) {
    CONSOLE.lock().put_char(c);
}

/// Writes a line to the console
pub fn write_line(s: &str) {
    CONSOLE.lock().write_line(s);
}

/// Clears the console
pub fn clear() {
    CONSOLE.lock().clear();
}

/// Sets the console colors
pub fn set_colors(fg: Color, bg: Color) {
    CONSOLE.lock().set_colors(fg, bg);
}

/// Console print macro
#[macro_export]
macro_rules! console_print {
    ($($arg:tt)*) => ({
        use core::fmt::Write;
        let _ = write!($crate::gpu::console::CONSOLE.lock(), $($arg)*);
    });
}

/// Console println macro
#[macro_export]
macro_rules! console_println {
    () => ($crate::console_print!("\n"));
    ($($arg:tt)*) => ({
        $crate::console_print!($($arg)*);
        $crate::console_print!("\n");
    });
}
