//! Input events normalised from winit.
//!
//! The widget layer never sees a winit type, which keeps the layout testable
//! without a window and lets the Android-derived interaction rules (long press
//! to move a keyframe, tap versus drag on the timeline) be expressed once.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Key {
    N,
    O,
    S,
    Z,
    Y,
    D,
    Escape,
    Enter,
    Tab,
    Space,
    Delete,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Comma,
    Period,
    Shift,
    Control,
    Alt,
}

/// A key with its modifiers folded in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Event {
    MouseMoved {
        position: [f32; 2],
    },
    MousePressed {
        position: [f32; 2],
        button: MouseButton,
    },
    MouseReleased {
        position: [f32; 2],
        button: MouseButton,
    },
    MouseWheel {
        delta: [f32; 2],
    },
    KeyPressed {
        key: Key,
        modifiers: Modifiers,
    },
    TextInput(String),
    /// A press held past the long-press threshold; the timeline and preview use
    /// this to distinguish a scrub from an intended clip move.
    LongPress {
        position: [f32; 2],
    },
    FocusLost,
}

/// Accumulated input for one frame.
#[derive(Default, Clone)]
pub struct Input {
    pub held: Option<MouseButton>,
    pub mouse: [f32; 2],
    pub pressed: Option<(MouseButton, [f32; 2])>,
    pub released: Option<(MouseButton, [f32; 2])>,
    pub wheel: [f32; 2],
    pub modifiers: Modifiers,
    pub long_press: Option<[f32; 2]>,
    pub keys: Vec<(Key, Modifiers)>,
    pub text: String,
}

impl Input {
    pub fn begin_frame(&mut self) {
        self.pressed = None;
        self.released = None;
        self.wheel = [0.0, 0.0];
        self.long_press = None;
        self.keys.clear();
        self.text.clear();
    }

    /// True while the primary button is held, whether or not it moved.
    pub fn dragging(&self) -> bool {
        self.held == Some(MouseButton::Left)
    }
}

/// A rectangular region with hit testing.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            min: [x, y],
            max: [x + width, y + height],
        }
    }
    pub fn width(&self) -> f32 {
        self.max[0] - self.min[0]
    }
    pub fn height(&self) -> f32 {
        self.max[1] - self.min[1]
    }
    pub fn contains(&self, point: [f32; 2]) -> bool {
        point[0] >= self.min[0]
            && point[0] < self.max[0]
            && point[1] >= self.min[1]
            && point[1] < self.max[1]
    }
    /// True when the point is within `slack` of the horizontal edge.
    pub fn near_edge(&self, point: [f32; 2], slack: f32) -> bool {
        point[1] >= self.min[1] && point[1] < self.max[1] && (point[0] - self.min[0]).abs() <= slack
    }
    pub fn contains_rect(&self, other: Rect) -> bool {
        other.min[0] >= self.min[0]
            && other.min[1] >= self.min[1]
            && other.max[0] <= self.max[0]
            && other.max[1] <= self.max[1]
    }
    /// Shrink symmetrically, used to inset widget contents from panel chrome.
    pub fn shrink(&self, amount: f32) -> Self {
        Self {
            min: [self.min[0] + amount, self.min[1] + amount],
            max: [self.max[0] - amount, self.max[1] - amount],
        }
    }
}

/// Scroll state for a panel that owns a scroll offset.
#[derive(Clone, Copy, Default, Debug)]
pub struct Scroll {
    pub offset: f32,
    /// Accumulated but unapplied wheel delta, in the same units as `offset`.
    pending: f32,
    pub max: f32,
}

impl Scroll {
    /// Recompute the scrollable distance without moving the offset.
    pub fn set_extent(&mut self, viewport: f32, content: f32) {
        self.max = (content - viewport).max(0.0);
        self.offset = self.offset.clamp(0.0, self.max);
    }

    /// Consume this frame's wheel delta, clamped to the available range.
    pub fn consume(&mut self, wheel: f32, viewport: f32, content: f32) {
        self.pending += wheel;
        self.set_extent(viewport, content);
        self.offset = (self.offset - self.pending).clamp(0.0, self.max);
        self.pending = 0.0;
    }
    /// Total scrollable distance, used to draw the scrollbar thumb.
    pub fn bar(&self, viewport: f32, content: f32) -> (f32, f32) {
        if content <= viewport || content <= 0.0 {
            return (0.0, 0.0);
        }
        let fraction = (viewport / content).clamp(0.05, 1.0);
        let travel = self.max;
        (fraction, (self.offset / travel.max(1.0)).clamp(0.0, 1.0))
    }
}
