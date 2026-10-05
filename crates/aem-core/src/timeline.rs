use crate::{Curve, Ease, Project, Track, Tween};
use serde::Serialize;

/// Derived edit data, built only for state responses, never for frame sampling.
#[derive(Debug, Serialize)]
pub struct TimelineKey<T> {
    pub frame: i64,
    pub local_frame: i32,
    pub value: T,
    pub ease: Ease,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}
#[derive(Debug, Serialize)]
pub struct TimelineTrack<T> {
    pub value: T,
    pub keys: Vec<TimelineKey<T>>,
    pub separated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axes: Option<Box<TimelineAxes>>,
}
#[derive(Debug, Serialize)]
pub struct TimelineAxes {
    pub x: TimelineTrack<f32>,
    pub y: TimelineTrack<f32>,
    pub z: TimelineTrack<f32>,
}
impl<T: Tween> Track<T> {
    pub fn timeline(&self, offset: i32) -> TimelineTrack<T> {
        TimelineTrack {
            value: self.static_value(),
            separated: self.axes.is_some(),
            axes: self.axes.as_ref().map(|a| {
                Box::new(TimelineAxes {
                    x: a.x.timeline(offset),
                    y: a.y.timeline(offset),
                    z: a.z.timeline(offset),
                })
            }),
            keys: self
                .keys
                .iter()
                .map(|k| TimelineKey {
                    frame: i64::from(k.frame) + i64::from(offset),
                    local_frame: k.frame,
                    value: k.value,
                    ease: k.ease,
                    curve: k.curve,
                })
                .collect(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct TimelineProperties {
    pub position: TimelineTrack<[f32; 3]>,
    pub rotation: TimelineTrack<[f32; 3]>,
    pub scale: TimelineTrack<[f32; 3]>,
    pub opacity: TimelineTrack<f32>,
}
#[derive(Debug, Serialize)]
pub struct TimelineLayer {
    pub object: u64,
    pub in_frame: u32,
    pub out_frame: u32,
    pub offset_frame: i32,
    pub active: bool,
    pub three_d: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<TimelineAudio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<TimelineVideo>,
    pub properties: TimelineProperties,
}
#[derive(Debug, Serialize)]
pub struct TimelineAudio {
    pub asset: u64,
    pub source_offset_us: u64,
    pub source_time_us: i64,
    pub source_duration_us: u64,
    pub volume: f32,
    pub muted: bool,
    pub spatial_properties: bool,
}
#[derive(Debug, Serialize)]
pub struct TimelineVideo {
    pub asset: u64,
    pub source_time_us: i64,
    pub source_offset_us: u64,
    pub duration_us: u64,
    pub display_width: u32,
    pub display_height: u32,
    pub has_audio: bool,
}
impl Project {
    pub fn timeline_layers(&self, frame: f64) -> Vec<TimelineLayer> {
        self.layers
            .iter()
            .map(|l| {
                let clip = l.clip(self.frames);
                let t = &l.transform;
                TimelineLayer {
                    object: l.id,
                    in_frame: clip.in_frame,
                    out_frame: clip.out_frame,
                    offset_frame: clip.offset_frame,
                    active: if matches!(l.content, crate::Content::Audio { .. }) {
                        frame >= f64::from(clip.in_frame) && frame < f64::from(clip.out_frame)
                    } else {
                        l.active(frame, self.frames)
                    },
                    three_d: l.three_d,
                    audio: if let Some(audio) = self.layer_audio(l) {
                        Some(TimelineAudio {
                            asset: audio.asset,
                            source_offset_us: audio.source_offset_us,
                            source_time_us: audio.source_offset_us as i64
                                + ((frame - f64::from(clip.offset_frame)) * 1_000_000.0
                                    / f64::from(self.fps))
                                .round() as i64,
                            source_duration_us: self
                                .audio_assets
                                .iter()
                                .find(|a| a.id == audio.asset)
                                .map_or(0, |a| a.duration_us),
                            volume: audio.volume,
                            muted: audio.muted,
                            spatial_properties: matches!(l.content, crate::Content::Video { .. }),
                        })
                    } else {
                        None
                    },
                    video: if let crate::Content::Video { video } = &l.content {
                        let a = self
                            .video_assets
                            .iter()
                            .find(|a| a.id == video.asset)
                            .unwrap();
                        Some(TimelineVideo {
                            asset: a.id,
                            source_time_us: video.source_time_us(l.local_frame(frame), self.fps),
                            source_offset_us: video.source_offset_us,
                            duration_us: a.duration_us,
                            display_width: a.display_width,
                            display_height: a.display_height,
                            has_audio: a.audio_asset.is_some(),
                        })
                    } else {
                        None
                    },
                    properties: TimelineProperties {
                        position: t.position.timeline(clip.offset_frame),
                        rotation: t.rotation.timeline(clip.offset_frame),
                        scale: t.scale.timeline(clip.offset_frame),
                        opacity: t.opacity.timeline(clip.offset_frame),
                    },
                }
            })
            .collect()
    }
}
