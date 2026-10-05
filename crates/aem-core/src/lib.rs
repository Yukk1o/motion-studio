//! Deterministic animation and project state, independent of UI and GPU APIs.
mod animation;
mod camera;
mod curve;
mod editor;
mod effects;
mod hierarchy;
mod model;
mod scene;
pub mod storage;

pub use animation::{Ease, Keyframe, Track, Tween};
pub use camera::{to_project, to_world, Camera, CameraMode, CameraPose, ObservationView, Observer};
pub use curve::{Curve, CurveSample, CurveShape, CurveSpace, Easing};
pub use editor::{Command, Engine, Property};
pub use effects::{
    CurveLut, CurveObject, CurveTrack, EffectAction, EffectInstance, EffectParam, PluginDependency,
    SampledEffect,
};
pub use model::{Asset, Content, Layer, ParentLink, Project, Transform, MAX_FRAMES, MAX_LAYERS};
pub use scene::{DrawLayer, Scene};
pub fn scene_prefix_delta(
    p: &Project,
    object: u64,
    frame: f64,
    delta: [f32; 3],
) -> Result<[f32; 3]> {
    let prefix = hierarchy::prefix(p, object, frame)?;
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
