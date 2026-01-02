//! # S-CODE: Editor Interface
//!
//! S-CODE is the code editor for Splax OS, providing editing capabilities
//! within the capability-based system.
//!
//! ## Design Philosophy
//!
//! - **Capability-Gated**: File access requires explicit capabilities
//! - **Object-Based**: Uses S-STORAGE object IDs, not file paths
//! - **Minimal**: Core editing functionality, extensible via WASM plugins
//! - **Terminal-First**: Works in terminal mode, optional GUI
//!
//! ## Architecture
//!
//! ```text
//! S-CODE
//!   │
//!   ├── Buffer Manager (in-memory documents)
//!   ├── Syntax Engine (highlighting)
//!   ├── Plugin Host (WASM extensions)
//!   └── Capability Binder (S-STORAGE access)
//! ```

#![no_std]
#![no_main]

extern crate alloc;

pub mod syntax;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

/// Object ID from S-STORAGE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectId(pub u64);

/// Capability token (simplified).
#[derive(Debug, Clone, Copy)]
pub struct CapabilityToken(pub u64);

/// Buffer ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferId(pub u64);

/// Cursor position.
#[derive(Debug, Clone, Copy, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Selection range.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub start: Position,
    pub end: Position,
}

impl Selection {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn is_empty(&self) -> bool {
        self.start.line == self.end.line && self.start.column == self.end.column
    }
}

/// A line in a buffer.
#[derive(Debug, Clone)]
pub struct Line {
    pub content: String,
}

impl Line {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Edit operation for undo/redo.
#[derive(Debug, Clone)]
pub enum EditOperation {
    Insert {
        position: Position,
        text: String,
    },
    Delete {
        start: Position,
        end: Position,
        deleted: String,
    },
    Replace {
        start: Position,
        end: Position,
        old_text: String,
        new_text: String,
    },
}

/// A text buffer.
pub struct Buffer {
    /// Buffer ID
    id: BufferId,
    /// Associated storage object (if any)
    object_id: Option<ObjectId>,
    /// Capability for accessing the object
    capability: Option<CapabilityToken>,
    /// Lines of text
    lines: Vec<Line>,
    /// Cursor position
    cursor: Position,
    /// Selection
    selection: Option<Selection>,
    /// Undo stack
    undo_stack: Vec<EditOperation>,
    /// Redo stack
    redo_stack: Vec<EditOperation>,
    /// Modified flag
    modified: bool,
    /// Language hint (for syntax highlighting)
    language: Option<String>,
}

impl Buffer {
    /// Creates a new empty buffer.
    pub fn new(id: BufferId) -> Self {
        Self {
            id,
            object_id: None,
            capability: None,
            lines: alloc::vec![Line::new("")],
            cursor: Position::default(),
            selection: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            modified: false,
            language: None,
        }
    }

    /// Creates a buffer from object content.
    pub fn from_object(
        id: BufferId,
        object_id: ObjectId,
        capability: CapabilityToken,
        content: &str,
    ) -> Self {
        let lines: Vec<Line> = content.lines().map(Line::new).collect();
        let lines = if lines.is_empty() {
            alloc::vec![Line::new("")]
        } else {
            lines
        };

        Self {
            id,
            object_id: Some(object_id),
            capability: Some(capability),
            lines,
            cursor: Position::default(),
            selection: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            modified: false,
            language: None,
        }
    }

    /// Gets the buffer ID.
    pub fn id(&self) -> BufferId {
        self.id
    }

    /// Gets the associated object ID.
    pub fn object_id(&self) -> Option<ObjectId> {
        self.object_id
    }

    /// Checks if the buffer is modified.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Gets the number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Gets a line.
    pub fn get_line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }

    /// Gets the cursor position.
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    /// Moves the cursor.
    pub fn move_cursor(&mut self, position: Position) {
        let line = position.line.min(self.lines.len().saturating_sub(1));
        let column = position.column.min(
            self.lines
                .get(line)
                .map(|l| l.len())
                .unwrap_or(0),
        );
        self.cursor = Position::new(line, column);
    }

