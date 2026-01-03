//! # Wayland Compositor
//!
//! A Wayland-compatible compositor for Splax OS.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Wayland Compositor                           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │    Surface Manager    │    Input Handler    │    Output Mgr     │
//! ├───────────────────────┼─────────────────────┼───────────────────┤
//! │   wl_surface          │    wl_seat          │    wl_output      │
//! │   wl_subsurface       │    wl_pointer       │                   │
//! │   xdg_surface         │    wl_keyboard      │                   │
//! └───────────────────────┴─────────────────────┴───────────────────┘
//! ```
//!
//! ## Protocol Support
//!
//! - Core Wayland protocol (wl_*)
//! - XDG Shell protocol (xdg_*)
//! - Linux DMA-BUF protocol
//! - Viewporter protocol

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// =============================================================================
// Object IDs and Types
// =============================================================================

/// Wayland object ID.
pub type ObjectId = u32;

/// Client ID.
pub type ClientId = u32;

/// Wayland object interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    WlDisplay,
    WlRegistry,
    WlCallback,
    WlCompositor,
    WlShm,
    WlShmPool,
    WlBuffer,
    WlSurface,
    WlSubsurface,
    WlSubcompositor,
    WlRegion,
    WlSeat,
    WlPointer,
    WlKeyboard,
    WlTouch,
    WlOutput,
    WlDataDeviceManager,
    WlDataDevice,
    WlDataSource,
    WlDataOffer,
    XdgWmBase,
    XdgSurface,
    XdgToplevel,
    XdgPopup,
    XdgPositioner,
    ZwpLinuxDmabuf,
    ZwpLinuxBufferParams,
    WpViewporter,
    WpViewport,
}

// =============================================================================
// Buffer Types
// =============================================================================

/// Shared memory pool.
pub struct ShmPool {
    /// Pool ID
    id: ObjectId,
    /// Client ID
    client: ClientId,
    /// File descriptor (or memory handle)
    fd: i32,
    /// Pool size
    size: u32,
    /// Mapped address
    data: u64,
}

/// Buffer format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferFormat {
    Argb8888,
    Xrgb8888,
    Rgb565,
    Argb2101010,
    Xrgb2101010,
    Abgr8888,
    Xbgr8888,
    Nv12,
    Yuyv,
}

impl BufferFormat {
    /// Bytes per pixel.
    pub fn bpp(&self) -> u32 {
        match self {
            Self::Argb8888 | Self::Xrgb8888 | Self::Abgr8888 | Self::Xbgr8888 => 4,
            Self::Argb2101010 | Self::Xrgb2101010 => 4,
            Self::Rgb565 => 2,
            Self::Nv12 => 1, // Special handling for YUV
            Self::Yuyv => 2,
        }
    }

    /// To Wayland format code.
    pub fn to_wl_format(&self) -> u32 {
        match self {
            Self::Argb8888 => 0,
            Self::Xrgb8888 => 1,
            Self::Rgb565 => 0x36314752, // DRM_FORMAT_RGB565
            Self::Argb2101010 => 0x30335241,
            Self::Xrgb2101010 => 0x30335258,
            Self::Abgr8888 => 0x34324241,
            Self::Xbgr8888 => 0x34324258,
            Self::Nv12 => 0x3231564E,
            Self::Yuyv => 0x56595559,
        }
    }
}

/// Wayland buffer.
#[derive(Clone)]
pub struct Buffer {
    /// Buffer ID
    pub id: ObjectId,
    /// Client ID
    pub client: ClientId,
    /// Buffer type
    pub buffer_type: BufferType,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Stride
    pub stride: u32,
    /// Format
    pub format: BufferFormat,
    /// Release callback pending
    pub release_pending: bool,
}

/// Buffer backing type.
#[derive(Clone)]
pub enum BufferType {
    Shm { pool: ObjectId, offset: u32 },
    Dmabuf { fd: i32, modifier: u64 },
}

// =============================================================================
// Surface Types
// =============================================================================

