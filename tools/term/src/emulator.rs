//! # Terminal Emulation
//!
//! ANSI/VT100/xterm-compatible terminal emulation for S-TERM.
//! Provides full escape sequence parsing and screen management.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

// =============================================================================
// Terminal Dimensions
// =============================================================================

/// Terminal dimensions.
#[derive(Debug, Clone, Copy)]
pub struct TerminalSize {
    /// Number of columns.
    pub cols: u16,
    /// Number of rows.
    pub rows: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

// =============================================================================
// Character Attributes
// =============================================================================

/// Character display attributes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CharAttrs {
    /// Foreground color (0-255 for 256-color, or RGB).
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Bold.
    pub bold: bool,
    /// Dim/faint.
    pub dim: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Blink.
    pub blink: bool,
    /// Reverse video.
    pub reverse: bool,
    /// Hidden/invisible.
    pub hidden: bool,
    /// Strikethrough.
    pub strikethrough: bool,
}

/// Terminal color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Default color.
    Default,
    /// Basic 16 colors (0-7 normal, 8-15 bright).
    Basic(u8),
    /// 256-color palette.
    Palette(u8),
    /// True color RGB.
    Rgb(u8, u8, u8),
}

impl Default for Color {
    fn default() -> Self {
        Color::Default
    }
}

// =============================================================================
// Terminal Cell
// =============================================================================

/// A single cell in the terminal grid.
#[derive(Debug, Clone)]
pub struct Cell {
    /// Character (Unicode).
    pub ch: char,
    /// Display attributes.
    pub attrs: CharAttrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            attrs: CharAttrs::default(),
        }
    }
}

// =============================================================================
// Cursor State
// =============================================================================

/// Cursor state.
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    /// Column (0-indexed).
    pub col: u16,
    /// Row (0-indexed).
    pub row: u16,
    /// Cursor visible.
    pub visible: bool,
    /// Cursor style.
    pub style: CursorStyle,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            visible: true,
            style: CursorStyle::Block,
        }
    }
}

/// Cursor style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

// =============================================================================
// Screen Buffer
// =============================================================================

/// Screen buffer containing the terminal grid.
pub struct ScreenBuffer {
    /// Cell grid (row-major order).
    cells: Vec<Cell>,
    /// Terminal size.
    size: TerminalSize,
    /// Scrollback buffer (oldest lines first).
    scrollback: VecDeque<Vec<Cell>>,
    /// Maximum scrollback lines.
    max_scrollback: usize,
}

impl ScreenBuffer {
    /// Creates a new screen buffer.
    pub fn new(size: TerminalSize, max_scrollback: usize) -> Self {
        let cells = (0..(size.cols as usize * size.rows as usize))
            .map(|_| Cell::default())
            .collect();
        
        Self {
            cells,
            size,
            scrollback: VecDeque::new(),
            max_scrollback,
        }
    }

    /// Gets the terminal size.
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Resizes the buffer.
    pub fn resize(&mut self, new_size: TerminalSize) {
        let mut new_cells = Vec::with_capacity(new_size.cols as usize * new_size.rows as usize);
        
        for row in 0..new_size.rows as usize {
            for col in 0..new_size.cols as usize {
                if row < self.size.rows as usize && col < self.size.cols as usize {
                    new_cells.push(self.get(col as u16, row as u16).clone());
                } else {
                    new_cells.push(Cell::default());
                }
            }
        }
        
        self.cells = new_cells;
        self.size = new_size;
    }

    /// Gets a cell at position.
    pub fn get(&self, col: u16, row: u16) -> Cell {
        let idx = row as usize * self.size.cols as usize + col as usize;
        self.cells.get(idx).cloned().unwrap_or_else(|| Cell { ch: ' ', attrs: CharAttrs::default() })
    }

    /// Gets a mutable cell at position.
    pub fn get_mut(&mut self, col: u16, row: u16) -> Option<&mut Cell> {
        let idx = row as usize * self.size.cols as usize + col as usize;
        self.cells.get_mut(idx)
    }

    /// Sets a cell at position.
    pub fn set(&mut self, col: u16, row: u16, cell: Cell) {
        if col < self.size.cols && row < self.size.rows {
            let idx = row as usize * self.size.cols as usize + col as usize;
            self.cells[idx] = cell;
        }
    }

