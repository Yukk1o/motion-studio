//! Android platform adapter.
//!
//! All JNI marshalling lives in [`jni_api`]; this crate otherwise contains the
//! Android media and surface backends that implement [`motion_host::Platform`].
//! On non-Android targets the crate is empty, because the desktop application
//! provides its own backends. Every editing operation lives in `motion-host`, which
//! is why the desktop application cannot drift behind the Android SDK surface.
#![recursion_limit = "256"]

pub use motion_core::Engine;
pub use motion_host::{Platform, Session};

#[cfg(target_os = "android")]
mod android_window;
#[cfg(target_os = "android")]
mod audio_decode;
#[cfg(target_os = "android")]
mod bridge;
#[cfg(target_os = "android")]
mod media_capabilities;
#[cfg(target_os = "android")]
mod platform;
#[cfg(target_os = "android")]
mod uri;
#[cfg(target_os = "android")]
mod video_decode;

#[cfg(target_os = "android")]
mod jni_api;

#[cfg(target_os = "android")]
pub use android_window::AndroidWindow;
#[cfg(target_os = "android")]
pub use platform::{current, install, AndroidPlatform};