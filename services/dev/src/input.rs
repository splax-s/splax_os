//! # Input Subsystem for S-DEV
//!
//! Userspace input device management (keyboard, mouse, touchscreen, etc.)

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

use super::driver::{InputEvent, InputEventType};
use super::DevError;

/// Input device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDeviceType {
    /// Keyboard
    Keyboard,
    /// Mouse
    Mouse,
    /// Touchpad
    Touchpad,
    /// Touchscreen
    Touchscreen,
    /// Joystick/Gamepad
    Joystick,
    /// Tablet/Digitizer
    Tablet,
    /// Other
    Other,
}

/// Key state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Released = 0,
    Pressed = 1,
    Repeat = 2,
}

impl From<i32> for KeyState {
    fn from(value: i32) -> Self {
        match value {
            0 => KeyState::Released,
            1 => KeyState::Pressed,
            _ => KeyState::Repeat,
        }
    }
}

/// LED state bits
#[derive(Debug, Clone, Copy)]
pub struct LedState(pub u8);

impl LedState {
    pub const NUM_LOCK: u8 = 0x01;
    pub const CAPS_LOCK: u8 = 0x02;
    pub const SCROLL_LOCK: u8 = 0x04;

    pub fn num_lock(&self) -> bool {
        self.0 & Self::NUM_LOCK != 0
    }

    pub fn caps_lock(&self) -> bool {
        self.0 & Self::CAPS_LOCK != 0
    }

    pub fn scroll_lock(&self) -> bool {
        self.0 & Self::SCROLL_LOCK != 0
    }

    pub fn set_num_lock(&mut self, on: bool) {
        if on {
            self.0 |= Self::NUM_LOCK;
        } else {
            self.0 &= !Self::NUM_LOCK;
        }
    }

    pub fn set_caps_lock(&mut self, on: bool) {
        if on {
            self.0 |= Self::CAPS_LOCK;
        } else {
            self.0 &= !Self::CAPS_LOCK;
        }
    }
}

/// Keyboard state
#[derive(Debug, Clone)]
pub struct KeyboardState {
    /// Currently pressed keys
    pub pressed_keys: [bool; 256],
    /// Modifier state
    pub modifiers: ModifierState,
    /// LED state
    pub leds: LedState,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            pressed_keys: [false; 256],
            modifiers: ModifierState::default(),
            leds: LedState(0),
        }
    }
}

/// Modifier key state
#[derive(Debug, Clone, Copy, Default)]
pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool, // Windows/Super/Command
    pub altgr: bool,
}

/// Mouse button state
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseButtonState {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub button4: bool,
    pub button5: bool,
}

/// Mouse state
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    /// Current X position
    pub x: i32,
    /// Current Y position
    pub y: i32,
    /// Button state
    pub buttons: MouseButtonState,
    /// Scroll accumulator
    pub scroll_x: i32,
    pub scroll_y: i32,
}

/// Input device
#[derive(Debug)]
pub struct InputDevice {
    /// Device ID
    pub id: u32,
    /// Device name
    pub name: String,
    /// Device type
    pub dev_type: InputDeviceType,
    /// Event queue
    events: VecDeque<InputEvent>,
    /// Maximum queue size
    max_events: usize,
    /// Grab state (exclusive access)
    grabbed: bool,
    /// Grabber ID
    grabber: Option<u64>,
}

impl InputDevice {
    /// Creates a new input device
    pub fn new(id: u32, name: &str, dev_type: InputDeviceType) -> Self {
        Self {
            id,
            name: String::from(name),
            dev_type,
            events: VecDeque::new(),
            max_events: 256,
            grabbed: false,
            grabber: None,
        }
    }

    /// Queues an event
    pub fn queue_event(&mut self, event: InputEvent) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Polls for events
    pub fn poll_events(&mut self) -> Vec<InputEvent> {
        self.events.drain(..).collect()
    }

    /// Checks if there are pending events
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// Grabs the device (exclusive access)
    pub fn grab(&mut self, client_id: u64) -> Result<(), DevError> {
        if self.grabbed {
            return Err(DevError::AlreadyBound);
        }
        self.grabbed = true;
        self.grabber = Some(client_id);
        Ok(())
    }

    /// Ungrabs the device
    pub fn ungrab(&mut self, client_id: u64) -> Result<(), DevError> {
        if !self.grabbed || self.grabber != Some(client_id) {
            return Err(DevError::PermissionDenied);
        }
        self.grabbed = false;
        self.grabber = None;
        Ok(())
    }
}

