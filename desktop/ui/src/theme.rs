//! Colour palette and metrics for the desktop shell.
//!
//! The layout mirrors After Effects: a near-black panel chrome, one accent
//! colour for selection and focus, and a muted grey scale for type. Values are
//! kept here rather than inline so a future light theme is a data change.

/// Linear RGBA colour with sRGB-encoded components.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color(pub [f32; 4]);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0,
        ])
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a])
    }
    pub const fn grey(v: u8) -> Self {
        Self::rgb(v, v, v)
    }
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self([self.0[0], self.0[1], self.0[2], alpha])
    }
}

/// Chrome colours shared by every panel.
pub mod palette {
    use super::Color;

    /// Window background behind all panels.
    pub const APP_BACKGROUND: Color = Color::rgb(19, 19, 21);
    /// Panel and dock background.
    pub const PANEL: Color = Color::rgb(28, 28, 32);
    /// Recessed areas: timeline background, list wells, viewer surround.
    pub const SUNKEN: Color = Color::rgb(22, 22, 25);
    /// Raised chrome: menu bar, toolbar, tab strip.
    pub const CHROME: Color = Color::rgb(38, 38, 43);
    /// Hairline borders between panels.
    pub const BORDER: Color = Color::rgb(52, 52, 58);
    /// Focus ring and selection accent.
    pub const ACCENT: Color = Color::rgb(84, 220, 199);
    /// Secondary accent for the playhead.
    pub const ACCENT_WARM: Color = Color::rgb(229, 193, 126);
    /// Primary text.
    pub const TEXT: Color = Color::rgb(237, 241, 245);
    /// Secondary text and disabled labels.
    pub const TEXT_MUTED: Color = Color::rgb(154, 162, 174);
    /// Keyframe diamonds in the timeline.
    pub const KEYFRAME: Color = Color::rgb(240, 196, 92);
    /// Track colour for the camera row.
    pub const TRACK_CAMERA: Color = Color::rgb(229, 193, 126);
    /// Alternating layer track colours.
    pub const TRACKS: [Color; 3] = [
        Color::rgb(110, 173, 232),
        Color::rgb(173, 157, 224),
        Color::rgb(103, 191, 175),
    ];
    /// Checkerboard behind transparent pixels.
    pub const TRANSPARENT_LIGHT: Color = Color::rgb(72, 72, 76);
    pub const TRANSPARENT_DARK: Color = Color::rgb(58, 58, 62);
}

/// Panel metrics in logical pixels at 100% UI scale.
pub mod metrics {
    /// Height of the application menu bar.
    pub const MENU_BAR: f32 = 26.0;
    /// Height of the compact tool bar under the menu bar.
    pub const TOOL_BAR: f32 = 34.0;
    /// Height of a panel tab strip.
    pub const TAB_BAR: f32 = 26.0;
    /// Height of one timeline layer row.
    pub const ROW: f32 = 26.0;
    /// Width of the timeline ruler gutter.
    pub const RULER: f32 = 92.0;
    /// Minimum width a dock may be dragged to.
    pub const MIN_DOCK: f32 = 160.0;
    /// Minimum height the timeline may be dragged to.
    pub const MIN_TIMELINE: f32 = 90.0;
    /// Width of a docked splitter grab handle.
    pub const SPLITTER: f32 = 6.0;
    /// Height of a property row in the effect and property panels.
    pub const PROPERTY_ROW: f32 = 22.0;
}

/// UI scale factor. Hosts expose this so a HiDPI display and a large-text
/// preference both map onto one number instead of every widget guessing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scale(pub f32);

impl Default for Scale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Scale {
    pub fn pixels(&self, value: f32) -> f32 {
        value * self.0
    }
    /// Clamp to the range where the dense timeline stays legible.
    pub fn clamped(value: f32) -> Self {
        Self(value.clamp(0.75, 2.0))
    }
}