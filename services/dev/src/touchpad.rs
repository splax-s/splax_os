//! # Touchpad Driver for S-DEV
//!
//! Comprehensive touchpad support with multi-touch gestures.
//!
//! ## Features
//!
//! - Multi-touch tracking (up to 10 fingers)
//! - Gesture recognition (tap, swipe, pinch, rotate)
//! - Palm rejection
//! - Pressure sensitivity
//! - Edge scrolling
//! - Click zones
//!
//! ## Supported Protocols
//!
//! - PS/2 Synaptics
//! - PS/2 ALPS
//! - HID multi-touch
//! - I2C HID (Intel Precision Touchpad)

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::driver::{InputEvent, InputEventType};
use super::DevError;

// =============================================================================
// Constants
// =============================================================================

/// Maximum simultaneous touch points.
pub const MAX_TOUCH_POINTS: usize = 10;

/// Minimum distance for swipe (in touchpad units).
pub const SWIPE_MIN_DISTANCE: i32 = 100;

/// Maximum time for tap gesture (ms).
pub const TAP_MAX_TIME_MS: u64 = 200;

/// Time for double-tap (ms).
pub const DOUBLE_TAP_TIME_MS: u64 = 300;

/// Minimum pinch distance change for detection.
pub const PINCH_MIN_DISTANCE: i32 = 50;

/// Palm rejection minimum size.
pub const PALM_MIN_SIZE: u32 = 100;

// =============================================================================
// Absolute Axis Codes
// =============================================================================

/// Absolute axis codes for multi-touch.
pub mod abs_codes {
    /// Single-touch X position.
    pub const ABS_X: u16 = 0x00;
    /// Single-touch Y position.
    pub const ABS_Y: u16 = 0x01;
    /// Pressure.
    pub const ABS_PRESSURE: u16 = 0x18;
    /// Tool width.
    pub const ABS_TOOL_WIDTH: u16 = 0x1c;

    /// Multi-touch slot.
    pub const ABS_MT_SLOT: u16 = 0x2f;
    /// Touch major axis length.
    pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
    /// Touch minor axis length.
    pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
    /// Touch orientation.
    pub const ABS_MT_ORIENTATION: u16 = 0x34;
    /// MT X position.
    pub const ABS_MT_POSITION_X: u16 = 0x35;
    /// MT Y position.
    pub const ABS_MT_POSITION_Y: u16 = 0x36;
    /// Tool type.
    pub const ABS_MT_TOOL_TYPE: u16 = 0x37;
    /// Tracking ID.
    pub const ABS_MT_TRACKING_ID: u16 = 0x39;
    /// Pressure.
    pub const ABS_MT_PRESSURE: u16 = 0x3a;
    /// Distance.
    pub const ABS_MT_DISTANCE: u16 = 0x3b;
}

/// Multi-touch tool types.
pub mod mt_tool {
    /// Finger.
    pub const MT_TOOL_FINGER: u32 = 0;
    /// Stylus/pen.
    pub const MT_TOOL_PEN: u32 = 1;
    /// Palm (for rejection).
    pub const MT_TOOL_PALM: u32 = 2;
}

// =============================================================================
// Touchpad Configuration
// =============================================================================

