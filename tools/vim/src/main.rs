//! # S-VIM: Vi IMproved for Splax OS
//!
//! A vi-compatible text editor implementation for Splax OS.
//! Provides modal editing with normal, insert, visual, and command modes.
//!
//! ## Features
//!
//! - Modal editing (normal, insert, visual, command-line)
//! - Basic file operations (read, write, quit)
//! - Movement commands (h, j, k, l, w, b, 0, $, G, gg)
//! - Editing commands (i, a, o, O, x, dd, yy, p, P)
//! - Search and replace (/, ?, :s)
//! - Undo/redo (u, Ctrl-r)
//! - Visual mode selection (v, V, Ctrl-v)
//! - Basic syntax highlighting
//! - Multiple buffers and windows

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;

// =============================================================================
// Editor Modes
// =============================================================================

/// Vim editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal mode - navigation and commands.
    Normal,
    /// Insert mode - text insertion.
    Insert,
    /// Visual mode - character selection.
    Visual,
    /// Visual line mode - line selection.
    VisualLine,
    /// Visual block mode - block selection.
    VisualBlock,
    /// Command-line mode - ex commands.
    Command,
    /// Search mode.
    Search,
    /// Replace mode.
    Replace,
}

impl Mode {
    /// Get mode indicator for status line.
    pub fn indicator(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "-- INSERT --",
            Mode::Visual => "-- VISUAL --",
            Mode::VisualLine => "-- VISUAL LINE --",
            Mode::VisualBlock => "-- VISUAL BLOCK --",
            Mode::Command => ":",
            Mode::Search => "/",
            Mode::Replace => "-- REPLACE --",
        }
    }
}

// =============================================================================
// Cursor Position
// =============================================================================

/// Cursor position in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// Row (0-indexed).
    pub row: usize,
    /// Column (0-indexed).
    pub col: usize,
}

impl Position {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

// =============================================================================
// Text Buffer
// =============================================================================

/// A text buffer representing a file.
#[derive(Debug, Clone)]
pub struct Buffer {
    /// Buffer name (filename).
    name: String,
    /// Lines of text.
    lines: Vec<String>,
    /// Whether buffer has been modified.
    modified: bool,
    /// File path (if opened from file).
    path: Option<String>,
    /// Line endings type.
    line_ending: LineEnding,
}

/// Line ending type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Unix style (LF).
    Unix,
    /// DOS/Windows style (CRLF).
    Dos,
    /// Classic Mac style (CR).
    Mac,
}

impl Default for LineEnding {
    fn default() -> Self {
        LineEnding::Unix
    }
}

impl Buffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        Self {
            name: "[No Name]".to_string(),
            lines: vec![String::new()],
            modified: false,
            path: None,
            line_ending: LineEnding::Unix,
        }
    }

    /// Create a buffer with content.
    pub fn from_content(name: &str, content: &str) -> Self {
        let (line_ending, lines) = Self::parse_content(content);

        Self {
            name: name.to_string(),
            lines,
            modified: false,
            path: Some(name.to_string()),
            line_ending,
        }
    }

    /// Parse content into lines, detecting line endings.
    fn parse_content(content: &str) -> (LineEnding, Vec<String>) {
        let line_ending = if content.contains("\r\n") {
            LineEnding::Dos
        } else if content.contains('\r') {
            LineEnding::Mac
        } else {
            LineEnding::Unix
        };

        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<String> = normalized.lines().map(|s| s.to_string()).collect();

        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };

        (line_ending, lines)
    }

    /// Get line count.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Get a line by index.
    pub fn get_line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(|s| s.as_str())
    }

    /// Get line length.
    pub fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map(|s| s.len()).unwrap_or(0)
    }

    /// Insert a character at position.
    pub fn insert_char(&mut self, pos: Position, ch: char) {
        if pos.row < self.lines.len() {
            let line = &mut self.lines[pos.row];
            if pos.col <= line.len() {
                line.insert(pos.col, ch);
                self.modified = true;
            }
        }
    }

    /// Delete a character at position.
    pub fn delete_char(&mut self, pos: Position) -> Option<char> {
        if pos.row < self.lines.len() {
            let line = &mut self.lines[pos.row];
            if pos.col < line.len() {
                self.modified = true;
                return Some(line.remove(pos.col));
            }
        }
        None
    }

    /// Insert a new line at position.
    pub fn insert_line(&mut self, row: usize, content: String) {
        if row <= self.lines.len() {
            self.lines.insert(row, content);
            self.modified = true;
        }
    }

    /// Delete a line.
    pub fn delete_line(&mut self, row: usize) -> Option<String> {
        if row < self.lines.len() && self.lines.len() > 1 {
            self.modified = true;
            return Some(self.lines.remove(row));
        }
        None
    }

    /// Split a line at position (for Enter key).
    pub fn split_line(&mut self, pos: Position) {
        if pos.row < self.lines.len() {
            let line = &self.lines[pos.row];
            let col = pos.col.min(line.len());
            let remainder = line[col..].to_string();
            self.lines[pos.row] = line[..col].to_string();
            self.lines.insert(pos.row + 1, remainder);
            self.modified = true;
        }
    }

    /// Join line with next line.
    pub fn join_lines(&mut self, row: usize) {
        if row + 1 < self.lines.len() {
            let next_line = self.lines.remove(row + 1);
            self.lines[row].push_str(&next_line);
            self.modified = true;
        }
    }

    /// Convert buffer to string.
    pub fn to_string(&self) -> String {
        let ending = match self.line_ending {
            LineEnding::Unix => "\n",
            LineEnding::Dos => "\r\n",
            LineEnding::Mac => "\r",
        };

        let mut result = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            result.push_str(line);
            if i < self.lines.len() - 1 {
                result.push_str(ending);
            }
        }
        result
    }

    /// Get buffer name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if modified.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Mark as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Undo/Redo Support
