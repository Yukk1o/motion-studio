//! Stable project data, validation and deterministic sampling; no editing runtime or GPU APIs.
mod animation;
mod audio;
pub mod boundaries;
mod camera;
pub mod composition;
mod curve;
pub mod color_curves;
mod commands;
pub mod effects;
pub mod expressions;
pub mod geometry;
pub mod hierarchy;
mod model;
pub mod particle_history;
pub mod masks;
pub mod compositing;
pub mod storage;
mod timeline;
mod video;
pub mod vector;

pub use boundaries::{EditSink, FrameEvaluator, PackageExportSource, PackagePublication};
pub use commands::{Command, EditResult, Property};
pub use animation::{Axis, AxisTracks, Ease, Keyframe, SpatialTangents, Track, Tween};
pub use audio::{AudioAsset, AudioClip};
pub use camera::{
    to_project, to_world, Camera, CameraMode, CameraPose, ObservationView, Observer, ProjectionKind,
};
pub use composition::{Composition, CompositionAction, CompositionClip, CompositionSettings, MAIN_COMPOSITION};
pub use curve::{Curve, CurveSample, CurveShape, CurveSpace, Easing};
pub use effects::{
    CurveLut, CurveObject, CurveTrack, EffectAction, EffectImageInput, EffectImageStage, EffectInstance, EffectParam, PluginDependency,
    SampledEffect,
};
pub use expressions::{
    ExpressionTarget, ExpressionValue, PropertyExpression, EXPRESSION_PROFILE, MAX_EXPRESSIONS,
};
pub use model::{
    FontAsset,
    Asset, Content, Layer, LayerTimeline, ParentLink, Project, Transform, MAX_COMPOSITION_FPS,
    MAX_FRAMES, MAX_LAYERS,
};
pub use timeline::{TimelineKey, TimelineLayer, TimelineProperties, TimelineTrack};
pub use video::{
    VideoAsset, VideoClip, VideoSample, MAX_VIDEO_DIMENSION, MAX_VIDEO_FPS, MAX_VIDEO_FRAMES,
    MAX_VIDEO_PIXELS,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Composition(#[from] composition::CompositionError),
    #[error("expression {target} at frame {frame}: {message}")]
    Expression {
        target: String,
        frame: f64,
        message: String,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("object {0} does not exist")]
    Missing(u64),
    #[error("object {0} is locked")]
    Locked(u64),
    #[error("edit gesture already active or history unavailable during a gesture")]
    Gesture,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Invalid(message.to_owned()))
    }
}