/// Touchpad configuration.
#[derive(Debug, Clone)]
pub struct TouchpadConfig {
    /// Enable tap-to-click.
    pub tap_to_click: bool,
    /// Enable two-finger tap as right-click.
    pub two_finger_tap_right_click: bool,
    /// Enable three-finger tap as middle-click.
    pub three_finger_tap_middle_click: bool,
    /// Enable two-finger scrolling.
    pub two_finger_scroll: bool,
    /// Enable edge scrolling.
    pub edge_scroll: bool,
    /// Enable natural scrolling (inverted).
    pub natural_scroll: bool,
    /// Enable horizontal scrolling.
    pub horizontal_scroll: bool,
    /// Enable pinch-to-zoom gesture.
    pub pinch_zoom: bool,
    /// Enable rotation gesture.
    pub rotation: bool,
    /// Enable three-finger swipe.
    pub three_finger_swipe: bool,
    /// Enable four-finger swipe.
    pub four_finger_swipe: bool,
    /// Palm rejection enabled.
    pub palm_rejection: bool,
    /// Pointer acceleration.
    pub acceleration: f32,
    /// Pointer sensitivity (1.0 = normal).
    pub sensitivity: f32,
    /// Click zone configuration.
    pub click_zones: ClickZoneConfig,
    /// Disable while typing.
    pub disable_while_typing: bool,
    /// Disable while external mouse connected.
    pub disable_with_mouse: bool,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            tap_to_click: true,
            two_finger_tap_right_click: true,
            three_finger_tap_middle_click: true,
            two_finger_scroll: true,
            edge_scroll: false,
            natural_scroll: true,
            horizontal_scroll: true,
            pinch_zoom: true,
            rotation: true,
            three_finger_swipe: true,
            four_finger_swipe: true,
            palm_rejection: true,
            acceleration: 1.0,
            sensitivity: 1.0,
            click_zones: ClickZoneConfig::default(),
            disable_while_typing: true,
            disable_with_mouse: false,
        }
    }
}

/// Click zone configuration for clickpads.
#[derive(Debug, Clone)]
pub struct ClickZoneConfig {
    /// Enable click zones.
    pub enabled: bool,
    /// Right zone starts at this X percentage.
    pub right_zone_start: f32,
    /// Middle zone ends at this X percentage.
    pub middle_zone_end: f32,
    /// Zones only in bottom area (percentage from bottom).
    pub bottom_zone_height: f32,
}

impl Default for ClickZoneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            right_zone_start: 0.66,
            middle_zone_end: 0.33,
            bottom_zone_height: 0.2,
        }
    }
}

// =============================================================================
// Touch Point
// =============================================================================

/// State of a single touch point.
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    /// Tracking ID (-1 if released).
    pub tracking_id: i32,
    /// X position.
    pub x: i32,
    /// Y position.
    pub y: i32,
    /// Pressure.
    pub pressure: u32,
    /// Touch major axis.
    pub major: u32,
    /// Touch minor axis.
    pub minor: u32,
    /// Orientation.
    pub orientation: i32,
    /// Tool type.
    pub tool_type: u32,
    /// Is palm.
    pub is_palm: bool,
    /// Touch start time.
    pub start_time: u64,
    /// Initial X position.
    pub start_x: i32,
    /// Initial Y position.
    pub start_y: i32,
}

impl TouchPoint {
    /// Create a new touch point.
    pub fn new(tracking_id: i32, x: i32, y: i32, time: u64) -> Self {
        Self {
            tracking_id,
            x,
            y,
            pressure: 0,
            major: 0,
            minor: 0,
            orientation: 0,
            tool_type: mt_tool::MT_TOOL_FINGER,
            is_palm: false,
            start_time: time,
            start_x: x,
            start_y: y,
        }
    }

    /// Check if this is a valid touch.
    pub fn is_valid(&self) -> bool {
        self.tracking_id >= 0
    }

    /// Calculate distance from start.
    pub fn distance_from_start(&self) -> f64 {
        let dx = (self.x - self.start_x) as f64;
        let dy = (self.y - self.start_y) as f64;
        libm::sqrt(dx * dx + dy * dy)
    }

    /// Calculate movement vector.
    pub fn movement(&self) -> (i32, i32) {
        (self.x - self.start_x, self.y - self.start_y)
    }
}

impl Default for TouchPoint {
    fn default() -> Self {
        Self {
            tracking_id: -1,
            x: 0,
            y: 0,
            pressure: 0,
            major: 0,
            minor: 0,
            orientation: 0,
            tool_type: 0,
            is_palm: false,
            start_time: 0,
            start_x: 0,
            start_y: 0,
        }
    }
}

// =============================================================================
// Gesture Types
// =============================================================================