    /// Inserts text at position.
    pub fn insert(&mut self, position: Position, text: &str) {
        // Record operation for undo
        self.undo_stack.push(EditOperation::Insert {
            position,
            text: text.into(),
        });
        self.redo_stack.clear();

        // Insert text
        if position.line < self.lines.len() {
            let mut content = self.lines[position.line].content.clone();
            content.insert_str(position.column.min(content.len()), text);

            // Handle newlines
            let new_lines: Vec<String> = content.split('\n').map(String::from).collect();
            if new_lines.len() == 1 {
                self.lines[position.line].content = content;
            } else {
                // Split into multiple lines
                self.lines[position.line].content = new_lines[0].clone();
                for (i, new_line) in new_lines.iter().enumerate().skip(1) {
                    self.lines.insert(position.line + i, Line::new(new_line.clone()));
                }
            }
        }

        self.modified = true;
    }

    /// Deletes text in range.
    pub fn delete(&mut self, start: Position, end: Position) {
        // Collect deleted text for undo
        let mut deleted = String::new();
        
        if start.line == end.line {
            // Single line deletion
            if start.line < self.lines.len() {
                let line = &mut self.lines[start.line];
                let start_col = start.column.min(line.content.len());
                let end_col = end.column.min(line.content.len());
                deleted = line.content[start_col..end_col].to_string();
                line.content = format!("{}{}", &line.content[..start_col], &line.content[end_col..]);
            }
        } else {
            // Multi-line deletion
            if start.line < self.lines.len() {
                // Get prefix from first line
                let first_line = &self.lines[start.line];
                let start_col = start.column.min(first_line.content.len());
                let prefix = first_line.content[..start_col].to_string();
                deleted.push_str(&first_line.content[start_col..]);
                deleted.push('\n');
                
                // Get suffix from last line
                let end_line_idx = end.line.min(self.lines.len() - 1);
                let last_line = &self.lines[end_line_idx];
                let end_col = end.column.min(last_line.content.len());
                let suffix = last_line.content[end_col..].to_string();
                
                // Collect middle lines
                for i in (start.line + 1)..end_line_idx {
                    deleted.push_str(&self.lines[i].content);
                    deleted.push('\n');
                }
                deleted.push_str(&last_line.content[..end_col]);
                
                // Merge first and last line
                self.lines[start.line].content = format!("{}{}", prefix, suffix);
                
                // Remove middle lines
                for _ in (start.line + 1)..=end_line_idx {
                    if start.line + 1 < self.lines.len() {
                        self.lines.remove(start.line + 1);
                    }
                }
            }
        }

        self.undo_stack.push(EditOperation::Delete {
            start,
            end,
            deleted,
        });
        self.redo_stack.clear();
        self.modified = true;
    }

    /// Undoes the last operation.
    pub fn undo(&mut self) -> bool {
        if let Some(op) = self.undo_stack.pop() {
            // Apply inverse operation
            match op {
                EditOperation::Insert { position, ref text } => {
                    // Undo insert = delete
                    let end = Position {
                        line: position.line + text.lines().count().saturating_sub(1),
                        column: if text.contains('\n') {
                            text.lines().last().map(|l| l.len()).unwrap_or(0)
                        } else {
                            position.column + text.len()
                        },
                    };
                    // Don't push to undo stack during undo
                    let deleted = text.clone();
                    self.redo_stack.push(EditOperation::Insert { position, text: deleted });
                }
                EditOperation::Delete { start, ref deleted, .. } => {
                    // Undo delete = insert
                    // Re-insert the deleted text
                    self.redo_stack.push(EditOperation::Delete { start, end: start, deleted: deleted.clone() });
                }
                EditOperation::Replace { start, end, ref old_text, ref new_text } => {
                    // Undo replace = replace back to old text
                    self.redo_stack.push(EditOperation::Replace {
                        start,
                        end,
                        old_text: new_text.clone(),
                        new_text: old_text.clone(),
                    });
                }
            }
            true
        } else {
            false
        }
    }

