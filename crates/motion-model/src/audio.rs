use crate::{ensure, Result};
use serde::{Deserialize, Serialize};

/// Source metadata, validated by the media service rather than file extensions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioAsset {
    pub id: u64,
    pub path: String,
    pub mime: String,
    pub bytes: u64,
    pub track: u32,
    pub sample_rate: u32,
    pub channels: u32,
    /// Audible frames after decoder delay/padding removal. PCM cache uses this rate.
    pub sample_frames: u64,
    pub duration_us: u64,
}
impl AudioAsset {
    pub fn validate(&self) -> Result<()> {
        crate::storage::validate_relative_path(&self.path)?;
        ensure(
            self.bytes > 0 && self.bytes <= crate::storage::MAX_MEDIA_ASSET,
            "invalid audio source size",
        )?;
        ensure(
            matches!(
                self.mime.as_str(),
                "audio/mp4"
                    | "audio/mpeg"
                    | "audio/wav"
                    | "audio/flac"
                    | "audio/ogg"
                    | "audio/aiff"
                    | "audio/aac"
                    | "audio/opus"
                    | "audio/vorbis"
                    | "audio/3gpp"
                    | "audio/amr-wb"
                    | "audio/x-matroska"
            ),
            "unsupported audio encoding",
        )?;
        ensure(
            (8_000..=192_000).contains(&self.sample_rate) && matches!(self.channels, 1 | 2),
            "audio requires mono/stereo at 8–192 kHz",
        )?;
        ensure(
            self.sample_frames > 0 && self.sample_frames <= u64::from(self.sample_rate) * 3600,
            "audio duration exceeds one hour",
        )?;
        ensure(
            self.duration_us == self.sample_frames * 1_000_000 / u64::from(self.sample_rate),
            "audio duration does not match decoded samples",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClip {
    pub asset: u64,
    pub source_offset_us: u64,
    #[serde(default = "unity")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}
fn unity() -> f32 {
    1.0
}
impl AudioClip {
    pub fn new(asset: u64) -> Self {
        Self {
            asset,
            source_offset_us: 0,
            volume: 1.0,
            muted: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.volume.is_finite() && (0.0..=2.0).contains(&self.volume),
            "audio volume must be between zero and two",
        )?;
        ensure(
            self.source_offset_us <= 3_600_000_000,
            "invalid audio source offset",
        )
    }
}