    /// Clears a range of cells.
    pub fn clear_range(&mut self, start_col: u16, start_row: u16, end_col: u16, end_row: u16, attrs: CharAttrs) {
        for row in start_row..=end_row.min(self.size.rows - 1) {
            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row { end_col } else { self.size.cols - 1 };
            
            for col in col_start..=col_end.min(self.size.cols - 1) {
                self.set(col, row, Cell { ch: ' ', attrs });
            }
        }
    }

    /// Clears the entire screen.
    pub fn clear(&mut self, attrs: CharAttrs) {
        for cell in &mut self.cells {
            cell.ch = ' ';
            cell.attrs = attrs;
        }
    }

    /// Scrolls the screen up by n lines.
    pub fn scroll_up(&mut self, n: u16, attrs: CharAttrs) {
        let n = n as usize;
        let cols = self.size.cols as usize;
        let rows = self.size.rows as usize;
        
        // Save lines to scrollback
        for row in 0..n.min(rows) {
            let start = row * cols;
            let end = start + cols;
            let line: Vec<Cell> = self.cells[start..end].to_vec();
            self.scrollback.push_back(line);
        }
        
        // Trim scrollback if needed
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
        }
        
        // Shift lines up
        let n = n.min(rows);
        self.cells.drain(0..n * cols);
        
        // Add empty lines at bottom
        for _ in 0..n * cols {
            self.cells.push(Cell { ch: ' ', attrs });
        }
    }

    /// Scrolls the screen down by n lines.
    pub fn scroll_down(&mut self, n: u16, attrs: CharAttrs) {
        let n = n as usize;
        let cols = self.size.cols as usize;
        let rows = self.size.rows as usize;
        
        // Remove lines from bottom
        let n = n.min(rows);
        self.cells.truncate((rows - n) * cols);
        
        // Insert empty lines at top
        let mut new_cells: Vec<Cell> = (0..n * cols)
            .map(|_| Cell { ch: ' ', attrs })
            .collect();
        new_cells.append(&mut self.cells);
        self.cells = new_cells;
    }

    /// Gets the scrollback line count.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Gets a scrollback line.
    pub fn get_scrollback(&self, line: usize) -> Option<&[Cell]> {
        self.scrollback.get(line).map(|v| v.as_slice())
    }
}

// =============================================================================
// ANSI Parser State
// =============================================================================

/// Parser state for ANSI escape sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParserState {
    /// Normal text processing.
    Ground,
    /// Just received ESC.
    Escape,
    /// Processing CSI sequence (ESC [).
    Csi,
    /// Processing CSI parameters.
    CsiParam,
    /// Processing OSC sequence (ESC ]).
    Osc,
    /// Processing DCS sequence (ESC P).
    Dcs,
}

/// ANSI escape sequence parser.
pub struct AnsiParser {
    state: ParserState,
    params: Vec<u16>,
    intermediate: Vec<char>,
    osc_string: String,
}

