mod runtime;
mod transpile;

use crate::{ensure, Axis, Error, ExpressionTarget, ExpressionValue, Project, Property, PropertyExpression, Result, Track};
use motion_model::expressions::{active, raw};
use serde_json::json;
use std::{
    borrow::Cow,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

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
pub trait ProjectExpressions {
    fn evaluated_at(&self, frame: f64) -> Result<Cow<'_, Project>>;
    fn evaluated_at_cancellable(&self, frame: f64, cancel: Arc<AtomicBool>) -> Result<Cow<'_, Project>>;
    fn expression_values(&self, frame: f64) -> Result<Vec<ExpressionValue>>;
}
impl ProjectExpressions for Project {
    /// Expression-free frames keep the existing allocation-free animation path.
    fn evaluated_at(&self, frame: f64) -> Result<Cow<'_, Project>> {
        if !self.expressions.iter().any(|e| active(self, e)) {
            return Ok(Cow::Borrowed(self));
        }
        self.evaluated_at_cancellable(frame, Arc::new(AtomicBool::new(false)))
    }
    fn evaluated_at_cancellable(
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
    fn expression_values(&self, frame: f64) -> Result<Vec<ExpressionValue>> {
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
    project.version = project.version.max(3);
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

/// The host supplies this evaluator explicitly to the JS-independent renderer.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExpressionEvaluator;
impl motion_model::FrameEvaluator for ExpressionEvaluator {
    fn evaluate<'a>(&self, project: &'a Project, frame: f64) -> Result<Cow<'a, Project>> {
        project.evaluated_at(frame)
    }
}
