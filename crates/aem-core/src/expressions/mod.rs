mod runtime;
mod transpile;

use crate::{ensure, Axis, CameraMode, Error, Project, Property, Result, Track};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    borrow::Cow,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

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
    pub(crate) fn set_object(&mut self, id: u64) {
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
enum RawTrack {
    Scalar(Track<f32>),
    Vector(Track<[f32; 3]>),
    Param(Track<[f32; 4]>, usize),
}
impl RawTrack {
    fn sample(&self, frame: f64) -> Vec<f32> {
        match self {
            Self::Scalar(t) => vec![t.sample(frame)],
            Self::Vector(t) => t.sample(frame).to_vec(),
            Self::Param(t, n) => t.sample(frame)[..*n].to_vec(),
        }
    }
    fn frames(&self) -> Vec<i32> {
        let mut frames: Vec<_> = match self {
            Self::Scalar(t) => t.key_frames().collect(),
            Self::Vector(t) => t.key_frames().collect(),
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
        let mut scalar = Track::constant(t.value[axis.index()]);
        scalar.keys = t
            .keys
            .iter()
            .map(|k| crate::Keyframe {
                frame: k.frame,
                value: k.value[axis.index()],
                ease: k.ease,
                curve: k.curve,
            })
            .collect();
        RawTrack::Scalar(scalar)
    } else {
        RawTrack::Vector(t.clone())
    }
}
fn raw(project: &Project, target: &ExpressionTarget) -> Result<RawTrack> {
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
                    && p.kind != aem_effects::ParamKind::Curve
                    && p.curve.is_none()
                    && p.animatable
                    && p.implemented,
                "expressions require an implemented, animatable continuous effect parameter",
            )?;
            Ok(RawTrack::Param(p.track.clone(), p.kind.dimensions()))
        }
    }
}
fn active(project: &Project, expression: &PropertyExpression) -> bool {
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
fn error(target: &ExpressionTarget, frame: f64, error: Error) -> Error {
    Error::Expression {
        target: serde_json::to_string(target).unwrap_or_default(),
        frame,
        message: error.to_string(),
    }
}
fn evaluate(
    project: &Project,
    e: &PropertyExpression,
    frame: f64,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<f32>> {
    let track = raw(project, &e.target)?;
    let layer = project.layers.iter().find(|l| l.id == e.target.object());
    let offset = layer.map_or(0, |l| l.clip(project.frames).offset_frame);
    let fps = project.fps as f64;
    let percent = matches!(
        e.target,
        ExpressionTarget::Property {
            property: Property::Opacity,
            ..
        }
    );
    let js_value = move |v: Vec<f32>| {
        let v: Vec<f32> = v
            .into_iter()
            .map(|v| if percent { v * 100.0 } else { v })
            .collect();
        if v.len() == 1 {
            json!(v[0])
        } else {
            json!(v)
        }
    };
    let local = frame - f64::from(offset);
    let keys:Vec<_>=track.frames().iter().map(|&f|json!({"time":(f64::from(f)+f64::from(offset))/fps,"value":js_value(track.sample(f64::from(f)))})).collect();
    let index = layer.map_or(0, |l| {
        project.layers.len() - project.layers.iter().position(|v| v.id == l.id).unwrap()
    });
    let clip = layer.map(|l| l.clip(project.frames));
    let seed = e.seed
        ^ (e.target.object() as u32).wrapping_mul(0x9e3779b9)
        ^ serde_json::to_string(&e.target)?
            .bytes()
            .fold(2166136261u32, |s, b| {
                (s ^ u32::from(b)).wrapping_mul(16777619)
            });
    let data=json!({"time":frame/fps,"value":js_value(track.sample(local)),"index":index,"keys":keys,"seed":seed,
        "comp":{"width":project.width,"height":project.height,"frameRate":project.fps,"frameDuration":1.0/fps,"duration":project.frames as f64/fps},
        "layer":{"name":layer.map_or("Camera",|l|l.name.as_str()),"index":index,"inPoint":clip.map_or(0.0,|c|c.in_frame as f64/fps),"outPoint":clip.map_or(project.frames as f64/fps,|c|c.out_frame as f64/fps),"startTime":offset as f64/fps}}).to_string();
    runtime::evaluate(
        &e.source,
        &data,
        move |t| js_value(track.sample(t * fps - offset as f64)).to_string(),
        deadline,
        cancel,
    )
    .map_err(|err| error(&e.target, frame, err))
}
fn write(
    project: &mut Project,
    target: &ExpressionTarget,
    frame: f64,
    result: &[f32],
) -> Result<()> {
    fn scalar(t: &mut Track<f32>, v: &[f32]) -> Result<()> {
        ensure(v.len() == 1, "expression must return a scalar")?;
        *t = Track::constant(v[0]);
        Ok(())
    }
    fn vec(t: &mut Track<[f32; 3]>, axis: Option<Axis>, f: f64, v: &[f32]) -> Result<()> {
        let mut base = t.sample(f);
        if let Some(a) = axis {
            ensure(v.len() == 1, "axis expression must return a scalar")?;
            base[a.index()] = v[0];
        } else {
            ensure(
                v.len() == 3,
                "vector expression must return three components",
            )?;
            base.copy_from_slice(v);
        }
        *t = Track::constant(base);
        Ok(())
    }
    match target {
        ExpressionTarget::Property {
            object,
            property,
            axis,
        } => {
            if *object == 0 {
                let c = &mut project.camera;
                match property {
                    Property::Position => vec(&mut c.position, *axis, frame, result)?,
                    Property::Target => vec(&mut c.target, *axis, frame, result)?,
                    Property::Roll => scalar(&mut c.roll, result)?,
                    Property::Fov => scalar(&mut c.fov, result)?,
                    Property::Radius => scalar(&mut c.radius, result)?,
                    Property::Azimuth => scalar(&mut c.azimuth, result)?,
                    Property::Elevation => scalar(&mut c.elevation, result)?,
                    _ => unreachable!(),
                }
                c.validate(project.frames)?;
            } else {
                let l = project.layer_mut(*object)?;
                let f = l.local_frame(frame);
                let t = &mut l.transform;
                match property {
                    Property::Position => vec(&mut t.position, *axis, f, result)?,
                    Property::Rotation => vec(&mut t.rotation, *axis, f, result)?,
                    Property::Scale => vec(&mut t.scale, *axis, f, result)?,
                    Property::Opacity => {
                        ensure(result.len() == 1, "opacity expression must return a scalar")?;
                        scalar(&mut t.opacity, &[(result[0] / 100.0).clamp(0.0, 1.0)])?;
                    }
                    _ => unreachable!(),
                }
                t.validate_local()?;
            }
        }
        ExpressionTarget::Effect {
            object,
            effect,
            param,
        } => {
            let p = project
                .layer_mut(*object)?
                .effects
                .iter_mut()
                .find(|e| e.id == *effect)
                .unwrap()
                .params
                .get_mut(param)
                .unwrap();
            ensure(
                result.len() == p.kind.dimensions(),
                "effect expression has incorrect dimensions",
            )?;
            ensure(
                result.iter().all(|v| *v >= p.min && *v <= p.max),
                "effect expression exceeds parameter range",
            )?;
            let mut value = [0.0; 4];
            value[..result.len()].copy_from_slice(result);
            p.track = Track::constant(value);
        }
    }
    Ok(())
}
impl Project {
    /// Expression-free frames keep the existing allocation-free animation path.
    pub fn evaluated_at(&self, frame: f64) -> Result<Cow<'_, Project>> {
        if !self.expressions.iter().any(|e| active(self, e)) {
            return Ok(Cow::Borrowed(self));
        }
        self.evaluated_at_cancellable(frame, Arc::new(AtomicBool::new(false)))
    }
    pub fn evaluated_at_cancellable(
        &self,
        frame: f64,
        cancel: Arc<AtomicBool>,
    ) -> Result<Cow<'_, Project>> {
        if !self.expressions.iter().any(|e| active(self, e)) {
            return Ok(Cow::Borrowed(self));
        }
        ensure(
            frame.is_finite() && frame >= 0.0 && frame < self.frames as f64,
            "invalid expression frame",
        )?;
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut evaluated = self.clone();
        for e in &self.expressions {
            if active(self, e) {
                let result = evaluate(self, e, frame, deadline, cancel.clone())?;
                write(&mut evaluated, &e.target, frame, &result)
                    .map_err(|err| error(&e.target, frame, err))?;
            }
        }
        Ok(Cow::Owned(evaluated))
    }
    pub fn expression_values(&self, frame: f64) -> Result<Vec<ExpressionValue>> {
        let evaluated = self.evaluated_at(frame)?;
        self.expressions
            .iter()
            .filter(|e| active(self, e))
            .map(|e| {
                Ok(ExpressionValue {
                    target: e.target.clone(),
                    value: raw(&evaluated, &e.target)?.sample(if e.target.object() == 0 {
                        frame
                    } else {
                        evaluated
                            .layers
                            .iter()
                            .find(|l| l.id == e.target.object())
                            .unwrap()
                            .local_frame(frame)
                    }),
                })
            })
            .collect()
    }
}
pub(crate) fn set(project: &mut Project, expression: PropertyExpression, frame: u32) -> Result<()> {
    ensure(
        frame < project.frames,
        "expression edit frame outside composition",
    )?;
    if expression.target.object() != 0 {
        ensure(
            !project.layer_mut(expression.target.object())?.locked,
            "object is locked",
        )?;
    }
    raw(project, &expression.target)?;
    if expression.enabled {
        runtime::compile(&expression.source)
            .map_err(|err| error(&expression.target, frame as f64, err))?;
        let result = evaluate(
            project,
            &expression,
            frame as f64,
            Instant::now() + Duration::from_millis(100),
            Arc::new(AtomicBool::new(false)),
        )?;
        let mut check = project.clone();
        write(&mut check, &expression.target, frame as f64, &result)
            .map_err(|err| error(&expression.target, frame as f64, err))?;
    }
    project.version = 3;
    if let Some(e) = project
        .expressions
        .iter_mut()
        .find(|e| e.target == expression.target)
    {
        *e = expression;
    } else {
        project.expressions.push(expression);
    }
    Ok(())
}
pub(crate) fn copy_layer(project: &mut Project, from: u64, to: u64) {
    let copies: Vec<_> = project
        .expressions
        .iter()
        .filter(|e| e.target.object() == from)
        .map(|e| {
            let mut e = e.clone();
            e.target.set_object(to);
            e
        })
        .collect();
    project.expressions.extend(copies);
}