/// Surface state.
#[derive(Clone)]
pub struct SurfaceState {
    /// Attached buffer
    pub buffer: Option<ObjectId>,
    /// Buffer offset X
    pub buffer_x: i32,
    /// Buffer offset Y
    pub buffer_y: i32,
    /// Damage regions
    pub damage: Vec<Rect>,
    /// Opaque region
    pub opaque: Option<ObjectId>,
    /// Input region
    pub input: Option<ObjectId>,
    /// Frame callbacks
    pub frame_callbacks: Vec<ObjectId>,
    /// Buffer transform
    pub transform: Transform,
    /// Buffer scale
    pub scale: i32,
}

impl Default for SurfaceState {
    fn default() -> Self {
        Self {
            buffer: None,
            buffer_x: 0,
            buffer_y: 0,
            damage: Vec::new(),
            opaque: None,
            input: None,
            frame_callbacks: Vec::new(),
            transform: Transform::Normal,
            scale: 1,
        }
    }
}

/// Surface transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    FlippedRotate90,
    FlippedRotate180,
    FlippedRotate270,
}

/// Rectangle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width as i32 &&
        py >= self.y && py < self.y + self.height as i32
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width as i32 &&
        self.x + self.width as i32 > other.x &&
        self.y < other.y + other.height as i32 &&
        self.y + self.height as i32 > other.y
    }
}

/// Surface role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    None,
    XdgToplevel,
    XdgPopup,
    Subsurface,
    Cursor,
    DragIcon,
}

/// Wayland surface.
pub struct Surface {
    /// Surface ID
    pub id: ObjectId,
    /// Client ID
    pub client: ClientId,
    /// Current state
    pub current: SurfaceState,
    /// Pending state
    pub pending: SurfaceState,
    /// Role
    pub role: SurfaceRole,
    /// Role object
    pub role_object: Option<ObjectId>,
    /// Parent surface (for subsurfaces)
    pub parent: Option<ObjectId>,
    /// Child subsurfaces
    pub children: Vec<ObjectId>,
    /// Position in parent
    pub position: (i32, i32),
}

impl Surface {
    /// Create a new surface.
    pub fn new(id: ObjectId, client: ClientId) -> Self {
        Self {
            id,
            client,
            current: SurfaceState::default(),
            pending: SurfaceState::default(),
            role: SurfaceRole::None,
            role_object: None,
            parent: None,
            children: Vec::new(),
            position: (0, 0),
        }
    }

    /// Commit pending state.
    pub fn commit(&mut self) {
        // Move pending to current
        if self.pending.buffer.is_some() {
            self.current.buffer = self.pending.buffer.take();
            self.current.buffer_x = self.pending.buffer_x;
            self.current.buffer_y = self.pending.buffer_y;
        }
        
        self.current.damage.extend(self.pending.damage.drain(..));
        self.current.opaque = self.pending.opaque.take().or(self.current.opaque);
        self.current.input = self.pending.input.take().or(self.current.input);
        self.current.frame_callbacks.extend(self.pending.frame_callbacks.drain(..));
        self.current.transform = self.pending.transform;
        self.current.scale = self.pending.scale;
    }
}

// =============================================================================
// XDG Shell Types
// =============================================================================

/// XDG surface state.
#[derive(Clone, Default)]
pub struct XdgSurfaceState {
    /// Geometry
    pub geometry: Option<Rect>,
}

/// XDG toplevel state.
#[derive(Clone)]
pub struct XdgToplevelState {
    /// Title
    pub title: String,
    /// App ID
    pub app_id: String,
    /// Minimum size
    pub min_size: (i32, i32),
    /// Maximum size
    pub max_size: (i32, i32),
    /// Is maximized
    pub maximized: bool,
    /// Is fullscreen
    pub fullscreen: bool,
    /// Is resizing
    pub resizing: bool,
    /// Is activated (focused)
    pub activated: bool,
    /// Is tiled
    pub tiled: TiledState,
}

impl Default for XdgToplevelState {
    fn default() -> Self {
        Self {
            title: String::new(),
            app_id: String::new(),
            min_size: (0, 0),
            max_size: (0, 0),
            maximized: false,
            fullscreen: false,
            resizing: false,
            activated: false,
            tiled: TiledState::empty(),
        }
    }
}

/// Tiled state flags.
#[derive(Clone, Copy, Default)]
pub struct TiledState(u32);

