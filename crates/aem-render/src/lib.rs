//! Native GPU rendering. Image decoding/upload happens on resource changes;
//! Video instances reuse bounded dynamic textures, uploading changed source frames.
mod measurement;
mod presenter;
mod quality;
mod renderer;
mod timing;
pub use aem_core::Scene;
pub use measurement::{FrameMeasurement, FrameRecorder};
pub use presenter::Presenter;
pub use quality::{PreviewMode, PreviewPolicy, PreviewTier};
pub use renderer::premultiply_pixels;
pub use renderer::{CaptureTarget, RenderError, RenderStats, RenderTarget, Renderer};
pub use timing::{GpuTimer, GpuTiming};