/// Recognized gestures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    /// No gesture.
    None,
    /// Single finger tap.
    Tap,
    /// Double tap.
    DoubleTap,
    /// Two finger tap.
    TwoFingerTap,
    /// Three finger tap.
    ThreeFingerTap,
    /// Single finger drag.
    Drag,
    /// Two finger scroll.
    Scroll,
    /// Two finger pinch (zoom).
    Pinch,
    /// Two finger rotate.
    Rotate,
    /// Three finger swipe.
    ThreeFingerSwipe,
    /// Four finger swipe.
    FourFingerSwipe,
}

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Gesture event.
#[derive(Debug, Clone)]
pub struct GestureEvent {
    /// Gesture type.
    pub gesture_type: GestureType,
    /// Gesture state (begin, update, end).
    pub state: GestureState,
    /// Number of fingers.
    pub finger_count: u8,
    /// Delta X (for scroll/swipe).
    pub delta_x: f64,
    /// Delta Y (for scroll/swipe).
    pub delta_y: f64,
    /// Scale factor (for pinch, 1.0 = no change).
    pub scale: f64,
    /// Rotation angle in radians.
    pub rotation: f64,
    /// Swipe direction (if applicable).
    pub direction: Option<SwipeDirection>,
    /// Timestamp.
    pub timestamp: u64,
}

/// Gesture state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureState {
    /// Gesture started.
    Begin,
    /// Gesture updated.
    Update,
    /// Gesture ended.
    End,
    /// Gesture cancelled.
    Cancel,
}

// =============================================================================
// Touchpad State
// =============================================================================

/// Current touchpad state.
#[derive(Debug)]
pub struct TouchpadState {
    /// Current slot for multi-touch protocol B.
    current_slot: usize,
    /// Touch points.
    points: [TouchPoint; MAX_TOUCH_POINTS],
    /// Active touch count.
    active_count: u8,
    /// Button state (for clickpads).
    button_pressed: bool,
    /// Physical button state.
    physical_buttons: PhysicalButtons,
    /// Last tap time.
    last_tap_time: u64,
    /// Last tap position.
    last_tap_pos: (i32, i32),
    /// Current gesture.
    current_gesture: GestureType,
    /// Gesture start state.
    gesture_start_points: [TouchPoint; MAX_TOUCH_POINTS],
    /// Initial pinch distance.
    pinch_start_distance: f64,
    /// Initial rotation angle.
    rotate_start_angle: f64,
    /// Accumulated scroll.
    scroll_accum: (f64, f64),
    /// Last event time.
    last_event_time: u64,
    /// Currently in palm.
    in_palm: bool,
}

/// Physical button state.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicalButtons {
    /// Left button.
    pub left: bool,
    /// Right button.
    pub right: bool,
    /// Middle button.
    pub middle: bool,
}

impl TouchpadState {
    /// Create new touchpad state.
    pub fn new() -> Self {
        Self {
            current_slot: 0,
            points: [TouchPoint::default(); MAX_TOUCH_POINTS],
            active_count: 0,
            button_pressed: false,
            physical_buttons: PhysicalButtons::default(),
            last_tap_time: 0,
            last_tap_pos: (0, 0),
            current_gesture: GestureType::None,
            gesture_start_points: [TouchPoint::default(); MAX_TOUCH_POINTS],
            pinch_start_distance: 0.0,
            rotate_start_angle: 0.0,
            scroll_accum: (0.0, 0.0),
            last_event_time: 0,
            in_palm: false,
        }
    }

    /// Get active touch count.
    pub fn active_touch_count(&self) -> u8 {
        self.active_count
    }

    /// Get all active touches.
    pub fn active_touches(&self) -> impl Iterator<Item = &TouchPoint> {
        self.points.iter().filter(|p| p.is_valid())
    }

    /// Get current slot.
    pub fn current_touch(&self) -> Option<&TouchPoint> {
        let point = &self.points[self.current_slot];
        if point.is_valid() {
            Some(point)
        } else {
            None
        }
    }