impl TiledState {
    pub const LEFT: u32 = 1 << 0;
    pub const RIGHT: u32 = 1 << 1;
    pub const TOP: u32 = 1 << 2;
    pub const BOTTOM: u32 = 1 << 3;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn contains(&self, flags: u32) -> bool {
        (self.0 & flags) == flags
    }
}

/// XDG toplevel.
pub struct XdgToplevel {
    /// Object ID
    pub id: ObjectId,
    /// XDG surface
    pub xdg_surface: ObjectId,
    /// Current state
    pub current: XdgToplevelState,
    /// Pending state
    pub pending: XdgToplevelState,
    /// Configure serial
    pub configure_serial: u32,
    /// Configured
    pub configured: bool,
}

/// XDG popup.
pub struct XdgPopup {
    /// Object ID
    pub id: ObjectId,
    /// XDG surface
    pub xdg_surface: ObjectId,
    /// Parent surface
    pub parent: ObjectId,
    /// Positioner
    pub positioner: ObjectId,
    /// Geometry
    pub geometry: Rect,
    /// Grabbed
    pub grabbed: bool,
}

// =============================================================================
// Input Types
// =============================================================================

/// Keyboard state.
pub struct KeyboardState {
    /// Focused surface
    pub focus: Option<ObjectId>,
    /// Pressed keys
    pub pressed: Vec<u32>,
    /// Modifiers
    pub modifiers: Modifiers,
    /// Repeat rate
    pub repeat_rate: i32,
    /// Repeat delay
    pub repeat_delay: i32,
}

/// Pointer state.
pub struct PointerState {
    /// Focused surface
    pub focus: Option<ObjectId>,
    /// Position X
    pub x: f64,
    /// Position Y
    pub y: f64,
    /// Pressed buttons
    pub buttons: u32,
}

/// Touch state.
pub struct TouchState {
    /// Touch points
    pub points: BTreeMap<i32, TouchPoint>,
}

/// Touch point.
pub struct TouchPoint {
    /// Surface
    pub surface: ObjectId,
    /// X position
    pub x: f64,
    /// Y position
    pub y: f64,
}

/// Keyboard modifiers.
#[derive(Clone, Copy, Default)]
pub struct Modifiers {
    pub mods_depressed: u32,
    pub mods_latched: u32,
    pub mods_locked: u32,
    pub group: u32,
}

// =============================================================================
// Output Types
// =============================================================================

/// Output mode.
#[derive(Clone)]
pub struct OutputMode {
    /// Width
    pub width: i32,
    /// Height
    pub height: i32,
    /// Refresh rate in mHz
    pub refresh: i32,
    /// Is preferred mode
    pub preferred: bool,
    /// Is current mode
    pub current: bool,
}

/// Output.
pub struct Output {
    /// Object ID
    pub id: ObjectId,
    /// Name
    pub name: String,
    /// Make
    pub make: String,
    /// Model
    pub model: String,
    /// Position X
    pub x: i32,
    /// Position Y
    pub y: i32,
    /// Physical width in mm
    pub physical_width: i32,
    /// Physical height in mm
    pub physical_height: i32,
    /// Subpixel layout
    pub subpixel: Subpixel,
    /// Transform
    pub transform: Transform,
    /// Scale
    pub scale: i32,
    /// Modes
    pub modes: Vec<OutputMode>,
}

/// Subpixel layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subpixel {
    Unknown,
    None,
    HorizontalRgb,
    HorizontalBgr,
    VerticalRgb,
    VerticalBgr,
}

// =============================================================================
// Compositor Core
// =============================================================================

/// Compositor error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorError {
    InvalidObject,
    InvalidClient,
    RoleAlreadySet,
    InvalidRole,
    BufferError,
    ProtocolError,
}

/// Client connection.
pub struct Client {
    /// Client ID
    pub id: ClientId,
    /// Objects owned by client
    pub objects: BTreeMap<ObjectId, Interface>,
    /// Display object (always 1)
    pub display: ObjectId,
    /// Next object ID
    next_id: AtomicU32,
}

impl Client {
    /// Create a new client.
    pub fn new(id: ClientId) -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(1, Interface::WlDisplay);
        
        Self {
            id,
            objects,
            display: 1,
            next_id: AtomicU32::new(2),
        }
    }

    /// Allocate a new object ID.
    pub fn new_id(&self) -> ObjectId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// Main compositor state.
