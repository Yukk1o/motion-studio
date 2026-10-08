//! Deterministic animation and project state, independent of UI and GPU APIs.
mod animation;
mod audio;
mod camera;
mod compositor;
pub mod composition;
mod curve;
pub mod color_curves;
mod editor;
mod effects;
mod expressions;
mod hierarchy;
mod model;
pub mod plugin_editor;
mod scene;
pub mod storage;
mod timeline;
mod video;
pub mod vector;

pub use animation::{Axis, AxisTracks, Ease, Keyframe, Track, Tween};
pub use audio::{AudioAsset, AudioClip};
pub use camera::{
    to_project, to_world, Camera, CameraMode, CameraPose, ObservationView, Observer, ProjectionKind,
};
pub use compositor::{PlaneBatch, PlaneCompositor, PlaneVertex};
pub use composition::{Composition, CompositionAction, CompositionClip, CompositionSettings, MAIN_COMPOSITION};
pub use curve::{Curve, CurveSample, CurveShape, CurveSpace, Easing};
pub use editor::{parse_commands, Command, EditResult, Engine, Property};
pub use effects::{
    CurveLut, CurveObject, CurveTrack, EffectAction, EffectInstance, EffectParam, PluginDependency,
    SampledEffect,
};
pub use expressions::{
    ExpressionTarget, ExpressionValue, PropertyExpression, EXPRESSION_PROFILE, MAX_EXPRESSIONS,
};
pub use model::{
    Asset, Content, Layer, LayerTimeline, ParentLink, Project, Transform, MAX_COMPOSITION_FPS,
    MAX_FRAMES, MAX_LAYERS,
};
pub use scene::{DrawLayer, HitCandidate, Scene};
pub use timeline::{TimelineKey, TimelineLayer, TimelineProperties, TimelineTrack};
pub use video::{
    VideoAsset, VideoClip, VideoSample, MAX_VIDEO_DIMENSION, MAX_VIDEO_FPS, MAX_VIDEO_FRAMES,
    MAX_VIDEO_PIXELS,
};
pub fn scene_prefix_delta(
    p: &Project,
    object: u64,
    frame: f64,
    delta: [f32; 3],
) -> Result<[f32; 3]> {
    let evaluated = p.evaluated_at(frame)?;
    let mut prefix = hierarchy::prefix(&evaluated, object, frame)?;
    if p.layers.iter().any(|l| l.id == object && !l.three_d) {
        prefix = scene::flat_matrix(prefix);
    }
    ensure(
        prefix.determinant().abs() > 1e-8,
        "cannot drag through a zero-scale parent",
    )?;
    let local = prefix
        .inverse()
        .transform_vector3(glam::Vec3::new(delta[0], -delta[1], -delta[2]));
    Ok([local.x, -local.y, -local.z])
}

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

pub(crate) fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Invalid(message.to_owned()))
    }
}