    /// Calculate center of all touches.
    pub fn touch_center(&self) -> Option<(i32, i32)> {
        let active: Vec<_> = self.active_touches().collect();
        if active.is_empty() {
            return None;
        }

        let sum_x: i32 = active.iter().map(|p| p.x).sum();
        let sum_y: i32 = active.iter().map(|p| p.y).sum();
        let count = active.len() as i32;

        Some((sum_x / count, sum_y / count))
    }

    /// Calculate distance between first two touches.
    pub fn two_finger_distance(&self) -> Option<f64> {
        let active: Vec<_> = self.active_touches().collect();
        if active.len() < 2 {
            return None;
        }

        let dx = (active[1].x - active[0].x) as f64;
        let dy = (active[1].y - active[0].y) as f64;
        Some(libm::sqrt(dx * dx + dy * dy))
    }

    /// Calculate angle between first two touches.
    pub fn two_finger_angle(&self) -> Option<f64> {
        let active: Vec<_> = self.active_touches().collect();
        if active.len() < 2 {
            return None;
        }

        let dx = (active[1].x - active[0].x) as f64;
        let dy = (active[1].y - active[0].y) as f64;
        Some(libm::atan2(dy, dx))
    }
}

impl Default for TouchpadState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Touchpad Hardware Info
// =============================================================================

/// Touchpad hardware capabilities.
#[derive(Debug, Clone)]
pub struct TouchpadInfo {
    /// Device name.
    pub name: [u8; 64],
    /// Vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Protocol type.
    pub protocol: TouchpadProtocol,
    /// X resolution (units per mm).
    pub x_res: u32,
    /// Y resolution (units per mm).
    pub y_res: u32,
    /// Minimum X.
    pub x_min: i32,
    /// Maximum X.
    pub x_max: i32,
    /// Minimum Y.
    pub y_min: i32,
    /// Maximum Y.
    pub y_max: i32,
    /// Pressure range.
    pub pressure_max: u32,
    /// Maximum touch slots.
    pub max_slots: u8,
    /// Is clickpad (button under surface).
    pub is_clickpad: bool,
    /// Has physical buttons.
    pub has_buttons: bool,
    /// Supports palm detection.
    pub palm_detect: bool,
}

/// Touchpad protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchpadProtocol {
    /// PS/2 Synaptics.
    Synaptics,
    /// PS/2 ALPS.
    Alps,
    /// PS/2 Elantech.
    Elantech,
    /// HID multi-touch.
    HidMultiTouch,
    /// I2C HID (Intel Precision).
    I2cHid,
    /// Generic.
    Generic,
}

// =============================================================================
// Touchpad Driver
// =============================================================================

/// Touchpad driver.
pub struct TouchpadDriver {
    /// Hardware info.
    pub info: TouchpadInfo,
    /// Configuration.
    pub config: TouchpadConfig,
    /// Current state.
    pub state: TouchpadState,
    /// Gesture callback.
    gesture_callback: Option<fn(GestureEvent)>,
    /// Tracking ID to slot mapping.
    tracking_map: BTreeMap<i32, usize>,
    /// Next tracking ID.
    next_tracking_id: i32,
}

impl TouchpadDriver {
    /// Create a new touchpad driver.
    pub fn new(info: TouchpadInfo) -> Self {
        Self {
            info,
            config: TouchpadConfig::default(),
            state: TouchpadState::new(),
            gesture_callback: None,
            tracking_map: BTreeMap::new(),
            next_tracking_id: 0,
        }
    }

    /// Set gesture callback.
    pub fn set_gesture_callback(&mut self, callback: fn(GestureEvent)) {
        self.gesture_callback = Some(callback);
    }

    /// Process a raw input event.
    pub fn process_event(&mut self, event: &InputEvent, timestamp: u64) {
        self.state.last_event_time = timestamp;

        match event.event_type {
            InputEventType::Absolute => {
                self.process_absolute_event(event.code, event.value, timestamp);
            }
            InputEventType::Key => {
                self.process_button_event(event.code, event.value);
            }
            _ => {}
        }
    }