pub struct Compositor {
    /// Surfaces
    surfaces: BTreeMap<(ClientId, ObjectId), Surface>,
    /// Buffers
    buffers: BTreeMap<(ClientId, ObjectId), Buffer>,
    /// XDG toplevels
    toplevels: BTreeMap<(ClientId, ObjectId), XdgToplevel>,
    /// XDG popups
    popups: BTreeMap<(ClientId, ObjectId), XdgPopup>,
    /// Outputs
    outputs: Vec<Output>,
    /// Clients
    clients: BTreeMap<ClientId, Client>,
    /// Keyboard state
    keyboard: KeyboardState,
    /// Pointer state
    pointer: PointerState,
    /// Next client ID
    next_client_id: AtomicU32,
    /// Frame counter
    frame_counter: AtomicU64,
    /// Stacking order (bottom to top)
    stacking_order: Vec<(ClientId, ObjectId)>,
}

impl Compositor {
    /// Create a new compositor.
    pub fn new() -> Self {
        Self {
            surfaces: BTreeMap::new(),
            buffers: BTreeMap::new(),
            toplevels: BTreeMap::new(),
            popups: BTreeMap::new(),
            outputs: Vec::new(),
            clients: BTreeMap::new(),
            keyboard: KeyboardState {
                focus: None,
                pressed: Vec::new(),
                modifiers: Modifiers::default(),
                repeat_rate: 25,
                repeat_delay: 600,
            },
            pointer: PointerState {
                focus: None,
                x: 0.0,
                y: 0.0,
                buttons: 0,
            },
            next_client_id: AtomicU32::new(1),
            frame_counter: AtomicU64::new(0),
            stacking_order: Vec::new(),
        }
    }

    /// Add a new client.
    pub fn add_client(&mut self) -> ClientId {
        let id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        self.clients.insert(id, Client::new(id));
        id
    }

    /// Remove a client.
    pub fn remove_client(&mut self, client: ClientId) {
        // Clean up all client resources
        self.surfaces.retain(|(c, _), _| *c != client);
        self.buffers.retain(|(c, _), _| *c != client);
        self.toplevels.retain(|(c, _), _| *c != client);
        self.popups.retain(|(c, _), _| *c != client);
        self.stacking_order.retain(|(c, _)| *c != client);
        self.clients.remove(&client);
    }

    /// Create a surface.
    pub fn create_surface(&mut self, client: ClientId, id: ObjectId) -> Result<(), CompositorError> {
        let surface = Surface::new(id, client);
        self.surfaces.insert((client, id), surface);
        
        if let Some(c) = self.clients.get_mut(&client) {
            c.objects.insert(id, Interface::WlSurface);
        }
        
        Ok(())
    }

    /// Attach buffer to surface.
    pub fn attach_buffer(&mut self, client: ClientId, surface: ObjectId, buffer: ObjectId, x: i32, y: i32) -> Result<(), CompositorError> {
        let surface = self.surfaces.get_mut(&(client, surface))
            .ok_or(CompositorError::InvalidObject)?;
        
        surface.pending.buffer = Some(buffer);
        surface.pending.buffer_x = x;
        surface.pending.buffer_y = y;
        
        Ok(())
    }

    /// Commit surface.
    pub fn commit_surface(&mut self, client: ClientId, surface: ObjectId) -> Result<(), CompositorError> {
        let surface = self.surfaces.get_mut(&(client, surface))
            .ok_or(CompositorError::InvalidObject)?;
        
        surface.commit();
        
        // Add to stacking order if not present
        let key = (client, surface.id);
        if !self.stacking_order.contains(&key) && surface.role != SurfaceRole::None {
            self.stacking_order.push(key);
        }
        
        Ok(())
    }

    /// Create XDG toplevel.
    pub fn create_xdg_toplevel(&mut self, client: ClientId, id: ObjectId, xdg_surface: ObjectId, surface: ObjectId) -> Result<(), CompositorError> {
        // Set surface role
        let surf = self.surfaces.get_mut(&(client, surface))
            .ok_or(CompositorError::InvalidObject)?;
        
        if surf.role != SurfaceRole::None {
            return Err(CompositorError::RoleAlreadySet);
        }
        
        surf.role = SurfaceRole::XdgToplevel;
        surf.role_object = Some(id);
        
        let toplevel = XdgToplevel {
            id,
            xdg_surface,
            current: XdgToplevelState::default(),
            pending: XdgToplevelState::default(),
            configure_serial: 0,
            configured: false,
        };
        
        self.toplevels.insert((client, id), toplevel);
        
        if let Some(c) = self.clients.get_mut(&client) {
            c.objects.insert(id, Interface::XdgToplevel);
        }
        
        Ok(())
    }

