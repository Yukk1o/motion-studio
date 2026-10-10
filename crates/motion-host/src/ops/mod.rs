//! Versioned editing operations shared by every host.
//!
//! Each function takes a [`Session`] and returns the JSON payload the UI
//! receives. Android wraps these in JNI entry points; the desktop shell calls
//! them through [`Host`]. Keeping the wire shapes identical is what makes the
//! two applications SDK-equivalent: the desktop UI cannot accidentally depend on
//! a capability that only exists on one platform.
use crate::platform::Platform;
use crate::session::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub mod composition;
pub mod editing;
pub mod effects;
pub mod export;
pub mod geometry;
pub mod fonts;
pub mod images;
pub mod media;
pub mod preview;
pub mod preview_inputs;
pub mod project;
pub mod snapshot;

/// A host application bound to one platform backend.
///
/// Every editing operation lives on this type, so the desktop shell and the
/// Android bridge call exactly the same code with the same wire formats.
pub struct Host {
    platform: Arc<dyn Platform>,
}

impl Host {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    pub fn create(&self, root: std::path::PathBuf, project_text: &str) -> Result<i64> {
        project::create(root, project_text, self.platform.clone())
    }
}

/// Standard `{"ok":…}` envelope shared with the Android bridge.
pub fn envelope<T: serde::Serialize>(value: T) -> Value {
    json!({"ok": true, "data": value})
}

/// Failure envelope. `composition_error:` payloads are also decoded into
/// `error_detail` so hosts can map structured engine errors to localized text.
pub fn error_envelope(error: &str) -> Value {
    json!({
        "ok": false,
        "error": error,
        "error_detail": error
            .strip_prefix("composition_error:")
            .and_then(|v| serde_json::from_str::<Value>(v).ok()),
    })
}

/// Wrap a host operation in the envelope and in a panic boundary.
///
/// Every host entry point routes through this so a panic in one operation can
/// never take down the process or leave a session wedged.
pub fn dispatch(operation: impl FnOnce() -> Result<Value>) -> Value {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => envelope(value),
        Ok(Err(error)) => error_envelope(&error),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown native error".into());
            error_envelope(&reason.chars().take(2048).collect::<String>())
        }
    }
}

/// Parse a host request body, rejecting oversized payloads before any work.
pub fn parse_request(text: &str, limit: usize, what: &str) -> Result<Value> {
    if text.len() > limit {
        return Err(format!("{what} too large"));
    }
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// Nanosecond suffix used for import/export sibling directories.
pub fn stamp() -> Result<u128> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())
        .map(|d| d.as_nanos())
}