    /// Process sync event (end of report).
    pub fn sync(&mut self, timestamp: u64) {
        // Update active count
        self.state.active_count = self.state.points.iter()
            .filter(|p| p.is_valid())
            .count() as u8;

        // Check for palm rejection
        self.check_palm_rejection();

        // Process gestures
        self.process_gestures(timestamp);
    }

    fn process_absolute_event(&mut self, code: u16, value: i32, timestamp: u64) {
        match code {
            abs_codes::ABS_MT_SLOT => {
                if (value as usize) < MAX_TOUCH_POINTS {
                    self.state.current_slot = value as usize;
                }
            }
            abs_codes::ABS_MT_TRACKING_ID => {
                let slot = self.state.current_slot;
                if value < 0 {
                    // Touch released
                    if self.state.points[slot].is_valid() {
                        self.on_touch_up(slot, timestamp);
                    }
                    self.state.points[slot].tracking_id = -1;
                } else {
                    // New touch
                    if !self.state.points[slot].is_valid() {
                        self.state.points[slot] = TouchPoint::new(
                            value,
                            0, 0,
                            timestamp,
                        );
                        self.on_touch_down(slot, timestamp);
                    }
                    self.state.points[slot].tracking_id = value;
                    self.tracking_map.insert(value, slot);
                }
            }
            abs_codes::ABS_MT_POSITION_X | abs_codes::ABS_X => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    let old_x = self.state.points[slot].x;
                    self.state.points[slot].x = value;
                    if self.state.points[slot].start_x == 0 && old_x == 0 {
                        self.state.points[slot].start_x = value;
                    }
                }
            }
            abs_codes::ABS_MT_POSITION_Y | abs_codes::ABS_Y => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    let old_y = self.state.points[slot].y;
                    self.state.points[slot].y = value;
                    if self.state.points[slot].start_y == 0 && old_y == 0 {
                        self.state.points[slot].start_y = value;
                    }
                }
            }
            abs_codes::ABS_MT_PRESSURE | abs_codes::ABS_PRESSURE => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    self.state.points[slot].pressure = value as u32;
                }
            }
            abs_codes::ABS_MT_TOUCH_MAJOR => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    self.state.points[slot].major = value as u32;
                }
            }
            abs_codes::ABS_MT_TOUCH_MINOR => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    self.state.points[slot].minor = value as u32;
                }
            }
            abs_codes::ABS_MT_ORIENTATION => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    self.state.points[slot].orientation = value;
                }
            }
            abs_codes::ABS_MT_TOOL_TYPE => {
                let slot = self.state.current_slot;
                if self.state.points[slot].is_valid() {
                    self.state.points[slot].tool_type = value as u32;
                }
            }
            _ => {}
        }
    }

    fn process_button_event(&mut self, code: u16, value: i32) {
        let pressed = value != 0;

        match code {
            // BTN_LEFT
            0x110 => {
                self.state.physical_buttons.left = pressed;
                self.state.button_pressed = pressed;
            }
            // BTN_RIGHT
            0x111 => {
                self.state.physical_buttons.right = pressed;
            }
            // BTN_MIDDLE
            0x112 => {
                self.state.physical_buttons.middle = pressed;
            }
            // BTN_TOUCH (clickpad)
            0x14a => {
                self.state.button_pressed = pressed;
            }
            // BTN_TOOL_FINGER
            0x145 => {}
            // BTN_TOOL_DOUBLETAP
            0x14d => {}
            // BTN_TOOL_TRIPLETAP
            0x14e => {}
            // BTN_TOOL_QUADTAP
            0x14f => {}
            _ => {}
        }
    }

    fn on_touch_down(&mut self, _slot: usize, timestamp: u64) {
        // Save gesture start state
        if self.state.current_gesture == GestureType::None {
            self.state.gesture_start_points = self.state.points.clone();
        }

        // Update pinch/rotate initial values
        if self.state.active_count == 2 {
            if let Some(dist) = self.state.two_finger_distance() {
                self.state.pinch_start_distance = dist;
            }
            if let Some(angle) = self.state.two_finger_angle() {
                self.state.rotate_start_angle = angle;
            }
        }

        let _ = timestamp;
    }

    fn on_touch_up(&mut self, _slot: usize, timestamp: u64) {
        // Get data from point before any mutable borrows
        let (duration, distance, tracking_id) = {
            let point = &self.state.points[_slot];
            let duration = timestamp.saturating_sub(point.start_time);
            let distance = point.distance_from_start();
            let tracking_id = point.tracking_id;
            (duration, distance, tracking_id)
        };

        // Check for tap gesture
        if duration <= TAP_MAX_TIME_MS && distance < SWIPE_MIN_DISTANCE as f64 {
            self.handle_tap(timestamp);
        }

        // Clean up tracking
        self.tracking_map.remove(&tracking_id);
    }

    fn handle_tap(&mut self, timestamp: u64) {
        let finger_count = self.state.active_count;
        let since_last_tap = timestamp.saturating_sub(self.state.last_tap_time);

        // Check for double tap
        if since_last_tap <= DOUBLE_TAP_TIME_MS && finger_count == 1 {
            self.emit_gesture(GestureEvent {
                gesture_type: GestureType::DoubleTap,
                state: GestureState::End,
                finger_count: 1,
                delta_x: 0.0,
                delta_y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                direction: None,
                timestamp,
            });
        } else {
            // Single/multi-finger tap
            let gesture = match finger_count {
                1 => GestureType::Tap,
                2 => GestureType::TwoFingerTap,
                3 => GestureType::ThreeFingerTap,
                _ => return,
            };

            self.emit_gesture(GestureEvent {
                gesture_type: gesture,
                state: GestureState::End,
                finger_count,
                delta_x: 0.0,
                delta_y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                direction: None,
                timestamp,
            });

            self.state.last_tap_time = timestamp;
            if let Some(center) = self.state.touch_center() {
                self.state.last_tap_pos = center;
            }
        }
    }

    fn check_palm_rejection(&mut self) {
        if !self.config.palm_rejection {
            return;
        }

        for point in &mut self.state.points {
            if point.is_valid() {
                // Check touch size
                if point.major >= PALM_MIN_SIZE || point.minor >= PALM_MIN_SIZE {
                    point.is_palm = true;
                }

                // Check tool type
                if point.tool_type == mt_tool::MT_TOOL_PALM {
                    point.is_palm = true;
                }
            }
        }

        // Update palm state
        self.state.in_palm = self.state.points.iter()
            .filter(|p| p.is_valid())
            .all(|p| p.is_palm);
    }

    fn process_gestures(&mut self, timestamp: u64) {
        // Skip if in palm
        if self.state.in_palm {
            return;
        }

        let finger_count = self.state.active_count;

        match finger_count {
            0 => {
                // End any ongoing gesture
                if self.state.current_gesture != GestureType::None {
                    self.end_gesture(timestamp);
                }
            }
            1 => {
                // Single finger - drag
                if self.state.current_gesture == GestureType::None {
                    self.state.current_gesture = GestureType::Drag;
                }
            }
            2 => {
                // Two fingers - scroll, pinch, or rotate
                self.process_two_finger_gesture(timestamp);
            }
            3 => {
                // Three finger swipe
                if self.config.three_finger_swipe {
                    self.process_multi_finger_swipe(3, timestamp);
                }
            }
            4 => {
                // Four finger swipe
                if self.config.four_finger_swipe {
                    self.process_multi_finger_swipe(4, timestamp);
                }
            }
            _ => {}
        }
    }

    fn process_two_finger_gesture(&mut self, timestamp: u64) {
        // Get current distance and angle
        let current_distance = match self.state.two_finger_distance() {
            Some(d) => d,
            None => return,
        };
        let current_angle = match self.state.two_finger_angle() {
            Some(a) => a,
            None => return,
        };

        let distance_delta = current_distance - self.state.pinch_start_distance;
        let angle_delta = current_angle - self.state.rotate_start_angle;

        // Calculate scroll delta from finger movement
        let scroll_delta = self.calculate_scroll_delta();

        // Determine gesture type based on movement pattern
        if distance_delta.abs() > PINCH_MIN_DISTANCE as f64 && self.config.pinch_zoom {
            // Pinch gesture
            if self.state.current_gesture != GestureType::Pinch {
                self.state.current_gesture = GestureType::Pinch;
                self.emit_gesture(GestureEvent {
                    gesture_type: GestureType::Pinch,
                    state: GestureState::Begin,
                    finger_count: 2,
                    delta_x: 0.0,
                    delta_y: 0.0,
                    scale: 1.0,
                    rotation: 0.0,
                    direction: None,
                    timestamp,
                });
            }

            let scale = current_distance / self.state.pinch_start_distance;
            self.emit_gesture(GestureEvent {
                gesture_type: GestureType::Pinch,
                state: GestureState::Update,
                finger_count: 2,
                delta_x: 0.0,
                delta_y: 0.0,
                scale,
                rotation: 0.0,
                direction: None,
                timestamp,
            });
        } else if angle_delta.abs() > 0.1 && self.config.rotation {
            // Rotation gesture
            if self.state.current_gesture != GestureType::Rotate {
                self.state.current_gesture = GestureType::Rotate;
            }
            self.emit_gesture(GestureEvent {
                gesture_type: GestureType::Rotate,
                state: GestureState::Update,
                finger_count: 2,
                delta_x: 0.0,
                delta_y: 0.0,
                scale: 1.0,
                rotation: angle_delta,
                direction: None,
                timestamp,
            });
        } else if self.config.two_finger_scroll {
            // Scroll gesture
            if self.state.current_gesture != GestureType::Scroll {
                self.state.current_gesture = GestureType::Scroll;
                self.emit_gesture(GestureEvent {
                    gesture_type: GestureType::Scroll,
                    state: GestureState::Begin,
                    finger_count: 2,
                    delta_x: 0.0,
                    delta_y: 0.0,
                    scale: 1.0,
                    rotation: 0.0,
                    direction: None,
                    timestamp,
                });
            }

            let (mut dx, mut dy) = scroll_delta;

            // Apply natural scrolling
            if self.config.natural_scroll {
                dx = -dx;
                dy = -dy;
            }

            self.emit_gesture(GestureEvent {
                gesture_type: GestureType::Scroll,
                state: GestureState::Update,
                finger_count: 2,
                delta_x: dx,
                delta_y: dy,
                scale: 1.0,
                rotation: 0.0,
                direction: None,
                timestamp,
            });
        }
    }

    fn process_multi_finger_swipe(&mut self, finger_count: u8, timestamp: u64) {
        let gesture_type = match finger_count {
            3 => GestureType::ThreeFingerSwipe,
            4 => GestureType::FourFingerSwipe,
            _ => return,
        };

        // Calculate average movement
        let mut total_dx = 0i32;
        let mut total_dy = 0i32;
        let mut count = 0;

        for point in self.state.points.iter().filter(|p| p.is_valid() && !p.is_palm) {
            let (dx, dy) = point.movement();
            total_dx += dx;
            total_dy += dy;
            count += 1;
        }

        if count == 0 {
            return;
        }

        let avg_dx = total_dx / count;
        let avg_dy = total_dy / count;

        // Check if swipe threshold met
        let distance = libm::sqrt((avg_dx * avg_dx + avg_dy * avg_dy) as f64);
        if distance < SWIPE_MIN_DISTANCE as f64 {
            return;
        }

        // Determine direction
        let direction = if avg_dx.abs() > avg_dy.abs() {
            if avg_dx > 0 {
                SwipeDirection::Right
            } else {
                SwipeDirection::Left
            }
        } else {
            if avg_dy > 0 {
                SwipeDirection::Down
            } else {
                SwipeDirection::Up
            }
        };

        self.emit_gesture(GestureEvent {
            gesture_type,
            state: GestureState::End,
            finger_count,
            delta_x: avg_dx as f64,
            delta_y: avg_dy as f64,
            scale: 1.0,
            rotation: 0.0,
            direction: Some(direction),
            timestamp,
        });
    }

    fn calculate_scroll_delta(&self) -> (f64, f64) {
        // Calculate average movement of all fingers
        let mut total_dx = 0i32;
        let mut total_dy = 0i32;
        let mut count = 0;

        for (i, point) in self.state.points.iter().enumerate() {
            if point.is_valid() && !point.is_palm {
                let start = &self.state.gesture_start_points[i];
                total_dx += point.x - start.x;
                total_dy += point.y - start.y;
                count += 1;
            }
        }

        if count == 0 {
            return (0.0, 0.0);
        }

        // Convert to scroll units (apply sensitivity)
        let dx = (total_dx as f64 / count as f64) * self.config.sensitivity as f64;
        let dy = (total_dy as f64 / count as f64) * self.config.sensitivity as f64;

        (dx, dy)
    }

    fn end_gesture(&mut self, timestamp: u64) {
        if self.state.current_gesture != GestureType::None {
            self.emit_gesture(GestureEvent {
                gesture_type: self.state.current_gesture,
                state: GestureState::End,
                finger_count: 0,
                delta_x: 0.0,
                delta_y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                direction: None,
                timestamp,
            });
        }
        self.state.current_gesture = GestureType::None;
        self.state.scroll_accum = (0.0, 0.0);
    }

    fn emit_gesture(&self, event: GestureEvent) {
        if let Some(callback) = self.gesture_callback {
            callback(event);
        }
    }

    /// Determine click button from position (for clickpads).
    pub fn determine_click_button(&self, x: i32, y: i32) -> ClickButton {
        if !self.info.is_clickpad || !self.config.click_zones.enabled {
            return ClickButton::Left;
        }

        // Normalize position to 0.0-1.0
        let width = (self.info.x_max - self.info.x_min) as f32;
        let height = (self.info.y_max - self.info.y_min) as f32;
        let norm_x = (x - self.info.x_min) as f32 / width;
        let norm_y = (y - self.info.y_min) as f32 / height;

        // Only use zones in bottom area
        if norm_y < (1.0 - self.config.click_zones.bottom_zone_height) {
            return ClickButton::Left;
        }

        // Determine zone
        if norm_x < self.config.click_zones.middle_zone_end {
            ClickButton::Left
        } else if norm_x > self.config.click_zones.right_zone_start {
            ClickButton::Right
        } else {
            ClickButton::Middle
        }
    }
}