impl AnsiParser {
    /// Creates a new parser.
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
            params: Vec::new(),
            intermediate: Vec::new(),
            osc_string: String::new(),
        }
    }

    /// Resets the parser to ground state.
    fn reset(&mut self) {
        self.state = ParserState::Ground;
        self.params.clear();
        self.intermediate.clear();
        self.osc_string.clear();
    }

    /// Parses input and returns actions.
    pub fn parse(&mut self, input: &str) -> Vec<TerminalAction> {
        let mut actions = Vec::new();
        
        for ch in input.chars() {
            match self.state {
                ParserState::Ground => {
                    match ch {
                        '\x1b' => self.state = ParserState::Escape,
                        '\n' => actions.push(TerminalAction::LineFeed),
                        '\r' => actions.push(TerminalAction::CarriageReturn),
                        '\t' => actions.push(TerminalAction::Tab),
                        '\x08' => actions.push(TerminalAction::Backspace),
                        '\x07' => actions.push(TerminalAction::Bell),
                        c if c >= ' ' => actions.push(TerminalAction::Print(c)),
                        _ => {} // Ignore other control chars
                    }
                }
                ParserState::Escape => {
                    match ch {
                        '[' => {
                            self.state = ParserState::Csi;
                            self.params.clear();
                            self.params.push(0);
                        }
                        ']' => {
                            self.state = ParserState::Osc;
                            self.osc_string.clear();
                        }
                        'P' => self.state = ParserState::Dcs,
                        'D' => {
                            actions.push(TerminalAction::Index);
                            self.reset();
                        }
                        'M' => {
                            actions.push(TerminalAction::ReverseIndex);
                            self.reset();
                        }
                        'E' => {
                            actions.push(TerminalAction::NextLine);
                            self.reset();
                        }
                        'c' => {
                            actions.push(TerminalAction::Reset);
                            self.reset();
                        }
                        '7' => {
                            actions.push(TerminalAction::SaveCursor);
                            self.reset();
                        }
                        '8' => {
                            actions.push(TerminalAction::RestoreCursor);
                            self.reset();
                        }
                        _ => self.reset(),
                    }
                }
                ParserState::Csi | ParserState::CsiParam => {
                    match ch {
                        '0'..='9' => {
                            self.state = ParserState::CsiParam;
                            if let Some(last) = self.params.last_mut() {
                                *last = last.saturating_mul(10).saturating_add((ch as u16) - ('0' as u16));
                            }
                        }
                        ';' => {
                            self.state = ParserState::CsiParam;
                            self.params.push(0);
                        }
                        ' '..='/' => {
                            self.intermediate.push(ch);
                        }
                        '@'..='~' => {
                            // Final byte
                            let action = self.dispatch_csi(ch);
                            if let Some(a) = action {
                                actions.push(a);
                            }
                            self.reset();
                        }
                        _ => self.reset(),
                    }
                }
                ParserState::Osc => {
                    match ch {
                        '\x07' | '\x1b' => {
                            // OSC terminator
                            let action = self.dispatch_osc();
                            if let Some(a) = action {
                                actions.push(a);
                            }
                            self.reset();
                        }
                        _ => self.osc_string.push(ch),
                    }
                }
                ParserState::Dcs => {
                    // DCS sequences are not commonly needed
                    if ch == '\x1b' || ch == '\x07' {
                        self.reset();
                    }
                }
            }
        }
        
        actions
    }

    /// Dispatches a CSI sequence.
    fn dispatch_csi(&self, final_byte: char) -> Option<TerminalAction> {
        let p1 = *self.params.first().unwrap_or(&0);
        let p2 = *self.params.get(1).unwrap_or(&0);
        
        match final_byte {
            'A' => Some(TerminalAction::CursorUp(p1.max(1))),
            'B' => Some(TerminalAction::CursorDown(p1.max(1))),
            'C' => Some(TerminalAction::CursorForward(p1.max(1))),
            'D' => Some(TerminalAction::CursorBack(p1.max(1))),
            'E' => Some(TerminalAction::CursorNextLine(p1.max(1))),
            'F' => Some(TerminalAction::CursorPreviousLine(p1.max(1))),
            'G' => Some(TerminalAction::CursorColumn(p1.max(1))),
            'H' | 'f' => Some(TerminalAction::CursorPosition(p2.max(1), p1.max(1))),
            'J' => Some(TerminalAction::EraseDisplay(p1)),
            'K' => Some(TerminalAction::EraseLine(p1)),
            'L' => Some(TerminalAction::InsertLines(p1.max(1))),
            'M' => Some(TerminalAction::DeleteLines(p1.max(1))),
            'P' => Some(TerminalAction::DeleteChars(p1.max(1))),
            'S' => Some(TerminalAction::ScrollUp(p1.max(1))),
            'T' => Some(TerminalAction::ScrollDown(p1.max(1))),
            '@' => Some(TerminalAction::InsertChars(p1.max(1))),
            'm' => Some(TerminalAction::SetGraphicsRendition(self.params.clone())),
            'r' => Some(TerminalAction::SetScrollRegion(p1, p2)),
            's' => Some(TerminalAction::SaveCursor),
            'u' => Some(TerminalAction::RestoreCursor),
            'h' => Some(TerminalAction::SetMode(self.params.clone(), true)),
            'l' => Some(TerminalAction::SetMode(self.params.clone(), false)),
            'n' => Some(TerminalAction::DeviceStatusReport(p1)),
            'c' => Some(TerminalAction::DeviceAttributes),
            _ => None,
        }
    }

    /// Dispatches an OSC sequence.
    fn dispatch_osc(&self) -> Option<TerminalAction> {
        // OSC 0;text - Set window title
        // OSC 1;text - Set icon name
        // OSC 2;text - Set window title
        if let Some(idx) = self.osc_string.find(';') {
            let cmd: u16 = self.osc_string[..idx].parse().unwrap_or(0);
            let text = &self.osc_string[idx + 1..];
            
            match cmd {
                0 | 2 => Some(TerminalAction::SetTitle(text.into())),
                _ => None,
            }
        } else {
            None
        }
    }
}

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Terminal Actions
// =============================================================================

