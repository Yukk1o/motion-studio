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
}
impl<T: Tween> Track<T> {
    fn timeline(&self, offset: i32) -> TimelineTrack<T> {
        TimelineTrack {
            value: self.value,
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
    pub properties: TimelineProperties,
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
                    active: l.active(frame, self.frames),
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
