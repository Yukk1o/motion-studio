use crate::{ensure, Axis, CameraMode, Error, Project, Property, Result, Track};
use serde::{Deserialize, Serialize};

pub const EXPRESSION_PROFILE: &str = "motion-studio-ae-js-1";
pub const MAX_EXPRESSIONS: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionTarget {
    Property {
        object: u64,
        property: Property,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<Axis>,
    },
    Effect {
        object: u64,
        effect: u64,
        param: String,
    },
}
impl ExpressionTarget {
    pub fn object(&self) -> u64 {
        match self {
            Self::Property { object, .. } | Self::Effect { object, .. } => *object,
        }
    }
    pub fn set_object(&mut self, id: u64) {
        match self {
            Self::Property { object, .. } | Self::Effect { object, .. } => *object = id,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyExpression {
    pub target: ExpressionTarget,
    pub source: String,
    pub enabled: bool,
    #[serde(default)]
    pub seed: u32,
    pub profile: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ExpressionValue {
    pub target: ExpressionTarget,
    pub value: Vec<f32>,
}

#[derive(Clone)]
pub enum RawTrack {
    Scalar(Track<f32>),
    Vector(Track<[f32; 3]>),
    Component(Track<[f32; 3]>, usize),
    Param(Track<[f32; 4]>, usize),
}
impl RawTrack {
    pub fn sample(&self, frame: f64) -> Vec<f32> {
        match self {
            Self::Scalar(t) => vec![t.sample(frame)],
            Self::Vector(t) => t.sample(frame).to_vec(),
            Self::Component(t, axis) => vec![t.sample(frame)[*axis]],
            Self::Param(t, n) => t.sample(frame)[..*n].to_vec(),
        }
    }
    pub fn frames(&self) -> Vec<i32> {
        let mut frames: Vec<_> = match self {
            Self::Scalar(t) => t.key_frames().collect(),
            Self::Vector(t) => t.key_frames().collect(),
            Self::Component(t, _) => t.key_frames().collect(),
            Self::Param(t, _) => t.key_frames().collect(),
        };
        frames.sort_unstable();
        frames.dedup();
        frames
    }
}
fn vector(t: &Track<[f32; 3]>, axis: Option<Axis>) -> RawTrack {
    if let Some(axis) = axis {
        if let Some(a) = &t.axes {
            return RawTrack::Scalar(a.get(axis).clone());
        }
        RawTrack::Component(t.clone(), axis.index())
    } else {
        RawTrack::Vector(t.clone())
    }
}
pub fn raw(project: &Project, target: &ExpressionTarget) -> Result<RawTrack> {
    match target {
        ExpressionTarget::Property {
            object,
            property,
            axis,
        } => {
            let result = if *object == 0 {
                ensure(project.camera.created, "camera does not exist")?;
                let c = &project.camera;
                match property {
                    Property::Position => {
                        ensure(
                            c.mode == CameraMode::Position,
                            "camera position requires position mode",
                        )?;
                        vector(&c.position, *axis)
                    }
                    Property::Target => vector(&c.target, *axis),
                    Property::Roll => RawTrack::Scalar(c.roll.clone()),
                    Property::Fov => RawTrack::Scalar(c.fov.clone()),
                    Property::Radius => RawTrack::Scalar(c.radius.clone()),
                    Property::Azimuth => RawTrack::Scalar(c.azimuth.clone()),
                    Property::Elevation => RawTrack::Scalar(c.elevation.clone()),
                    _ => {
                        return Err(Error::Invalid(
                            "unsupported camera expression property".into(),
                        ))
                    }
                }
            } else {
                let l = project
                    .layers
                    .iter()
                    .find(|l| l.id == *object)
                    .ok_or(Error::Missing(*object))?;
                match property {
                    Property::Position => vector(&l.transform.position, *axis),
                    Property::Rotation => vector(&l.transform.rotation, *axis),
                    Property::Scale => vector(&l.transform.scale, *axis),
                    Property::Opacity => RawTrack::Scalar(l.transform.opacity.clone()),
                    _ => {
                        return Err(Error::Invalid(
                            "unsupported layer expression property".into(),
                        ))
                    }
                }
            };
            ensure(
                axis.is_none()
                    || matches!(
                        property,
                        Property::Position
                            | Property::Rotation
                            | Property::Scale
                            | Property::Target
                    ),
                "scalar property has no axis",
            )?;
            Ok(result)
        }
        ExpressionTarget::Effect {
            object,
            effect,
            param,
        } => {
            let l = project
                .layers
                .iter()
                .find(|l| l.id == *object)
                .ok_or(Error::Missing(*object))?;
            let p = l
                .effects
                .iter()
                .find(|e| e.id == *effect)
                .and_then(|e| e.params.get(param))
                .ok_or_else(|| {
                    Error::Invalid("expression effect parameter does not exist".into())
                })?;
            ensure(
                !p.kind.discrete()
                    && p.kind != motion_effects::ParamKind::Curve
                    && p.curve.is_none()
                    && p.animatable
                    && p.implemented,
                "expressions require an implemented, animatable continuous effect parameter",
            )?;
            Ok(RawTrack::Param(p.track.clone(), p.kind.dimensions()))
        }
    }
}
pub fn active(project: &Project, expression: &PropertyExpression) -> bool {
    expression.enabled
        && match &expression.target {
            ExpressionTarget::Effect { object, effect, .. } => project
                .layers
                .iter()
                .find(|l| l.id == *object)
                .and_then(|l| l.effects.iter().find(|e| e.id == *effect))
                .is_some_and(|e| e.enabled),
            _ => true,
        }
}
pub(crate) fn validate(project: &Project) -> Result<()> {
    ensure(
        project.expressions.len() <= MAX_EXPRESSIONS,
        "expression limit exceeded",
    )?;
    ensure(
        project.version >= 3 || project.expressions.is_empty(),
        "expressions require project format 3",
    )?;
    for (i, e) in project.expressions.iter().enumerate() {
        ensure(
            e.profile == EXPRESSION_PROFILE,
            "unsupported expression profile",
        )?;
        ensure(
            !e.source.trim().is_empty() && e.source.len() <= 8192,
            "expression source must be 1..8192 bytes",
        )?;
        raw(project, &e.target)?;
        for other in &project.expressions[..i] {
            ensure(other.target != e.target, "duplicate expression target")?;
            if let (
                ExpressionTarget::Property {
                    object: a,
                    property: p,
                    axis: x,
                },
                ExpressionTarget::Property {
                    object: b,
                    property: q,
                    axis: y,
                },
            ) = (&e.target, &other.target)
            {
                ensure(
                    a != b || p != q || (x.is_some() && y.is_some()),
                    "whole property and axis expressions cannot overlap",
                )?;
            }
        }
    }
    Ok(())
}