/// Actions generated by the parser.
#[derive(Debug, Clone)]
pub enum TerminalAction {
    /// Print a character.
    Print(char),
    /// Carriage return.
    CarriageReturn,
    /// Line feed.
    LineFeed,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Bell.
    Bell,
    /// Cursor up.
    CursorUp(u16),
    /// Cursor down.
    CursorDown(u16),
    /// Cursor forward.
    CursorForward(u16),
    /// Cursor back.
    CursorBack(u16),
    /// Cursor to next line.
    CursorNextLine(u16),
    /// Cursor to previous line.
    CursorPreviousLine(u16),
    /// Cursor to column.
    CursorColumn(u16),
    /// Cursor to position (col, row, 1-indexed).
    CursorPosition(u16, u16),
    /// Erase display (0=below, 1=above, 2=all, 3=scrollback).
    EraseDisplay(u16),
    /// Erase line (0=right, 1=left, 2=all).
    EraseLine(u16),
    /// Insert lines.
    InsertLines(u16),
    /// Delete lines.
    DeleteLines(u16),
    /// Insert characters.
    InsertChars(u16),
    /// Delete characters.
    DeleteChars(u16),
    /// Scroll up.
    ScrollUp(u16),
    /// Scroll down.
    ScrollDown(u16),
    /// Set graphics rendition (SGR).
    SetGraphicsRendition(Vec<u16>),
    /// Set scroll region.
    SetScrollRegion(u16, u16),
    /// Save cursor position.
    SaveCursor,
    /// Restore cursor position.
    RestoreCursor,
    /// Set mode.
    SetMode(Vec<u16>, bool),
    /// Device status report.
    DeviceStatusReport(u16),
    /// Device attributes query.
    DeviceAttributes,
    /// Set window title.
    SetTitle(String),
    /// Index (move cursor down, scroll if at bottom).
    Index,
    /// Reverse index (move cursor up, scroll if at top).
    ReverseIndex,
    /// Next line (CR + LF).
    NextLine,
    /// Reset terminal.
    Reset,
}

// =============================================================================
// Terminal Emulator
// =============================================================================

/// Full terminal emulator.
pub struct Terminal {
    /// Screen buffer.
    screen: ScreenBuffer,
    /// Alternate screen buffer.
    alt_screen: Option<ScreenBuffer>,
    /// Cursor state.
    cursor: Cursor,
    /// Saved cursor state.
    saved_cursor: Option<Cursor>,
    /// Current character attributes.
    current_attrs: CharAttrs,
    /// ANSI parser.
    parser: AnsiParser,
    /// Window title.
    title: String,
    /// Scroll region (top, bottom), 0-indexed.
    scroll_region: Option<(u16, u16)>,
    /// Tab stops.
    tab_stops: Vec<u16>,
    /// Application cursor keys mode.
    app_cursor_keys: bool,
    /// Application keypad mode.
    app_keypad: bool,
    /// Origin mode.
    origin_mode: bool,
    /// Auto-wrap mode.
    auto_wrap: bool,
    /// Insert mode.
    insert_mode: bool,
}

