//! Input handling for S-CANVAS
//!
//! Routes keyboard and mouse input to the appropriate windows.

use super::{WindowId, Point};

/// Mouse button
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// Key state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
    Repeat,
}

/// Modifier keys
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,  // Super/Windows key
    pub caps_lock: bool,
    pub num_lock: bool,
}

/// Keyboard event
#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    /// Scancode (hardware key code)
    pub scancode: u32,
    /// Virtual key code (OS key code)
    pub keycode: u32,
    /// Key state
    pub state: KeyState,
    /// Modifiers
    pub modifiers: Modifiers,
    /// Unicode character (if printable)
    pub char: Option<char>,
}

/// Mouse event
#[derive(Debug, Clone)]
pub struct MouseEvent {
    /// Event type
    pub kind: MouseEventKind,
    /// Position in screen coordinates
    pub position: Point,
    /// Button (for click events)
    pub button: Option<MouseButton>,
    /// Modifiers
    pub modifiers: Modifiers,
}

/// Mouse event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    /// Mouse moved
    Move,
    /// Button pressed
    ButtonDown,
    /// Button released
    ButtonUp,
    /// Mouse wheel vertical
    WheelVertical(i32),
    /// Mouse wheel horizontal
    WheelHorizontal(i32),
    /// Mouse entered window
    Enter,
    /// Mouse left window
    Leave,
}

/// Input focus type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusKind {
    Keyboard,
    Pointer,
}

/// Input router state
pub struct InputRouter {
    /// Current keyboard focus
    keyboard_focus: Option<WindowId>,
    /// Current pointer focus
    pointer_focus: Option<WindowId>,
    /// Current mouse position
    mouse_position: Point,
    /// Current modifiers
    modifiers: Modifiers,
    /// Pressed mouse buttons
    pressed_buttons: u8,
    /// Grabbed window (receives all input)
    grab: Option<WindowId>,
}

impl InputRouter {
    pub fn new() -> Self {
        Self {
            keyboard_focus: None,
            pointer_focus: None,
            mouse_position: Point::default(),
            modifiers: Modifiers::default(),
            pressed_buttons: 0,
            grab: None,
        }
    }

    /// Set keyboard focus
    pub fn set_keyboard_focus(&mut self, window: Option<WindowId>) {
        self.keyboard_focus = window;
    }

    /// Set pointer focus
    pub fn set_pointer_focus(&mut self, window: Option<WindowId>) {
        self.pointer_focus = window;
    }

    /// Get keyboard focus
    pub fn keyboard_focus(&self) -> Option<WindowId> {
        self.keyboard_focus
    }

    /// Get pointer focus
    pub fn pointer_focus(&self) -> Option<WindowId> {
        self.pointer_focus
    }

    /// Update mouse position
    pub fn update_mouse_position(&mut self, position: Point) {
        self.mouse_position = position;
    }

    /// Get current mouse position
    pub fn mouse_position(&self) -> Point {
        self.mouse_position
    }

    /// Update modifiers
    pub fn update_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Get current modifiers
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Press mouse button
    pub fn press_button(&mut self, button: MouseButton) {
        self.pressed_buttons |= 1 << (button as u8);
    }

    /// Release mouse button
    pub fn release_button(&mut self, button: MouseButton) {
        self.pressed_buttons &= !(1 << (button as u8));
    }

    /// Check if button is pressed
    pub fn is_button_pressed(&self, button: MouseButton) -> bool {
        (self.pressed_buttons & (1 << (button as u8))) != 0
    }

    /// Grab input to window
    pub fn grab(&mut self, window: WindowId) {
        self.grab = Some(window);
    }

    /// Release grab
    pub fn ungrab(&mut self) {
        self.grab = None;
    }

    /// Get grabbed window
    pub fn grabbed(&self) -> Option<WindowId> {
        self.grab
    }

    /// Route keyboard event to appropriate window
    pub fn route_keyboard_event(&self, event: &KeyboardEvent) -> Option<WindowId> {
        self.grab.or(self.keyboard_focus)
    }

    /// Route mouse event to appropriate window
    pub fn route_mouse_event(&self, event: &MouseEvent) -> Option<WindowId> {
        self.grab.or(self.pointer_focus)
    }
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}
