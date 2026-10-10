//! Platform-neutral editing host.
//!
//! `aem-host` owns everything that is not a window or a codec: the edit engine,
//! scene sampling, effect planning, GPU preview, media scheduling and the JSON
//! protocol the UI speaks. Android and desktop both build on it, so a capability
//! added here appears on both platforms at once and the desktop application
//! cannot drift behind the Android SDK.
//!
//! ```
//! use std::sync::Arc;
//! use aem_host::{ops::Host, platform::Platform};
//! # struct Desktop; impl Platform for Desktop {
//! #     fn name(&self) -> &'static str { "test" }
//! #     fn probe_video(&self, _: &std::path::Path, _: Option<u32>, _: Option<u32>, _: &dyn Fn() -> aem_media::Result<()>) -> aem_media::Result<aem_media::VideoProbe> { unimplemented!() }
//! #     fn open_decoder(&self, _: &std::path::Path, _: aem_core::VideoAsset, _: Vec<u64>) -> aem_host::Result<Box<dyn aem_host::platform::VideoDecoder>> { unimplemented!() }
//! #     fn decode_audio(&self, _: &std::path::Path, _: &std::path::Path, _: Option<u32>, _: u64, _: &mut dyn FnMut(f64) -> aem_media::Result<()>) -> aem_media::Result<aem_core::AudioAsset> { unimplemented!() }
//! #     fn media_capabilities(&self, _: Option<&aem_host::platform::VideoQuery>) -> aem_host::Result<serde_json::Value> { Ok(serde_json::json!({})) }
//! #     fn attach_surface(&self, _: Arc<dyn wgpu::WindowHandle + Send + Sync>, _: u32, _: u32) -> aem_host::Result<aem_host::platform::SurfaceTarget> { unimplemented!() }
//! # }
//! let host = Host::new(Arc::new(Desktop));
//! assert_eq!(host.platform().name(), "test");
//! ```
#![recursion_limit = "256"]

pub mod ops;
pub mod platform;
pub mod session;
pub mod video_cache;
pub mod video_decode_policy;
pub mod video_frame;
pub mod video_frames;

pub use platform::{Platform, SurfaceTarget, VideoDecoder, VideoQuery};
pub use session::{Result, Session};

/// Error type alias used across the host surface.
pub type SessionError = String;

/// Base64 for plugin editor assets and legacy HTML editor pages.
///
/// The Android bridge used to own this helper because JNI has no byte-array
/// marshalling for these payloads; keeping it here lets both hosts return the
/// same JSON without a second implementation.
pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let a = c[0] as usize;
        let b = c.get(1).copied().unwrap_or(0) as usize;
        let d = c.get(2).copied().unwrap_or(0) as usize;
        out.push(TABLE[a >> 2] as char);
        out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if c.len() > 1 {
            TABLE[((b & 15) << 2) | (d >> 6)] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            TABLE[d & 63] as char
        } else {
            '='
        });
    }
    out
}