impl Terminal {
    /// Creates a new terminal.
    pub fn new(size: TerminalSize) -> Self {
        let mut tab_stops = Vec::new();
        for i in (0..size.cols).step_by(8) {
            tab_stops.push(i);
        }
        
        Self {
            screen: ScreenBuffer::new(size, 10000),
            alt_screen: None,
            cursor: Cursor::default(),
            saved_cursor: None,
            current_attrs: CharAttrs::default(),
            parser: AnsiParser::new(),
            title: String::from("S-TERM"),
            scroll_region: None,
            tab_stops,
            app_cursor_keys: false,
            app_keypad: false,
            origin_mode: false,
            auto_wrap: true,
            insert_mode: false,
        }
    }

    /// Gets the terminal size.
    pub fn size(&self) -> TerminalSize {
        self.screen.size()
    }

    /// Resizes the terminal.
    pub fn resize(&mut self, size: TerminalSize) {
        self.screen.resize(size);
        if let Some(ref mut alt) = self.alt_screen {
            alt.resize(size);
        }
        
        // Update tab stops
        self.tab_stops.clear();
        for i in (0..size.cols).step_by(8) {
            self.tab_stops.push(i);
        }
        
        // Clamp cursor
        self.cursor.col = self.cursor.col.min(size.cols - 1);
        self.cursor.row = self.cursor.row.min(size.rows - 1);
    }

    /// Processes input data.
    pub fn process(&mut self, data: &str) {
        let actions = self.parser.parse(data);
        for action in actions {
            self.execute(action);
        }
    }

