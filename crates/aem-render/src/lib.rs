//! Native GPU rendering. Image decoding/upload happens on resource changes;
//! playback uploads only fixed-size layer uniforms.
mod presenter;
mod renderer;
pub use aem_core::Scene;
pub use presenter::Presenter;
pub use renderer::premultiply_pixels;
pub use renderer::{CaptureTarget, RenderError, RenderStats, Renderer};