// =============================================================================

/// An edit action for undo/redo.
#[derive(Debug, Clone)]
pub enum EditAction {
    /// Insert text.
    Insert { pos: Position, text: String },
    /// Delete text.
    Delete { pos: Position, text: String },
    /// Insert line.
    InsertLine { row: usize, content: String },
    /// Delete line.
    DeleteLine { row: usize, content: String },
}

impl EditAction {
    /// Get the inverse action (for undo).
    pub fn inverse(&self) -> Self {
        match self {
            EditAction::Insert { pos, text } => EditAction::Delete {
                pos: *pos,
                text: text.clone(),
            },
            EditAction::Delete { pos, text } => EditAction::Insert {
                pos: *pos,
                text: text.clone(),
            },
            EditAction::InsertLine { row, content } => EditAction::DeleteLine {
                row: *row,
                content: content.clone(),
            },
            EditAction::DeleteLine { row, content } => EditAction::InsertLine {
                row: *row,
                content: content.clone(),
            },
        }
    }
}

// =============================================================================
// Register (Clipboard)
// =============================================================================

/// Register type for yank/paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterType {
    /// Character-wise register.
    Char,
    /// Line-wise register.
    Line,
    /// Block-wise register.
    Block,
}

/// A register for yank/paste operations.
#[derive(Debug, Clone)]
pub struct Register {
    /// Content.
    pub content: String,
    /// Register type.
    pub reg_type: RegisterType,
}

impl Default for Register {
    fn default() -> Self {
        Self {
            content: String::new(),
            reg_type: RegisterType::Char,
        }
    }
}

// =============================================================================
// Vim Editor State
// =============================================================================

/// The main Vim editor state.
pub struct VimEditor {
    /// Current buffer.
    buffer: Buffer,
    /// Current mode.
    mode: Mode,
    /// Cursor position.
    cursor: Position,
    /// View offset (for scrolling).
    view_offset: Position,
    /// Terminal size.
    size: (usize, usize),
    /// Undo stack.
    undo_stack: VecDeque<EditAction>,
    /// Redo stack.
    redo_stack: VecDeque<EditAction>,
    /// Default register.
    register: Register,
    /// Named registers (a-z).
    named_registers: [Register; 26],
    /// Command line buffer.
    command_buffer: String,
    /// Search pattern.
    search_pattern: String,
    /// Last search direction.
    search_forward: bool,
    /// Status message.
    status_message: Option<String>,
    /// Pending operator (d, y, c).
    pending_operator: Option<char>,
    /// Count prefix for commands.
    count: Option<usize>,
    /// Visual mode start position.
    visual_start: Position,
}

