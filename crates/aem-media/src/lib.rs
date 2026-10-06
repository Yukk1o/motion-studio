//! Owned asynchronous media import, video caches and bounded PCM mixing. No UI dependencies.
mod avc;
mod decode;
mod formats;
mod matroska;
pub use matroska::{matroska_metadata, MatroskaMetadata, MatroskaTrack};
mod jobs;
mod mixer;
mod mp4;
mod video;
mod video_timing;
pub use video_timing::source_frame_rate;
mod video_metadata;
pub use avc::{avc_metadata, AvcMetadata};
pub use decode::{decode_audio, DecodeAudio};
pub use formats::{source_extension, NATIVE_AUDIO_MIMES, VIDEO_MIMES};
pub use jobs::{AudioJobs, ImportOptions, Limits, TaskStatus};
pub use mixer::{read_waveform, AudioMixer, WaveBucket};
pub use video::{
    load_video_index, save_index, video_cache_path, ProbeVideo, VideoImportOptions, VideoJobs,
    VideoProbe, VideoTaskStatus,
};
pub use video_metadata::{hevc_metadata, vp9_metadata};
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
