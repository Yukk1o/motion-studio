//! Owned, asynchronous audio import and bounded PCM mixing. No UI dependencies.
mod decode;
mod jobs;
mod mixer;
mod mp4;
pub use jobs::{AudioJobs, ImportOptions, Limits, TaskStatus};
pub use mixer::{read_waveform, AudioMixer, WaveBucket};
pub type Result<T> = std::result::Result<T, String>;
pub const OUTPUT_RATE: u64 = 48_000;
pub const MAX_BLOCK_FRAMES: usize = 48_000;
pub const PCM_VERSION: u32 = 1;

pub fn cache_path(
    root: &std::path::Path,
    asset: &aem_core::AudioAsset,
) -> Result<std::path::PathBuf> {
    aem_core::storage::validate_relative_path(&asset.path).map_err(|e| e.to_string())?;
    let stem = std::path::Path::new(&asset.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid audio source name")?;
    Ok(root.join("cache/audio-v1").join(format!("{stem}.pcm")))
}

pub(crate) fn contained_dir(root: &std::path::Path, path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    if !path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("media directory resolves outside project".into());
    }
    Ok(())
}