/// Key codes (subset of Linux key codes)
#[allow(dead_code)]
pub mod keycodes {
    pub const KEY_RESERVED: u16 = 0;
    pub const KEY_ESC: u16 = 1;
    pub const KEY_1: u16 = 2;
    pub const KEY_2: u16 = 3;
    pub const KEY_3: u16 = 4;
    pub const KEY_4: u16 = 5;
    pub const KEY_5: u16 = 6;
    pub const KEY_6: u16 = 7;
    pub const KEY_7: u16 = 8;
    pub const KEY_8: u16 = 9;
    pub const KEY_9: u16 = 10;
    pub const KEY_0: u16 = 11;
    pub const KEY_MINUS: u16 = 12;
    pub const KEY_EQUAL: u16 = 13;
    pub const KEY_BACKSPACE: u16 = 14;
    pub const KEY_TAB: u16 = 15;
    pub const KEY_Q: u16 = 16;
    pub const KEY_W: u16 = 17;
    pub const KEY_E: u16 = 18;
    pub const KEY_R: u16 = 19;
    pub const KEY_T: u16 = 20;
    pub const KEY_Y: u16 = 21;
    pub const KEY_U: u16 = 22;
    pub const KEY_I: u16 = 23;
    pub const KEY_O: u16 = 24;
    pub const KEY_P: u16 = 25;
    pub const KEY_LEFTBRACE: u16 = 26;
    pub const KEY_RIGHTBRACE: u16 = 27;
    pub const KEY_ENTER: u16 = 28;
    pub const KEY_LEFTCTRL: u16 = 29;
    pub const KEY_A: u16 = 30;
    pub const KEY_S: u16 = 31;
    pub const KEY_D: u16 = 32;
    pub const KEY_F: u16 = 33;
    pub const KEY_G: u16 = 34;
    pub const KEY_H: u16 = 35;
    pub const KEY_J: u16 = 36;
    pub const KEY_K: u16 = 37;
    pub const KEY_L: u16 = 38;
    pub const KEY_SEMICOLON: u16 = 39;
    pub const KEY_APOSTROPHE: u16 = 40;
    pub const KEY_GRAVE: u16 = 41;
    pub const KEY_LEFTSHIFT: u16 = 42;
    pub const KEY_BACKSLASH: u16 = 43;
    pub const KEY_Z: u16 = 44;
    pub const KEY_X: u16 = 45;
    pub const KEY_C: u16 = 46;
    pub const KEY_V: u16 = 47;
    pub const KEY_B: u16 = 48;
    pub const KEY_N: u16 = 49;
    pub const KEY_M: u16 = 50;
    pub const KEY_COMMA: u16 = 51;
    pub const KEY_DOT: u16 = 52;
    pub const KEY_SLASH: u16 = 53;
    pub const KEY_RIGHTSHIFT: u16 = 54;
    pub const KEY_KPASTERISK: u16 = 55;
    pub const KEY_LEFTALT: u16 = 56;
    pub const KEY_SPACE: u16 = 57;
    pub const KEY_CAPSLOCK: u16 = 58;
    pub const KEY_F1: u16 = 59;
    pub const KEY_F2: u16 = 60;
    pub const KEY_F3: u16 = 61;
    pub const KEY_F4: u16 = 62;
    pub const KEY_F5: u16 = 63;
    pub const KEY_F6: u16 = 64;
    pub const KEY_F7: u16 = 65;
    pub const KEY_F8: u16 = 66;
    pub const KEY_F9: u16 = 67;
    pub const KEY_F10: u16 = 68;
    pub const KEY_NUMLOCK: u16 = 69;
    pub const KEY_SCROLLLOCK: u16 = 70;
    pub const KEY_UP: u16 = 103;
    pub const KEY_LEFT: u16 = 105;
    pub const KEY_RIGHT: u16 = 106;
    pub const KEY_DOWN: u16 = 108;
    pub const KEY_DELETE: u16 = 111;
}

/// Relative axis codes
#[allow(dead_code)]
pub mod rel_codes {
    pub const REL_X: u16 = 0x00;
    pub const REL_Y: u16 = 0x01;
    pub const REL_Z: u16 = 0x02;
    pub const REL_WHEEL: u16 = 0x08;
    pub const REL_HWHEEL: u16 = 0x06;
}

/// Button codes
#[allow(dead_code)]
pub mod btn_codes {
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const BTN_SIDE: u16 = 0x113;
    pub const BTN_EXTRA: u16 = 0x114;
    pub const BTN_TOUCH: u16 = 0x14a;
}

/// Input subsystem manager
pub struct InputManager {
    /// Input devices
    devices: BTreeMap<u32, InputDevice>,
    /// Keyboard state
    pub keyboard_state: KeyboardState,
    /// Mouse state
    pub mouse_state: MouseState,
    /// Next device ID
    next_device_id: u32,
    /// Focus handler (receives all keyboard events)
    focus_handler: Option<u64>,
    /// Pointer handler (receives all mouse events)
    pointer_handler: Option<u64>,
}

