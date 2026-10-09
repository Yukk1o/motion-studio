//! Desktop widget toolkit for the Motion Studio shell.
//!
//! The shell is a native winit/wgpu application rather than a web view, so the
//! composition preview is presented by the engine's own swapchain with no copy
//! and no IPC hop. Panels are drawn here through a fixed two-pass pipeline:
//!
//! 1. [`paint`] records solid shapes and atlas-sampled glyphs.
//! 2. [`dock`] resolves an After Effects style tab/dock tree into rectangles.
//! 3. [`ui`] draws widgets and reports interaction through [`input`].
//!
//! Keeping the layout model free of rendering means a layout test needs no GPU,
//! which is why [`dock`] carries its own assertions.

pub mod dock;
pub mod input;
pub mod paint;
pub mod text;
pub mod theme;
pub mod ui;

pub use input::{Input, Key, MouseButton, Rect};
pub use paint::{PaintList, Painter};
pub use text::TextAtlas;
pub use theme::{Color, Scale};