    /// Raise surface to top.
    pub fn raise_surface(&mut self, client: ClientId, surface: ObjectId) {
        let key = (client, surface);
        if let Some(pos) = self.stacking_order.iter().position(|k| *k == key) {
            self.stacking_order.remove(pos);
            self.stacking_order.push(key);
        }
    }

    /// Set keyboard focus.
    pub fn set_keyboard_focus(&mut self, surface: Option<ObjectId>) {
        self.keyboard.focus = surface;
    }

    /// Handle key event.
    pub fn handle_key(&mut self, key: u32, pressed: bool) {
        if pressed {
            if !self.keyboard.pressed.contains(&key) {
                self.keyboard.pressed.push(key);
            }
        } else {
            self.keyboard.pressed.retain(|&k| k != key);
        }
        
        // Send key event to focused surface
        // (Would send wl_keyboard.key event)
    }

    /// Handle pointer motion.
    pub fn handle_pointer_motion(&mut self, x: f64, y: f64) {
        self.pointer.x = x;
        self.pointer.y = y;
        
        // Find surface under pointer
        for (client, surface_id) in self.stacking_order.iter().rev() {
            if let Some(surface) = self.surfaces.get(&(*client, *surface_id)) {
                // Check if pointer is within surface bounds
                let rect = Rect::new(
                    surface.position.0,
                    surface.position.1,
                    100, // Would use actual buffer size
                    100,
                );
                
                if rect.contains(x as i32, y as i32) {
                    self.pointer.focus = Some(*surface_id);
                    break;
                }
            }
        }
    }

    /// Handle pointer button.
    pub fn handle_pointer_button(&mut self, button: u32, pressed: bool) {
        if pressed {
            self.pointer.buttons |= 1 << button;
        } else {
            self.pointer.buttons &= !(1 << button);
        }
        
        // Send button event to focused surface
    }

    /// Render frame.
    pub fn render_frame(&mut self, framebuffer: &mut [u32], width: u32, height: u32) {
        // Clear background
        for pixel in framebuffer.iter_mut() {
            *pixel = 0xFF1E1E2E; // Dark background
        }
        
        let frame = self.frame_counter.fetch_add(1, Ordering::Relaxed);
        
        // Render surfaces in stacking order
        for (client, surface_id) in self.stacking_order.iter() {
            if let Some(surface) = self.surfaces.get(&(*client, *surface_id)) {
                if let Some(buffer_id) = surface.current.buffer {
                    if let Some(buffer) = self.buffers.get(&(*client, buffer_id)) {
                        self.blit_buffer(
                            framebuffer,
                            width,
                            height,
                            buffer,
                            surface.position.0,
                            surface.position.1,
                        );
                    }
                }
                
                // Send frame callbacks
                for callback in &surface.current.frame_callbacks {
                    // Would send wl_callback.done event with timestamp
                    let _ = callback; // Use callback
                }
            }
        }
        
        let _ = frame; // Use frame counter
    }

    /// Blit buffer to framebuffer.
    fn blit_buffer(&self, fb: &mut [u32], fb_width: u32, fb_height: u32, buffer: &Buffer, x: i32, y: i32) {
        // Simple blitting - real implementation would handle various formats
        for row in 0..buffer.height {
            let dst_y = y + row as i32;
            if dst_y < 0 || dst_y >= fb_height as i32 {
                continue;
            }
            
            for col in 0..buffer.width {
                let dst_x = x + col as i32;
                if dst_x < 0 || dst_x >= fb_width as i32 {
                    continue;
                }
                
                let _src_offset = row * buffer.stride + col * 4;
                let dst_offset = dst_y as u32 * fb_width + dst_x as u32;
                
                // Would read from buffer memory
                fb[dst_offset as usize] = 0xFF4CAF50; // Placeholder green
            }
        }
    }