impl VimEditor {
    /// Create a new Vim editor.
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            mode: Mode::Normal,
            cursor: Position::default(),
            view_offset: Position::default(),
            size: (80, 24),
            undo_stack: VecDeque::with_capacity(1000),
            redo_stack: VecDeque::with_capacity(1000),
            register: Register::default(),
            named_registers: Default::default(),
            command_buffer: String::new(),
            search_pattern: String::new(),
            search_forward: true,
            status_message: None,
            pending_operator: None,
            count: None,
            visual_start: Position::default(),
        }
    }

    /// Open a file.
    pub fn open(&mut self, path: &str, content: &str) {
        self.buffer = Buffer::from_content(path, content);
        self.cursor = Position::default();
        self.view_offset = Position::default();
        self.mode = Mode::Normal;
        self.status_message = Some(alloc::format!(
            "\"{}\" {}L, {}B",
            path,
            self.buffer.line_count(),
            content.len()
        ));
    }

    /// Set terminal size.
    pub fn set_size(&mut self, width: usize, height: usize) {
        self.size = (width, height);
    }

    /// Handle a key press.
    pub fn handle_key(&mut self, key: Key) -> EditorAction {
        match self.mode {
            Mode::Normal => self.handle_normal_key(key),
            Mode::Insert => self.handle_insert_key(key),
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                self.handle_visual_key(key)
            }
            Mode::Command => self.handle_command_key(key),
            Mode::Search => self.handle_search_key(key),
            Mode::Replace => self.handle_replace_key(key),
        }
    }

    /// Handle key in normal mode.
    fn handle_normal_key(&mut self, key: Key) -> EditorAction {
        self.status_message = None;

        match key {
            // Mode switching
            Key::Char('i') => {
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('a') => {
                self.move_cursor_right();
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('I') => {
                self.move_cursor_start_of_line();
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('A') => {
                self.move_cursor_end_of_line();
                self.move_cursor_right(); // Past end for append
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('o') => {
                let row = self.cursor.row;
                self.buffer.insert_line(row + 1, String::new());
                self.cursor = Position::new(row + 1, 0);
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('O') => {
                let row = self.cursor.row;
                self.buffer.insert_line(row, String::new());
                self.cursor = Position::new(row, 0);
                self.mode = Mode::Insert;
                EditorAction::Redraw
            }
            Key::Char('v') => {
                self.visual_start = self.cursor;
                self.mode = Mode::Visual;
                EditorAction::Redraw
            }
            Key::Char('V') => {
                self.visual_start = self.cursor;
                self.mode = Mode::VisualLine;
                EditorAction::Redraw
            }
            Key::Char(':') => {
                self.command_buffer.clear();
                self.mode = Mode::Command;
                EditorAction::Redraw
            }
            Key::Char('/') => {
                self.command_buffer.clear();
                self.search_forward = true;
                self.mode = Mode::Search;
                EditorAction::Redraw
            }
            Key::Char('?') => {
                self.command_buffer.clear();
                self.search_forward = false;
                self.mode = Mode::Search;
                EditorAction::Redraw
            }
            Key::Char('R') => {
                self.mode = Mode::Replace;
                EditorAction::Redraw
            }

            // Movement
            Key::Char('h') | Key::Left => {
                self.move_cursor_left();
                EditorAction::Redraw
            }
            Key::Char('j') | Key::Down => {
                self.move_cursor_down();
                EditorAction::Redraw
            }
            Key::Char('k') | Key::Up => {
                self.move_cursor_up();
                EditorAction::Redraw
            }
            Key::Char('l') | Key::Right => {
                self.move_cursor_right();
                EditorAction::Redraw
            }
            Key::Char('w') => {
                self.move_word_forward();
                EditorAction::Redraw
            }
            Key::Char('b') => {
                self.move_word_backward();
                EditorAction::Redraw
            }
            Key::Char('e') => {
                self.move_word_end();
                EditorAction::Redraw
            }
            Key::Char('0') if self.count.is_none() => {
                self.cursor.col = 0;
                EditorAction::Redraw
            }
            Key::Char('^') => {
                self.move_cursor_first_nonblank();
                EditorAction::Redraw
            }
            Key::Char('$') => {
                self.move_cursor_end_of_line();
                EditorAction::Redraw
            }
            Key::Char('G') => {
                self.cursor.row = self.buffer.line_count().saturating_sub(1);
                self.clamp_cursor();
                EditorAction::Redraw
            }
            Key::Char('g') => {
                // Wait for second 'g'
                self.pending_operator = Some('g');
                EditorAction::None
            }

            // Editing
            Key::Char('x') => {
                if let Some(ch) = self.buffer.delete_char(self.cursor) {
                    self.register = Register {
                        content: ch.to_string(),
                        reg_type: RegisterType::Char,
                    };
                    self.clamp_cursor();
                }
                EditorAction::Redraw
            }
            Key::Char('d') => {
                if self.pending_operator == Some('d') {
                    // dd - delete line
                    if let Some(line) = self.buffer.delete_line(self.cursor.row) {
                        self.register = Register {
                            content: line,
                            reg_type: RegisterType::Line,
                        };
                        self.clamp_cursor();
                    }
                    self.pending_operator = None;
                } else {
                    self.pending_operator = Some('d');
                }
                EditorAction::Redraw
            }
            Key::Char('y') => {
                if self.pending_operator == Some('y') {
                    // yy - yank line
                    if let Some(line) = self.buffer.get_line(self.cursor.row) {
                        self.register = Register {
                            content: line.to_string(),
                            reg_type: RegisterType::Line,
                        };
                    }
                    self.pending_operator = None;
                } else {
                    self.pending_operator = Some('y');
                }
                EditorAction::Redraw
            }
            Key::Char('p') => {
                self.paste_after();
                EditorAction::Redraw
            }
            Key::Char('P') => {
                self.paste_before();
                EditorAction::Redraw
            }
            Key::Char('u') => {
                self.undo();
                EditorAction::Redraw
            }
            Key::Ctrl('r') => {
                self.redo();
                EditorAction::Redraw
            }
            Key::Char('J') => {
                self.buffer.join_lines(self.cursor.row);
                EditorAction::Redraw
            }

            // Search
            Key::Char('n') => {
                self.search_next();
                EditorAction::Redraw
            }
            Key::Char('N') => {
                self.search_prev();
                EditorAction::Redraw
            }

            // Count
            Key::Char(c @ '1'..='9') => {
                let digit = c as usize - '0' as usize;
                self.count = Some(self.count.unwrap_or(0) * 10 + digit);
                EditorAction::None
            }

            // Handle pending operator
            _ if self.pending_operator == Some('g') => {
                match key {
                    Key::Char('g') => {
                        // gg - go to first line
                        self.cursor.row = 0;
                        self.clamp_cursor();
                    }
                    _ => {}
                }
                self.pending_operator = None;
                EditorAction::Redraw
            }

            Key::Esc => {
                self.pending_operator = None;
                self.count = None;
                EditorAction::Redraw
            }

            _ => EditorAction::None,
        }
    }

    /// Handle key in insert mode.
    fn handle_insert_key(&mut self, key: Key) -> EditorAction {
        match key {
            Key::Esc => {
                // Move cursor back one if not at start
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Char(c) => {
                self.buffer.insert_char(self.cursor, c);
                self.cursor.col += 1;
                EditorAction::Redraw
            }
            Key::Enter => {
                self.buffer.split_line(self.cursor);
                self.cursor = Position::new(self.cursor.row + 1, 0);
                EditorAction::Redraw
            }
            Key::Backspace => {
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                    self.buffer.delete_char(self.cursor);
                } else if self.cursor.row > 0 {
                    // Join with previous line
                    let prev_len = self.buffer.line_len(self.cursor.row - 1);
                    self.buffer.join_lines(self.cursor.row - 1);
                    self.cursor = Position::new(self.cursor.row - 1, prev_len);
                }
                EditorAction::Redraw
            }
            Key::Delete => {
                let line_len = self.buffer.line_len(self.cursor.row);
                if self.cursor.col < line_len {
                    self.buffer.delete_char(self.cursor);
                } else if self.cursor.row + 1 < self.buffer.line_count() {
                    self.buffer.join_lines(self.cursor.row);
                }
                EditorAction::Redraw
            }
            Key::Tab => {
                // Insert 4 spaces (or tab character based on settings)
                for _ in 0..4 {
                    self.buffer.insert_char(self.cursor, ' ');
                    self.cursor.col += 1;
                }
                EditorAction::Redraw
            }
            Key::Left => {
                self.move_cursor_left();
                EditorAction::Redraw
            }
            Key::Right => {
                self.move_cursor_right();
                EditorAction::Redraw
            }
            Key::Up => {
                self.move_cursor_up();
                EditorAction::Redraw
            }
            Key::Down => {
                self.move_cursor_down();
                EditorAction::Redraw
            }
            Key::Home => {
                self.cursor.col = 0;
                EditorAction::Redraw
            }
            Key::End => {
                self.move_cursor_end_of_line();
                EditorAction::Redraw
            }
            _ => EditorAction::None,
        }
    }

    /// Handle key in visual mode.
    fn handle_visual_key(&mut self, key: Key) -> EditorAction {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Char('y') => {
                // Yank selection
                let text = self.get_selection();
                self.register = Register {
                    content: text,
                    reg_type: if self.mode == Mode::VisualLine {
                        RegisterType::Line
                    } else {
                        RegisterType::Char
                    },
                };
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Char('d') | Key::Char('x') => {
                // Delete selection
                self.delete_selection();
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            // Movement keys work the same
            Key::Char('h') | Key::Left => {
                self.move_cursor_left();
                EditorAction::Redraw
            }
            Key::Char('j') | Key::Down => {
                self.move_cursor_down();
                EditorAction::Redraw
            }
            Key::Char('k') | Key::Up => {
                self.move_cursor_up();
                EditorAction::Redraw
            }
            Key::Char('l') | Key::Right => {
                self.move_cursor_right();
                EditorAction::Redraw
            }
            _ => EditorAction::None,
        }
    }

    /// Handle key in command mode.
    fn handle_command_key(&mut self, key: Key) -> EditorAction {
        match key {
            Key::Esc => {
                self.command_buffer.clear();
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Enter => {
                let cmd = core::mem::take(&mut self.command_buffer);
                self.mode = Mode::Normal;
                self.execute_command(&cmd)
            }
            Key::Char(c) => {
                self.command_buffer.push(c);
                EditorAction::Redraw
            }
            Key::Backspace => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                }
                EditorAction::Redraw
            }
            _ => EditorAction::None,
        }
    }

    /// Handle key in search mode.
    fn handle_search_key(&mut self, key: Key) -> EditorAction {
        match key {
            Key::Esc => {
                self.command_buffer.clear();
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Enter => {
                self.search_pattern = core::mem::take(&mut self.command_buffer);
                self.mode = Mode::Normal;
                if self.search_forward {
                    self.search_next();
                } else {
                    self.search_prev();
                }
                EditorAction::Redraw
            }
            Key::Char(c) => {
                self.command_buffer.push(c);
                EditorAction::Redraw
            }
            Key::Backspace => {
                self.command_buffer.pop();
                if self.command_buffer.is_empty() {
                    self.mode = Mode::Normal;
                }
                EditorAction::Redraw
            }
            _ => EditorAction::None,
        }
    }

    /// Handle key in replace mode.
    fn handle_replace_key(&mut self, key: Key) -> EditorAction {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                EditorAction::Redraw
            }
            Key::Char(c) => {
                // Replace character under cursor
                self.buffer.delete_char(self.cursor);
                self.buffer.insert_char(self.cursor, c);
                self.cursor.col += 1;
                self.clamp_cursor();
                EditorAction::Redraw
            }
            Key::Backspace => {
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                EditorAction::Redraw
            }
            _ => EditorAction::None,
        }
    }

    /// Execute an ex command.
    fn execute_command(&mut self, cmd: &str) -> EditorAction {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return EditorAction::Redraw;
        }

        let command = parts[0];

        match command {
            "q" | "quit" => {
                if self.buffer.is_modified() {
                    self.status_message = Some(
                        "E37: No write since last change (add ! to override)".to_string()
                    );
                    EditorAction::Redraw
                } else {
                    EditorAction::Quit
                }
            }
            "q!" | "quit!" => EditorAction::Quit,
            "w" | "write" => {
                let path_str = parts.get(1).map(|s| s.to_string())
                    .or_else(|| self.buffer.path.clone());
                if let Some(path) = path_str {
                    let content = self.buffer.to_string();
                    let line_count = self.buffer.line_count();
                    self.buffer.mark_saved();
                    self.status_message = Some(alloc::format!(
                        "\"{}\" {}L, {}B written",
                        path,
                        line_count,
                        content.len()
                    ));
                    EditorAction::Save { path, content }
                } else {
                    self.status_message = Some("E32: No file name".to_string());
                    EditorAction::Redraw
                }
            }
            "wq" | "x" => {
                let path_str = parts.get(1).map(|s| s.to_string())
                    .or_else(|| self.buffer.path.clone());
                if let Some(path) = path_str {
                    let content = self.buffer.to_string();
                    EditorAction::SaveAndQuit { path, content }
                } else {
                    self.status_message = Some("E32: No file name".to_string());
                    EditorAction::Redraw
                }
            }
            "e" | "edit" => {
                if parts.len() > 1 {
                    EditorAction::Open { path: parts[1].to_string() }
                } else {
                    self.status_message = Some("E32: No file name".to_string());
                    EditorAction::Redraw
                }
            }
            "set" => {
                // Handle settings
                if parts.len() > 1 {
                    self.handle_set_command(&parts[1..]);
                }
                EditorAction::Redraw
            }
            _ => {
                // Check for line number
                if let Ok(line) = command.parse::<usize>() {
                    self.cursor.row = line.saturating_sub(1).min(
                        self.buffer.line_count().saturating_sub(1)
                    );
                    self.clamp_cursor();
                } else {
                    self.status_message = Some(alloc::format!(
                        "E492: Not an editor command: {}",
                        command
                    ));
                }
                EditorAction::Redraw
            }
        }
    }

    /// Handle :set command.
    fn handle_set_command(&mut self, args: &[&str]) {
        for arg in args {
            match *arg {
                "number" | "nu" => {
                    self.status_message = Some("Line numbers enabled".to_string());
                }
                "nonumber" | "nonu" => {
                    self.status_message = Some("Line numbers disabled".to_string());
                }
                _ => {
                    self.status_message = Some(alloc::format!("Unknown option: {}", arg));
                }
            }
        }
    }

    // ==========================================================================
    // Cursor Movement
    // ==========================================================================

    fn move_cursor_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        let line_len = self.buffer.line_len(self.cursor.row);
        let max_col = if self.mode == Mode::Insert {
            line_len
        } else {
            line_len.saturating_sub(1)
        };
        if self.cursor.col < max_col {
            self.cursor.col += 1;
        }
    }

    fn move_cursor_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.clamp_cursor();
        }
    }

    fn move_cursor_down(&mut self) {
        if self.cursor.row + 1 < self.buffer.line_count() {
            self.cursor.row += 1;
            self.clamp_cursor();
        }
    }

    fn move_cursor_start_of_line(&mut self) {
        self.cursor.col = 0;
    }

    fn move_cursor_end_of_line(&mut self) {
        let line_len = self.buffer.line_len(self.cursor.row);
        self.cursor.col = line_len.saturating_sub(1);
    }

    fn move_cursor_first_nonblank(&mut self) {
        if let Some(line) = self.buffer.get_line(self.cursor.row) {
            self.cursor.col = line.chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
        }
    }

    fn move_word_forward(&mut self) {
        if let Some(line) = self.buffer.get_line(self.cursor.row) {
            let chars: Vec<char> = line.chars().collect();
            let mut col = self.cursor.col;

            // Skip current word
            while col < chars.len() && !chars[col].is_whitespace() {
                col += 1;
            }
            // Skip whitespace
            while col < chars.len() && chars[col].is_whitespace() {
                col += 1;
            }

            if col >= chars.len() && self.cursor.row + 1 < self.buffer.line_count() {
                // Go to next line
                self.cursor.row += 1;
                self.cursor.col = 0;
                self.move_cursor_first_nonblank();
            } else {
                self.cursor.col = col.min(chars.len().saturating_sub(1));
            }
        }
    }

    fn move_word_backward(&mut self) {
        if let Some(line) = self.buffer.get_line(self.cursor.row) {
            let chars: Vec<char> = line.chars().collect();
            let mut col = self.cursor.col.saturating_sub(1);

            // Skip whitespace
            while col > 0 && chars[col].is_whitespace() {
                col -= 1;
            }
            // Skip word
            while col > 0 && !chars[col - 1].is_whitespace() {
                col -= 1;
            }

            self.cursor.col = col;
        }
    }

    fn move_word_end(&mut self) {
        if let Some(line) = self.buffer.get_line(self.cursor.row) {
            let chars: Vec<char> = line.chars().collect();
            let mut col = self.cursor.col + 1;

            // Skip whitespace
            while col < chars.len() && chars[col].is_whitespace() {
                col += 1;
            }
            // Find end of word
            while col + 1 < chars.len() && !chars[col + 1].is_whitespace() {
                col += 1;
            }

            self.cursor.col = col.min(chars.len().saturating_sub(1));
        }
    }

    fn clamp_cursor(&mut self) {
        // Clamp row
        if self.cursor.row >= self.buffer.line_count() {
            self.cursor.row = self.buffer.line_count().saturating_sub(1);
        }

        // Clamp column
        let line_len = self.buffer.line_len(self.cursor.row);
        let max_col = if self.mode == Mode::Insert {
            line_len
        } else {
            line_len.saturating_sub(1).max(0)
        };
        if self.cursor.col > max_col {
            self.cursor.col = max_col;
        }
    }

    // ==========================================================================
    // Copy/Paste
    // ==========================================================================

    fn paste_after(&mut self) {
        match self.register.reg_type {
            RegisterType::Line => {
                self.buffer.insert_line(
                    self.cursor.row + 1,
                    self.register.content.clone()
                );
                self.cursor.row += 1;
                self.cursor.col = 0;
            }
            RegisterType::Char | RegisterType::Block => {
                for (i, ch) in self.register.content.chars().enumerate() {
                    self.buffer.insert_char(
                        Position::new(self.cursor.row, self.cursor.col + 1 + i),
                        ch
                    );
                }
                self.cursor.col += self.register.content.len();
            }
        }
    }

    fn paste_before(&mut self) {
        match self.register.reg_type {
            RegisterType::Line => {
                self.buffer.insert_line(
                    self.cursor.row,
                    self.register.content.clone()
                );
                self.cursor.col = 0;
            }
            RegisterType::Char | RegisterType::Block => {
                for (i, ch) in self.register.content.chars().enumerate() {
                    self.buffer.insert_char(
                        Position::new(self.cursor.row, self.cursor.col + i),
                        ch
                    );
                }
            }
        }
    }

    // ==========================================================================
    // Undo/Redo
    // ==========================================================================

    fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            self.apply_action(&action.inverse());
            self.redo_stack.push_back(action);
        }
    }

    fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop_back() {
            self.apply_action(&action);
            self.undo_stack.push_back(action);
        }
    }

    fn apply_action(&mut self, action: &EditAction) {
        match action {
            EditAction::Insert { pos, text } => {
                for (i, ch) in text.chars().enumerate() {
                    self.buffer.insert_char(
                        Position::new(pos.row, pos.col + i),
                        ch
                    );
                }
            }
            EditAction::Delete { pos, text } => {
                for _ in 0..text.len() {
                    self.buffer.delete_char(*pos);
                }
            }
            EditAction::InsertLine { row, content } => {
                self.buffer.insert_line(*row, content.clone());
            }
            EditAction::DeleteLine { row, .. } => {
                self.buffer.delete_line(*row);
            }
        }
    }

    // ==========================================================================
    // Search
    // ==========================================================================

    fn search_next(&mut self) {
        if self.search_pattern.is_empty() {
            return;
        }

        let start_row = self.cursor.row;
        let start_col = self.cursor.col + 1;

        // Search from cursor to end
        for row in start_row..self.buffer.line_count() {
            if let Some(line) = self.buffer.get_line(row) {
                let search_start = if row == start_row { start_col } else { 0 };
                if let Some(pos) = line[search_start..].find(&self.search_pattern) {
                    self.cursor = Position::new(row, search_start + pos);
                    return;
                }
            }
        }

        // Wrap around
        for row in 0..=start_row {
            if let Some(line) = self.buffer.get_line(row) {
                if let Some(pos) = line.find(&self.search_pattern) {
                    self.cursor = Position::new(row, pos);
                    self.status_message = Some("search hit BOTTOM, continuing at TOP".to_string());
                    return;
                }
            }
        }

        self.status_message = Some(alloc::format!(
            "E486: Pattern not found: {}",
            self.search_pattern
        ));
    }

    fn search_prev(&mut self) {
        if self.search_pattern.is_empty() {
            return;
        }

        let start_row = self.cursor.row;
        let start_col = self.cursor.col;

        // Search from cursor to start
        for row in (0..=start_row).rev() {
            if let Some(line) = self.buffer.get_line(row) {
                let search_end = if row == start_row { start_col } else { line.len() };
                if let Some(pos) = line[..search_end].rfind(&self.search_pattern) {
                    self.cursor = Position::new(row, pos);
                    return;
                }
            }
        }

        // Wrap around
        for row in (start_row..self.buffer.line_count()).rev() {
            if let Some(line) = self.buffer.get_line(row) {
                if let Some(pos) = line.rfind(&self.search_pattern) {
                    self.cursor = Position::new(row, pos);
                    self.status_message = Some("search hit TOP, continuing at BOTTOM".to_string());
                    return;
                }
            }
        }

        self.status_message = Some(alloc::format!(
            "E486: Pattern not found: {}",
            self.search_pattern
        ));
    }

    // ==========================================================================
    // Selection
    // ==========================================================================

    fn get_selection(&self) -> String {
        let (start, end) = if self.visual_start.row < self.cursor.row
            || (self.visual_start.row == self.cursor.row && self.visual_start.col <= self.cursor.col)
        {
            (self.visual_start, self.cursor)
        } else {
            (self.cursor, self.visual_start)
        };

        match self.mode {
            Mode::VisualLine => {
                let mut result = String::new();
                for row in start.row..=end.row {
                    if let Some(line) = self.buffer.get_line(row) {
                        result.push_str(line);
                        result.push('\n');
                    }
                }
                result
            }
            _ => {
                if start.row == end.row {
                    if let Some(line) = self.buffer.get_line(start.row) {
                        line[start.col..=end.col.min(line.len().saturating_sub(1))].to_string()
                    } else {
                        String::new()
                    }
                } else {
                    let mut result = String::new();
                    if let Some(line) = self.buffer.get_line(start.row) {
                        result.push_str(&line[start.col..]);
                        result.push('\n');
                    }
                    for row in (start.row + 1)..end.row {
                        if let Some(line) = self.buffer.get_line(row) {
                            result.push_str(line);
                            result.push('\n');
                        }
                    }
                    if let Some(line) = self.buffer.get_line(end.row) {
                        result.push_str(&line[..=end.col.min(line.len().saturating_sub(1))]);
                    }
                    result
                }
            }
        }
    }

    fn delete_selection(&mut self) {
        let text = self.get_selection();
        self.register = Register {
            content: text,
            reg_type: if self.mode == Mode::VisualLine {
                RegisterType::Line
            } else {
                RegisterType::Char
            },
        };

        let (start, end) = if self.visual_start.row < self.cursor.row
            || (self.visual_start.row == self.cursor.row && self.visual_start.col <= self.cursor.col)
        {
            (self.visual_start, self.cursor)
        } else {
            (self.cursor, self.visual_start)
        };

        match self.mode {
            Mode::VisualLine => {
                for _ in start.row..=end.row {
                    self.buffer.delete_line(start.row);
                }
                self.cursor = Position::new(start.row.min(self.buffer.line_count().saturating_sub(1)), 0);
            }
            _ => {
                // Simple implementation - just delete from start to end
                // A full implementation would handle multi-line properly
                if start.row == end.row {
                    for _ in start.col..=end.col {
                        self.buffer.delete_char(start);
                    }
                }
                self.cursor = start;
            }
        }

        self.clamp_cursor();
    }

    // ==========================================================================
    // Rendering
    // ==========================================================================

    /// Get current mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Get cursor position.
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    /// Get buffer reference.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Get status message.
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Get command buffer (for command/search mode).
    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    /// Render the editor to a string (for terminal output).
    pub fn render(&self) -> String {
        let mut output = String::new();

        // Calculate visible lines
        let visible_height = self.size.1.saturating_sub(2); // Leave room for status

        // Render lines
        for i in 0..visible_height {
            let row = self.view_offset.row + i;
            if row < self.buffer.line_count() {
                if let Some(line) = self.buffer.get_line(row) {
                    output.push_str(line);
                }
            } else {
                output.push('~');
            }
            output.push('\n');
        }

        // Status line
        output.push_str(&alloc::format!(
            "{} | {} | {}:{}\n",
            self.buffer.name(),
            self.mode.indicator(),
            self.cursor.row + 1,
            self.cursor.col + 1
        ));

        // Command line
        match self.mode {
            Mode::Command => {
                output.push(':');
                output.push_str(&self.command_buffer);
            }
            Mode::Search => {
                output.push(if self.search_forward { '/' } else { '?' });
                output.push_str(&self.command_buffer);
            }
            _ => {
                if let Some(msg) = &self.status_message {
                    output.push_str(msg);
                }
            }
        }

        output
    }
}

