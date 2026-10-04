//! Native GPU rendering. Image decoding/upload happens on resource changes;
//! playback uploads only fixed-size layer uniforms.
mod renderer;
pub use aem_core::Scene;
pub use renderer::{CaptureTarget, RenderError, RenderStats, Renderer};
