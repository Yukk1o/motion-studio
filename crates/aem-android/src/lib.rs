//! Android platform boundary. Native runtime integration is the next implementation step.
#![recursion_limit = "256"]
pub use aem_core::Engine;
#[cfg(any(target_os = "android", test))]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod video_frame;
#[cfg(any(target_os = "android", test))]
mod video_cache;
#[cfg(target_os = "android")]
mod runtime;
