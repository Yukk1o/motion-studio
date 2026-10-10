//! Editing, undo and expression execution over shared Motion Studio data.
mod editor;
mod expressions;
mod hierarchy;
pub mod plugin_editor;
pub mod storage;

pub use motion_model::{compositing, composition, color_curves, particle_history, masks, vector};
pub use editor::{parse_commands, Engine};
pub use expressions::{ExpressionEvaluator, ProjectExpressions};
pub use motion_model::{Axis, AxisTracks, Ease, Keyframe, SpatialTangents, Track, Tween};
pub use motion_model::{AudioAsset, AudioClip};
pub use motion_model::{
    to_project, to_world, Camera, CameraMode, CameraPose, ObservationView, Observer, ProjectionKind,
};
pub use motion_model::{Composition, CompositionAction, CompositionClip, CompositionSettings, MAIN_COMPOSITION};
pub use motion_model::{Curve, CurveSample, CurveShape, CurveSpace, Easing};
pub use motion_model::{
    CurveLut, CurveObject, CurveTrack, EffectAction, EffectImageInput, EffectImageStage, EffectInstance, EffectParam, PluginDependency,
    SampledEffect,
};
pub use motion_model::{
    FontAsset,
    Asset, Content, Layer, LayerTimeline, ParentLink, Project, Transform, MAX_COMPOSITION_FPS,
    MAX_FRAMES, MAX_LAYERS,
};
pub use motion_model::{TimelineKey, TimelineLayer, TimelineProperties, TimelineTrack};
pub use motion_model::{
    VideoAsset, VideoClip, VideoSample, MAX_VIDEO_DIMENSION, MAX_VIDEO_FPS, MAX_VIDEO_FRAMES,
    MAX_VIDEO_PIXELS,
};
pub use motion_model::{Command, EditResult, Property, Error, Result, ExpressionTarget, ExpressionValue, PropertyExpression, EXPRESSION_PROFILE, MAX_EXPRESSIONS};
pub(crate) use motion_model::{ensure, effects};

pub fn scene_prefix_delta(
    p: &Project,
    object: u64,
    frame: f64,
    delta: [f32; 3],
) -> Result<[f32; 3]> {
    let evaluated = p.evaluated_at(frame)?;
    let mut prefix = hierarchy::prefix(&evaluated, object, frame)?;
    if p.layers.iter().any(|l| l.id == object && !l.three_d) {
        prefix = motion_model::geometry::flat_matrix(prefix);
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