impl InputManager {
    /// Creates a new input manager
    pub fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
            keyboard_state: KeyboardState::default(),
            mouse_state: MouseState::default(),
            next_device_id: 1,
            focus_handler: None,
            pointer_handler: None,
        }
    }

    /// Registers an input device
    pub fn register_device(&mut self, name: &str, dev_type: InputDeviceType) -> u32 {
        let id = self.next_device_id;
        self.next_device_id += 1;
        self.devices.insert(id, InputDevice::new(id, name, dev_type));
        id
    }

    /// Unregisters an input device
    pub fn unregister_device(&mut self, id: u32) -> Option<InputDevice> {
        self.devices.remove(&id)
    }

    /// Gets a device
    pub fn get_device(&mut self, id: u32) -> Option<&mut InputDevice> {
        self.devices.get_mut(&id)
    }

    /// Lists all devices
    pub fn list_devices(&self) -> Vec<&InputDevice> {
        self.devices.values().collect()
    }

    /// Processes a raw input event
    pub fn process_event(&mut self, device_id: u32, event: InputEvent) {
        // Update global state
        match event.event_type {
            InputEventType::Key => {
                let keycode = event.code as usize;
                if keycode < 256 {
                    self.keyboard_state.pressed_keys[keycode] = event.value != 0;

                    // Update modifiers
                    match event.code {
                        keycodes::KEY_LEFTSHIFT | keycodes::KEY_RIGHTSHIFT => {
                            self.keyboard_state.modifiers.shift = event.value != 0;
                        }
                        keycodes::KEY_LEFTCTRL => {
                            self.keyboard_state.modifiers.ctrl = event.value != 0;
                        }
                        keycodes::KEY_LEFTALT => {
                            self.keyboard_state.modifiers.alt = event.value != 0;
                        }
                        keycodes::KEY_CAPSLOCK if event.value == 1 => {
                            let current = self.keyboard_state.leds.caps_lock();
                            self.keyboard_state.leds.set_caps_lock(!current);
                        }
                        keycodes::KEY_NUMLOCK if event.value == 1 => {
                            let current = self.keyboard_state.leds.num_lock();
                            self.keyboard_state.leds.set_num_lock(!current);
                        }
                        _ => {}
                    }
                }

                // Mouse buttons
                match event.code {
                    btn_codes::BTN_LEFT => {
                        self.mouse_state.buttons.left = event.value != 0;
                    }
                    btn_codes::BTN_RIGHT => {
                        self.mouse_state.buttons.right = event.value != 0;
                    }
                    btn_codes::BTN_MIDDLE => {
                        self.mouse_state.buttons.middle = event.value != 0;
                    }
                    _ => {}
                }
            }
            InputEventType::Relative => {
                match event.code {
                    rel_codes::REL_X => {
                        self.mouse_state.x = self.mouse_state.x.saturating_add(event.value);
                    }
                    rel_codes::REL_Y => {
                        self.mouse_state.y = self.mouse_state.y.saturating_add(event.value);
                    }
                    rel_codes::REL_WHEEL => {
                        self.mouse_state.scroll_y = self.mouse_state.scroll_y.saturating_add(event.value);
                    }
                    rel_codes::REL_HWHEEL => {
                        self.mouse_state.scroll_x = self.mouse_state.scroll_x.saturating_add(event.value);
                    }
                    _ => {}
                }
            }
            InputEventType::Absolute => {
                // Touchscreen absolute positioning
                // Would need screen dimensions to map properly
            }
            InputEventType::Misc => {}
        }

        // Queue to device
        if let Some(device) = self.devices.get_mut(&device_id) {
            device.queue_event(event);
        }
    }

    /// Polls events from all devices
    pub fn poll_all_events(&mut self) -> Vec<(u32, InputEvent)> {
        let mut all_events = Vec::new();
        for (id, device) in &mut self.devices {
            for event in device.poll_events() {
                all_events.push((*id, event));
            }
        }
        all_events
    }

    /// Sets focus handler
    pub fn set_focus_handler(&mut self, handler: u64) {
        self.focus_handler = Some(handler);
    }

    /// Clears focus handler
    pub fn clear_focus_handler(&mut self) {
        self.focus_handler = None;
    }

    /// Gets current focus handler
    pub fn get_focus_handler(&self) -> Option<u64> {
        self.focus_handler
    }

    /// Checks if a key is currently pressed
    pub fn is_key_pressed(&self, keycode: u16) -> bool {
        if (keycode as usize) < 256 {
            self.keyboard_state.pressed_keys[keycode as usize]
        } else {
            false
        }
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