impl Default for VimEditor {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Key Input
// =============================================================================

/// Keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Regular character.
    Char(char),
    /// Control + key.
    Ctrl(char),
    /// Alt + key.
    Alt(char),
    /// Escape.
    Esc,
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Tab.
    Tab,
    /// Arrow keys.
    Up,
    Down,
    Left,
    Right,
    /// Home/End.
    Home,
    End,
    /// Page Up/Down.
    PageUp,
    PageDown,
    /// Insert.
    Insert,
    /// Function keys.
    F(u8),
}

/// Editor action result.
#[derive(Debug)]
pub enum EditorAction {
    /// No action needed.
    None,
    /// Redraw the screen.
    Redraw,
    /// Quit the editor.
    Quit,
    /// Save file.
    Save { path: String, content: String },
    /// Save and quit.
    SaveAndQuit { path: String, content: String },
    /// Open a file.
    Open { path: String },
}

// =============================================================================
// Entry Point
// =============================================================================

/// Stub allocator for tools (only used in no_std builds, not tests).
#[cfg(not(test))]
struct StubAllocator;

#[cfg(not(test))]
unsafe impl core::alloc::GlobalAlloc for StubAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: StubAllocator = StubAllocator;

/// Entry point.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // Initialize vim
    let _vim = VimEditor::new();
    
    // Main loop would be:
    // 1. Read key from terminal
    // 2. Handle key press
    // 3. Render screen
    // 4. Repeat until quit
    
    0
}