/// Click button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickButton {
    Left,
    Middle,
    Right,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_info() -> TouchpadInfo {
        TouchpadInfo {
            name: [0; 64],
            vendor_id: 0,
            product_id: 0,
            protocol: TouchpadProtocol::HidMultiTouch,
            x_res: 100,
            y_res: 100,
            x_min: 0,
            x_max: 1000,
            y_min: 0,
            y_max: 600,
            pressure_max: 255,
            max_slots: 5,
            is_clickpad: true,
            has_buttons: false,
            palm_detect: true,
        }
    }

    #[test]
    fn test_touchpad_driver_creation() {
        let info = create_test_info();
        let driver = TouchpadDriver::new(info);
        assert_eq!(driver.state.active_touch_count(), 0);
    }

    #[test]
    fn test_touch_point() {
        let mut point = TouchPoint::new(1, 100, 200, 1000);
        assert!(point.is_valid());
        assert_eq!(point.distance_from_start(), 0.0);

        point.x = 200;
        point.y = 200;
        let distance = point.distance_from_start();
        assert!(distance > 99.0 && distance < 101.0); // ~100
    }

    #[test]
    fn test_click_zones() {
        let info = create_test_info();
        let driver = TouchpadDriver::new(info);

        // Left zone
        assert_eq!(driver.determine_click_button(100, 550), ClickButton::Left);

        // Middle zone
        assert_eq!(driver.determine_click_button(500, 550), ClickButton::Middle);

        // Right zone
        assert_eq!(driver.determine_click_button(900, 550), ClickButton::Right);

        // Top area (always left)
        assert_eq!(driver.determine_click_button(500, 100), ClickButton::Left);
    }
}
