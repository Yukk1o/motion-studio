//! Shared asset path and size constraints; project/file publication stays in core.
use crate::{ensure, Result};

pub const MAX_MEDIA_ASSET: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PACKAGE: u64 = 4 * 1024 * 1024 * 1024;
/// The payload budget plus bounded ZIP headers/compression overhead.
pub const MAX_PACKAGE_ARCHIVE: u64 = MAX_PACKAGE + 4 * 1024 * 1024;
pub const TRANSFER_BUFFER_BYTES: usize = 64 * 1024;

pub fn validate_relative_path(path: &str) -> Result<()> {
    ensure(
        path.len() <= 1024 && path.starts_with("assets/"),
        "assets must be inside the assets directory",
    )?;
    ensure(
        !path.contains('\\') && !path.contains(':') && !path.contains('\0'),
        "invalid asset path",
    )?;
    ensure(
        path.split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".."),
        "invalid asset path component",
    )
}