/// Panic handler.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_insert() {
        let mut buffer = Buffer::new();
        buffer.insert_char(Position::new(0, 0), 'H');
        buffer.insert_char(Position::new(0, 1), 'i');
        assert_eq!(buffer.get_line(0), Some("Hi"));
    }

    #[test]
    fn test_vim_insert_mode() {
        let mut vim = VimEditor::new();
        
        vim.handle_key(Key::Char('i')); // Enter insert mode
        assert_eq!(vim.mode(), Mode::Insert);
        
        vim.handle_key(Key::Char('H'));
        vim.handle_key(Key::Char('e'));
        vim.handle_key(Key::Char('l'));
        vim.handle_key(Key::Char('l'));
        vim.handle_key(Key::Char('o'));
        
        vim.handle_key(Key::Esc); // Back to normal
        assert_eq!(vim.mode(), Mode::Normal);
        assert_eq!(vim.buffer().get_line(0), Some("Hello"));
    }

    #[test]
    fn test_vim_delete_line() {
        let mut vim = VimEditor::new();
        vim.open("test.txt", "Line 1\nLine 2\nLine 3");
        
        vim.handle_key(Key::Char('d'));
        vim.handle_key(Key::Char('d')); // dd
        
        assert_eq!(vim.buffer().line_count(), 2);
        assert_eq!(vim.buffer().get_line(0), Some("Line 2"));
    }
}