    /// Executes a terminal action.
    fn execute(&mut self, action: TerminalAction) {
        let size = self.screen.size();
        
        match action {
            TerminalAction::Print(ch) => {
                if self.insert_mode {
                    // Shift characters right
                    for col in (self.cursor.col + 1..size.cols).rev() {
                        let prev = self.screen.get(col - 1, self.cursor.row).clone();
                        self.screen.set(col, self.cursor.row, prev);
                    }
                }
                
                self.screen.set(self.cursor.col, self.cursor.row, Cell {
                    ch,
                    attrs: self.current_attrs,
                });
                
                if self.cursor.col < size.cols - 1 {
                    self.cursor.col += 1;
                } else if self.auto_wrap {
                    self.cursor.col = 0;
                    if self.cursor.row < size.rows - 1 {
                        self.cursor.row += 1;
                    } else {
                        self.screen.scroll_up(1, self.current_attrs);
                    }
                }
            }
            TerminalAction::CarriageReturn => {
                self.cursor.col = 0;
            }
            TerminalAction::LineFeed => {
                if self.cursor.row < size.rows - 1 {
                    self.cursor.row += 1;
                } else {
                    self.screen.scroll_up(1, self.current_attrs);
                }
            }
            TerminalAction::Tab => {
                // Find next tab stop
                let next_tab = self.tab_stops.iter()
                    .find(|&&t| t > self.cursor.col)
                    .copied()
                    .unwrap_or(size.cols - 1);
                self.cursor.col = next_tab.min(size.cols - 1);
            }
            TerminalAction::Backspace => {
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
            }
            TerminalAction::Bell => {
                // Would trigger audio/visual bell
            }
            TerminalAction::CursorUp(n) => {
                self.cursor.row = self.cursor.row.saturating_sub(n);
            }
            TerminalAction::CursorDown(n) => {
                self.cursor.row = (self.cursor.row + n).min(size.rows - 1);
            }
            TerminalAction::CursorForward(n) => {
                self.cursor.col = (self.cursor.col + n).min(size.cols - 1);
            }
            TerminalAction::CursorBack(n) => {
                self.cursor.col = self.cursor.col.saturating_sub(n);
            }
            TerminalAction::CursorNextLine(n) => {
                self.cursor.col = 0;
                self.cursor.row = (self.cursor.row + n).min(size.rows - 1);
            }
            TerminalAction::CursorPreviousLine(n) => {
                self.cursor.col = 0;
                self.cursor.row = self.cursor.row.saturating_sub(n);
            }
            TerminalAction::CursorColumn(col) => {
                self.cursor.col = (col.saturating_sub(1)).min(size.cols - 1);
            }
            TerminalAction::CursorPosition(col, row) => {
                self.cursor.col = (col.saturating_sub(1)).min(size.cols - 1);
                self.cursor.row = (row.saturating_sub(1)).min(size.rows - 1);
            }
            TerminalAction::EraseDisplay(mode) => {
                match mode {
                    0 => {
                        // Erase from cursor to end
                        self.screen.clear_range(
                            self.cursor.col, self.cursor.row,
                            size.cols - 1, size.rows - 1,
                            self.current_attrs,
                        );
                    }
                    1 => {
                        // Erase from start to cursor
                        self.screen.clear_range(
                            0, 0,
                            self.cursor.col, self.cursor.row,
                            self.current_attrs,
                        );
                    }
                    2 | 3 => {
                        // Erase entire screen
                        self.screen.clear(self.current_attrs);
                    }
                    _ => {}
                }
            }
            TerminalAction::EraseLine(mode) => {
                match mode {
                    0 => {
                        // Erase from cursor to end of line
                        self.screen.clear_range(
                            self.cursor.col, self.cursor.row,
                            size.cols - 1, self.cursor.row,
                            self.current_attrs,
                        );
                    }
                    1 => {
                        // Erase from start of line to cursor
                        self.screen.clear_range(
                            0, self.cursor.row,
                            self.cursor.col, self.cursor.row,
                            self.current_attrs,
                        );
                    }
                    2 => {
                        // Erase entire line
                        self.screen.clear_range(
                            0, self.cursor.row,
                            size.cols - 1, self.cursor.row,
                            self.current_attrs,
                        );
                    }
                    _ => {}
                }
            }
            TerminalAction::ScrollUp(n) => {
                self.screen.scroll_up(n, self.current_attrs);
            }
            TerminalAction::ScrollDown(n) => {
                self.screen.scroll_down(n, self.current_attrs);
            }
            TerminalAction::SetGraphicsRendition(params) => {
                self.process_sgr(&params);
            }
            TerminalAction::SaveCursor => {
                self.saved_cursor = Some(self.cursor);
            }
            TerminalAction::RestoreCursor => {
                if let Some(saved) = self.saved_cursor {
                    self.cursor = saved;
                }
            }
            TerminalAction::SetTitle(title) => {
                self.title = title;
            }
            TerminalAction::Index => {
                if self.cursor.row < size.rows - 1 {
                    self.cursor.row += 1;
                } else {
                    self.screen.scroll_up(1, self.current_attrs);
                }
            }
            TerminalAction::ReverseIndex => {
                if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                } else {
                    self.screen.scroll_down(1, self.current_attrs);
                }
            }
            TerminalAction::NextLine => {
                self.cursor.col = 0;
                if self.cursor.row < size.rows - 1 {
                    self.cursor.row += 1;
                } else {
                    self.screen.scroll_up(1, self.current_attrs);
                }
            }
            TerminalAction::Reset => {
                self.cursor = Cursor::default();
                self.current_attrs = CharAttrs::default();
                self.screen.clear(CharAttrs::default());
            }
            TerminalAction::InsertLines(n) => {
                // Insert n blank lines at cursor row
                for _ in 0..n {
                    self.screen.scroll_down(1, self.current_attrs);
                }
            }
            TerminalAction::DeleteLines(n) => {
                // Delete n lines at cursor row
                for _ in 0..n {
                    self.screen.scroll_up(1, self.current_attrs);
                }
            }
            TerminalAction::InsertChars(n) => {
                // Insert n blank characters at cursor
                for _ in 0..n {
                    for col in (self.cursor.col + 1..size.cols).rev() {
                        let prev = self.screen.get(col - 1, self.cursor.row).clone();
                        self.screen.set(col, self.cursor.row, prev);
                    }
                    self.screen.set(self.cursor.col, self.cursor.row, Cell {
                        ch: ' ',
                        attrs: self.current_attrs,
                    });
                }
            }
            TerminalAction::DeleteChars(n) => {
                // Delete n characters at cursor
                for _ in 0..n {
                    for col in self.cursor.col..size.cols - 1 {
                        let next = self.screen.get(col + 1, self.cursor.row).clone();
                        self.screen.set(col, self.cursor.row, next);
                    }
                    self.screen.set(size.cols - 1, self.cursor.row, Cell {
                        ch: ' ',
                        attrs: self.current_attrs,
                    });
                }
            }
            TerminalAction::SetScrollRegion(top, bottom) => {
                let top = top.saturating_sub(1);
                let bottom = if bottom == 0 { size.rows - 1 } else { bottom.saturating_sub(1) };
                self.scroll_region = Some((top, bottom.min(size.rows - 1)));
            }
            TerminalAction::SetMode(params, enable) => {
                for param in params {
                    match param {
                        1 => self.app_cursor_keys = enable,
                        4 => self.insert_mode = enable,
                        7 => self.auto_wrap = enable,
                        25 => self.cursor.visible = enable,
                        1049 => {
                            // Alternate screen buffer
                            if enable {
                                let alt = ScreenBuffer::new(size, 0);
                                self.alt_screen = Some(core::mem::replace(&mut self.screen, alt));
                            } else if let Some(main) = self.alt_screen.take() {
                                self.screen = main;
                            }
                        }
                        _ => {}
                    }
                }
            }
            TerminalAction::DeviceStatusReport(_) | TerminalAction::DeviceAttributes => {
                // Would respond via PTY
            }
        }
    }

    /// Processes SGR (Select Graphic Rendition) parameters.
    fn process_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.current_attrs = CharAttrs::default(),
                1 => self.current_attrs.bold = true,
                2 => self.current_attrs.dim = true,
                3 => self.current_attrs.italic = true,
                4 => self.current_attrs.underline = true,
                5 => self.current_attrs.blink = true,
                7 => self.current_attrs.reverse = true,
                8 => self.current_attrs.hidden = true,
                9 => self.current_attrs.strikethrough = true,
                22 => { self.current_attrs.bold = false; self.current_attrs.dim = false; }
                23 => self.current_attrs.italic = false,
                24 => self.current_attrs.underline = false,
                25 => self.current_attrs.blink = false,
                27 => self.current_attrs.reverse = false,
                28 => self.current_attrs.hidden = false,
                29 => self.current_attrs.strikethrough = false,
                30..=37 => self.current_attrs.fg = Color::Basic((params[i] - 30) as u8),
                38 => {
                    // Extended foreground color
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_attrs.fg = Color::Palette(params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.current_attrs.fg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.current_attrs.fg = Color::Default,
                40..=47 => self.current_attrs.bg = Color::Basic((params[i] - 40) as u8),
                48 => {
                    // Extended background color
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_attrs.bg = Color::Palette(params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.current_attrs.bg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.current_attrs.bg = Color::Default,
                90..=97 => self.current_attrs.fg = Color::Basic((params[i] - 90 + 8) as u8),
                100..=107 => self.current_attrs.bg = Color::Basic((params[i] - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }

    /// Gets the current cursor position.
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Gets the screen buffer.
    pub fn screen(&self) -> &ScreenBuffer {
        &self.screen
    }

    /// Gets the window title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Gets current attributes.
    pub fn current_attrs(&self) -> CharAttrs {
        self.current_attrs
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new(TerminalSize::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_basic() {
        let mut parser = AnsiParser::new();
        let actions = parser.parse("Hello");
        assert_eq!(actions.len(), 5);
    }

    #[test]
    fn test_parser_csi() {
        let mut parser = AnsiParser::new();
        let actions = parser.parse("\x1b[2J");
        assert!(matches!(actions.get(0), Some(TerminalAction::EraseDisplay(2))));
    }

    #[test]
    fn test_terminal_print() {
        let mut term = Terminal::new(TerminalSize { cols: 80, rows: 24 });
        term.process("Hello");
        assert_eq!(term.cursor().col, 5);
        assert_eq!(term.screen().get(0, 0).ch, 'H');
        assert_eq!(term.screen().get(4, 0).ch, 'o');
    }

    #[test]
    fn test_terminal_cursor_move() {
        let mut term = Terminal::new(TerminalSize { cols: 80, rows: 24 });
        term.process("\x1b[10;20H"); // Move to row 10, col 20
        assert_eq!(term.cursor().row, 9);  // 0-indexed
        assert_eq!(term.cursor().col, 19);
    }
}