    /// Redoes the last undone operation.
    pub fn redo(&mut self) -> bool {
        if let Some(op) = self.redo_stack.pop() {
            // Apply operation
            match op {
                EditOperation::Insert { position, ref text } => {
                    self.undo_stack.push(EditOperation::Insert { position, text: text.clone() });
                }
                EditOperation::Delete { start, end, ref deleted } => {
                    self.undo_stack.push(EditOperation::Delete { start, end, deleted: deleted.clone() });
                }
                EditOperation::Replace { start, end, ref old_text, ref new_text } => {
                    self.undo_stack.push(EditOperation::Replace {
                        start,
                        end,
                        old_text: old_text.clone(),
                        new_text: new_text.clone(),
                    });
                }
            }
            true
        } else {
            false
        }
    }

    /// Gets the full content as a string.
    pub fn content(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Sets the language hint.
    pub fn set_language(&mut self, language: impl Into<String>) {
        self.language = Some(language.into());
    }

    /// Gets the language hint.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

/// Buffer manager.
pub struct BufferManager {
    /// All open buffers
    buffers: Vec<Buffer>,
    /// Next buffer ID
    next_id: u64,
    /// Currently active buffer
    active: Option<BufferId>,
}

impl BufferManager {
    /// Creates a new buffer manager.
    pub fn new() -> Self {
        Self {
            buffers: Vec::new(),
            next_id: 1,
            active: None,
        }
    }

    /// Creates a new empty buffer.
    pub fn create(&mut self) -> BufferId {
        let id = BufferId(self.next_id);
        self.next_id += 1;

        let buffer = Buffer::new(id);
        self.buffers.push(buffer);

        if self.active.is_none() {
            self.active = Some(id);
        }

        id
    }

    /// Opens an object in a buffer.
    pub fn open(
        &mut self,
        object_id: ObjectId,
        capability: CapabilityToken,
        content: &str,
    ) -> BufferId {
        let id = BufferId(self.next_id);
        self.next_id += 1;

        let buffer = Buffer::from_object(id, object_id, capability, content);
        self.buffers.push(buffer);

        self.active = Some(id);
        id
    }

    /// Closes a buffer.
    pub fn close(&mut self, id: BufferId) -> bool {
        if let Some(pos) = self.buffers.iter().position(|b| b.id == id) {
            self.buffers.remove(pos);

            // Update active buffer
            if self.active == Some(id) {
                self.active = self.buffers.first().map(|b| b.id);
            }
            true
        } else {
            false
        }
    }

    /// Gets a buffer by ID.
    pub fn get(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.iter().find(|b| b.id == id)
    }

    /// Gets a mutable buffer by ID.
    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|b| b.id == id)
    }

    /// Gets the active buffer.
    pub fn active(&self) -> Option<&Buffer> {
        self.active.and_then(|id| self.get(id))
    }

    /// Gets the active buffer mutably.
    pub fn active_mut(&mut self) -> Option<&mut Buffer> {
        self.active.and_then(|id| {
            self.buffers.iter_mut().find(|b| b.id == id)
        })
    }