    /// Add output.
    pub fn add_output(&mut self, output: Output) {
        self.outputs.push(output);
    }

    /// Get outputs.
    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Protocol Message Types
// =============================================================================

/// Wayland message header.
#[repr(C)]
pub struct MessageHeader {
    /// Object ID
    pub object_id: u32,
    /// Opcode and size
    pub opcode_size: u32,
}

impl MessageHeader {
    /// Get opcode.
    pub fn opcode(&self) -> u16 {
        (self.opcode_size & 0xFFFF) as u16
    }

    /// Get message size.
    pub fn size(&self) -> u16 {
        ((self.opcode_size >> 16) & 0xFFFF) as u16
    }

    /// Create new header.
    pub fn new(object_id: u32, opcode: u16, size: u16) -> Self {
        Self {
            object_id,
            opcode_size: (size as u32) << 16 | opcode as u32,
        }
    }
}

/// Protocol opcodes for wl_display.
pub mod wl_display {
    pub const ERROR: u16 = 0;
    pub const DELETE_ID: u16 = 1;
    // Requests
    pub const SYNC: u16 = 0;
    pub const GET_REGISTRY: u16 = 1;
}

/// Protocol opcodes for wl_compositor.
pub mod wl_compositor {
    pub const CREATE_SURFACE: u16 = 0;
    pub const CREATE_REGION: u16 = 1;
}

/// Protocol opcodes for wl_surface.
pub mod wl_surface {
    // Events
    pub const ENTER: u16 = 0;
    pub const LEAVE: u16 = 1;
    // Requests
    pub const DESTROY: u16 = 0;
    pub const ATTACH: u16 = 1;
    pub const DAMAGE: u16 = 2;
    pub const FRAME: u16 = 3;
    pub const SET_OPAQUE_REGION: u16 = 4;
    pub const SET_INPUT_REGION: u16 = 5;
    pub const COMMIT: u16 = 6;
    pub const SET_BUFFER_TRANSFORM: u16 = 7;
    pub const SET_BUFFER_SCALE: u16 = 8;
    pub const DAMAGE_BUFFER: u16 = 9;
}

/// Protocol opcodes for xdg_toplevel.
pub mod xdg_toplevel {
    // Events
    pub const CONFIGURE: u16 = 0;
    pub const CLOSE: u16 = 1;
    // Requests
    pub const DESTROY: u16 = 0;
    pub const SET_PARENT: u16 = 1;
    pub const SET_TITLE: u16 = 2;
    pub const SET_APP_ID: u16 = 3;
    pub const SHOW_WINDOW_MENU: u16 = 4;
    pub const MOVE: u16 = 5;
    pub const RESIZE: u16 = 6;
    pub const SET_MAX_SIZE: u16 = 7;
    pub const SET_MIN_SIZE: u16 = 8;
    pub const SET_MAXIMIZED: u16 = 9;
    pub const UNSET_MAXIMIZED: u16 = 10;
    pub const SET_FULLSCREEN: u16 = 11;
    pub const UNSET_FULLSCREEN: u16 = 12;
    pub const SET_MINIMIZED: u16 = 13;
}

// =============================================================================
// Global Instance
// =============================================================================

static COMPOSITOR: Mutex<Option<Compositor>> = Mutex::new(None);

/// Initialize the compositor.
pub fn init() {
    let mut compositor = Compositor::new();
    
    // Add default output
    compositor.add_output(Output {
        id: 1,
        name: String::from("HDMI-A-1"),
        make: String::from("Splax"),
        model: String::from("Virtual Display"),
        x: 0,
        y: 0,
        physical_width: 520,
        physical_height: 320,
        subpixel: Subpixel::None,
        transform: Transform::Normal,
        scale: 1,
        modes: alloc::vec![
            OutputMode {
                width: 1920,
                height: 1080,
                refresh: 60000,
                preferred: true,
                current: true,
            },
            OutputMode {
                width: 1280,
                height: 720,
                refresh: 60000,
                preferred: false,
                current: false,
            },
        ],
    });
    
    *COMPOSITOR.lock() = Some(compositor);
    
    crate::serial_println!("[WAYLAND] Compositor initialized");
}

/// Get the compositor instance.
pub fn compositor() -> &'static Mutex<Option<Compositor>> {
    &COMPOSITOR
}
