//! Platform boundary.
//!
//! Everything above this module is pure: project model, composition sampling,
//! effect planning, GPU rendering and media scheduling. Everything below it is
//! supplied by the host application. The Android build implements these
//! capabilities with `AMediaExtractor`/`AMediaCodec` and a `SurfaceView`; the
//! desktop build implements them with libav and a winit window. Because both
//! hosts drive the same [`crate::Session`], the SDK surface that the UI sees
//! (`state`, `sample_render_plan_into`, `plugin`, `media_capabilities`, ...)
//! is identical on both platforms by construction rather than by convention.
use crate::video_frame::DecodedFrame;
use aem_core::{AudioAsset, VideoAsset};
use aem_media::{Result, VideoProbe};
use serde_json::Value;
use std::path::Path;

/// A wgpu surface plus the renderer and configuration chosen for one window.
///
/// The host owns surface creation because only it knows the windowing system.
/// The session owns everything after the surface exists, so preview policy,
/// scratch budgets and presentation are shared verbatim.
pub struct SurfaceTarget {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub renderer: aem_render::Renderer,
}

/// A decoder that can produce frames for one media file.
///
/// `frame` must honour `check` between packets: a preview that scrubs or
/// seeks away cancels the in-flight request instead of decoding to a target
/// nobody will display.
// Decoders are created inside and remain on their dedicated decode worker.
// Android's JNI attachment and codec objects must not cross threads.
pub trait VideoDecoder {
    fn frame(&mut self, target_us: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame>;
    /// Number of seeks performed so far, reported as preview metrics.
    fn seeks(&self) -> u64 {
        0
    }
}

/// A query for the device decoder inventory, mirroring Android's
/// `MediaCodecList` filter so the UI can ask identical questions on both hosts.
#[derive(Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoQuery {
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub frame_rate: Option<f64>,
}

/// Host-provided media, surface and inventory capabilities.
///
/// Implementations must be cheap to call from the session thread; anything
/// expensive (opening codecs, enumerating formats) belongs behind an explicit
/// call rather than in `capabilities`.
pub trait Platform: Send + Sync {
    /// Short identifier recorded in preview and export reports.
    fn name(&self) -> &'static str;

    /// Probe a video container/codec and return the importable asset description.
    fn probe_video(
        &self,
        path: &Path,
        selected: Option<u32>,
        audio_selected: Option<u32>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<VideoProbe>;

    /// Open a decoder for an already imported asset.
    fn open_decoder(
        &self,
        path: &Path,
        asset: VideoAsset,
        pts: Vec<u64>,
    ) -> Result<Box<dyn VideoDecoder>>;

    /// Audio fallback used only when the portable Symphonia path cannot demux
    /// or decode a source. Implementations must never widen a damaged portable
    /// format into a permissive partial decode.
    fn decode_audio(
        &self,
        path: &Path,
        pcm: &Path,
        selected_track: Option<u32>,
        max_pcm_bytes: u64,
        check: &mut dyn FnMut(f64) -> Result<()>,
    ) -> Result<AudioAsset>;

    /// Device decoder inventory in the `media_capabilities` wire shape.
    fn media_capabilities(&self, query: Option<&VideoQuery>) -> Result<Value>;

    /// Create the presentation surface for a window.
    ///
    /// Returning an error is expected when the windowing backend cannot present
    /// on the requested GPU API; the host decides which backends to try.
    fn attach_surface(
        &self,
        window: std::sync::Arc<dyn wgpu::WindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<SurfaceTarget>;
}
