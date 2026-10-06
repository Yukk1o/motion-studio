use crate::{ensure, Result};
use serde::{Deserialize, Serialize};

/// All timestamps are on the source presentation timeline, including leading edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoAsset {
    pub id: u64,
    pub path: String,
    pub bytes: u64,
    pub mime: String,
    pub track: u32,
    pub width: u32,
    pub height: u32,
    pub rotation: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub video_start_us: u64,
    pub video_end_us: u64,
    pub duration_us: u64,
    pub frame_count: u32,
    pub variable_frame_rate: bool,
    pub nominal_frame_rate: f64,
    pub color_standard: u32,
    pub color_range: u32,
    /// A separate asset ID referencing the same owned source; never a second copy.
    pub audio_asset: Option<u64>,
}
impl VideoAsset {
    pub fn validate(&self) -> Result<()> {
        crate::storage::validate_relative_path(&self.path)?;
        ensure(
            self.bytes > 0 && self.bytes <= crate::storage::MAX_MEDIA_ASSET,
            "invalid video source size",
        )?;
        ensure(
            matches!(
                self.mime.as_str(),
                "video/avc" | "video/hevc" | "video/x-vnd.on2.vp8" | "video/x-vnd.on2.vp9"
            ),
            "unsupported video codec",
        )?;
        ensure(
            self.width > 0
                && self.height > 0
                && self.width <= 1920
                && self.height <= 1920
                && u64::from(self.width) * u64::from(self.height) <= 1920 * 1080,
            "video exceeds 1080p pixel budget",
        )?;
        ensure(
            matches!(self.rotation, 0 | 90 | 180 | 270),
            "unsupported video rotation",
        )?;
        let expected = if self.rotation % 180 == 0 {
            (self.width, self.height)
        } else {
            (self.height, self.width)
        };
        ensure(
            (self.display_width, self.display_height) == expected,
            "video display size mismatch",
        )?;
        ensure(
            self.video_start_us < self.video_end_us
                && self.video_end_us <= self.duration_us
                && self.duration_us <= 3_600_000_000
                && self.frame_count > 0
                && self.frame_count <= 500_000,
            "invalid video presentation interval",
        )?;
        ensure(
            self.nominal_frame_rate.is_finite()
                && self.nominal_frame_rate > 0.0
                && self.nominal_frame_rate <= 120.0,
            "video exceeds 120 fps",
        )?;
        ensure(
            matches!(self.color_standard, 1 | 2 | 4) && matches!(self.color_range, 1 | 2),
            "unsupported SDR color space",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoClip {
    pub asset: u64,
    #[serde(default)]
    pub source_offset_us: u64,
    #[serde(default = "unity")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}
fn unity() -> f32 {
    1.0
}
impl VideoClip {
    pub fn new(asset: u64) -> Self {
        Self {
            asset,
            source_offset_us: 0,
            volume: 1.0,
            muted: false,
        }
    }
    pub fn source_time_us(&self, local_frame: f64, fps: u32) -> i64 {
        self.source_offset_us as i64 + (local_frame * 1_000_000.0 / f64::from(fps)).round() as i64
    }
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.source_offset_us <= 3_600_000_000
                && self.volume.is_finite()
                && (0.0..=2.0).contains(&self.volume),
            "invalid video source offset or volume",
        )
    }
}

#[derive(Clone, Debug)]
pub struct VideoSample {
    pub asset: u64,
    pub source_time_us: u64,
}