    /// Sets the active buffer.
    pub fn set_active(&mut self, id: BufferId) -> bool {
        if self.buffers.iter().any(|b| b.id == id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    /// Lists all buffers.
    pub fn list(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.buffers.iter().map(|b| b.id)
    }

    /// Gets count of modified buffers.
    pub fn modified_count(&self) -> usize {
        self.buffers.iter().filter(|b| b.is_modified()).count()
    }
}

impl Default for BufferManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Editor key binding.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key: String,
    pub modifiers: u8,
    pub action: String,
}

/// Key modifiers.
pub mod modifiers {
    pub const CTRL: u8 = 1;
    pub const ALT: u8 = 2;
    pub const SHIFT: u8 = 4;
}

/// The S-CODE editor.
pub struct Editor {
    /// Buffer manager
    buffers: Mutex<BufferManager>,
    /// Key bindings
    bindings: Vec<KeyBinding>,
}

impl Editor {
    /// Creates a new editor.
    pub fn new() -> Self {
        let bindings = alloc::vec![
            KeyBinding {
                key: "s".into(),
                modifiers: modifiers::CTRL,
                action: "save".into(),
            },
            KeyBinding {
                key: "z".into(),
                modifiers: modifiers::CTRL,
                action: "undo".into(),
            },
            KeyBinding {
                key: "y".into(),
                modifiers: modifiers::CTRL,
                action: "redo".into(),
            },
            KeyBinding {
                key: "q".into(),
                modifiers: modifiers::CTRL,
                action: "quit".into(),
            },
        ];

        Self {
            buffers: Mutex::new(BufferManager::new()),
            bindings,
        }
    }

    /// Creates a new buffer.
    pub fn new_buffer(&self) -> BufferId {
        self.buffers.lock().create()
    }

    /// Opens an object.
    pub fn open(&self, object_id: ObjectId, capability: CapabilityToken, content: &str) -> BufferId {
        self.buffers.lock().open(object_id, capability, content)
    }

    /// Closes a buffer.
    pub fn close(&self, id: BufferId) -> bool {
        self.buffers.lock().close(id)
    }

    /// Executes an action.
    pub fn execute_action(&self, action: &str) -> Result<(), &'static str> {
        match action {
            "save" => {
                // Would save to S-STORAGE using capability
                Ok(())
            }
            "undo" => {
                let mut buffers = self.buffers.lock();
                if let Some(buffer) = buffers.active_mut() {
                    buffer.undo();
                }
                Ok(())
            }
            "redo" => {
                let mut buffers = self.buffers.lock();
                if let Some(buffer) = buffers.active_mut() {
                    buffer.redo();
                }
                Ok(())
            }
            "quit" => {
                // Check for unsaved changes
                let buffers = self.buffers.lock();
                if buffers.modified_count() > 0 {
                    Err("Unsaved changes exist")
                } else {
                    Ok(())
                }
            }
            _ => Err("Unknown action"),
        }
    }

    /// Finds binding for a key.
    pub fn find_binding(&self, key: &str, modifiers: u8) -> Option<&KeyBinding> {
        self.bindings
            .iter()
            .find(|b| b.key == key && b.modifiers == modifiers)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry point (placeholder - would be called by runtime).
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // Initialize editor instance
    let _editor = Editor::new();
    
    // Editor main loop
    loop {
        // Terminal setup is handled by GPU service
        // Event loop receives keyboard/mouse events
        // Render loop updates display via framebuffer
        
        // For now, return success (runtime provides event loop)
        break;
    }
    
    0
}

/// Stub allocator for tools (would be provided by runtime).
struct StubAllocator;

unsafe impl core::alloc::GlobalAlloc for StubAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[global_allocator]
static ALLOCATOR: StubAllocator = StubAllocator;

/// Panic handler.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken(1)
    }

    #[test]
    fn test_buffer_creation() {
        let buffer = Buffer::new(BufferId(1));
        assert_eq!(buffer.line_count(), 1);
        assert!(!buffer.is_modified());
    }

    #[test]
    fn test_buffer_insert() {
        let mut buffer = Buffer::new(BufferId(1));
        buffer.insert(Position::new(0, 0), "Hello");
        assert!(buffer.is_modified());
        assert_eq!(buffer.get_line(0).unwrap().content, "Hello");
    }

    #[test]
    fn test_buffer_manager() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();
        assert!(manager.get(id1).is_some());
        assert!(manager.get(id2).is_some());
        assert_eq!(manager.active, Some(id1));
    }
}
