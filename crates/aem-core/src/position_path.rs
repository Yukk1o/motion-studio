//! Read-only editing geometry in the current parent/camera coordinate system.
use crate::{ensure, Error, ExpressionTarget, Project, Property, Result, Scene, Track, Tween};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PositionTarget {
    Property {
        object: u64,
        property: Property,
    },
    Effect {
        object: u64,
        effect: u64,
        param: String,
    },
}
impl PositionTarget {
    pub fn object(&self) -> u64 {
        match self {
            Self::Property { object, .. } | Self::Effect { object, .. } => *object,
        }
    }
}

fn point<T: Tween + Serialize>(value: T) -> [f32; 3] {
    // Position arrays have two, three or four stored components; the fourth is
    // the SDK parameter padding, never a homogeneous coordinate.
    value
        .spatial_components()
        .expect("validated position vector")
}

fn geometry<T: Tween + Serialize>(
    track: &Track<T>,
    sampled: T,
    offset: i32,
    frame: f64,
    frames: u32,
    matrix: Mat4,
    editable: bool,
) -> Value {
    let visible: Vec<_> = track
        .keys
        .iter()
        .filter(|k| {
            let f = i64::from(k.frame) + i64::from(offset);
            f >= 0 && f < i64::from(frames)
        })
        .collect();
    let near = visible.partition_point(|k| f64::from(k.frame) + f64::from(offset) <= frame);
    let start = near
        .saturating_sub(256)
        .min(visible.len().saturating_sub(512));
    let nodes: Vec<_> = visible
        .iter()
        .skip(start)
        .take(512)
        .map(|k| {
            let index = track
                .keys
                .binary_search_by_key(&k.frame, |v| v.frame)
                .unwrap();
            let incoming = (index > 0).then(|| {
                k.spatial.as_ref().and_then(|s| s.incoming).map_or_else(
                    || point(track.keys[index - 1].value.mix(k.value, 2. / 3.)),
                    |v| point(k.value.add(v)),
                )
            });
            let outgoing = (index + 1 < track.keys.len()).then(|| {
                k.spatial.as_ref().and_then(|s| s.outgoing).map_or_else(
                    || point(k.value.mix(track.keys[index + 1].value, 1. / 3.)),
                    |v| point(k.value.add(v)),
                )
            });
            json!({"frame":i64::from(k.frame)+i64::from(offset),"local_frame":k.frame,
            "value":point(k.value),"incoming":incoming,"outgoing":outgoing,"spatial":k.spatial})
        })
        .collect();
    let mut samples = Vec::new();
    if track.is_animated() {
        let times: Vec<_> = track.key_frames().collect();
        if let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) {
            let a = (f64::from(*first) + f64::from(offset)).max(0.);
            let b = (f64::from(*last) + f64::from(offset)).min(f64::from(frames - 1));
            if b >= a {
                for i in 0..=256 {
                    let at = a + (b - a) * f64::from(i) / 256.;
                    samples.push(
                        json!({"frame":at,"value":point(track.sample(at-f64::from(offset)))}),
                    );
                }
            }
        }
    }
    json!({"version":1,"matrix":matrix.to_cols_array(),"value":point(sampled),
        "keys":nodes,"samples":samples,"editable":editable,"spatial_editable":editable&&track.axes.is_none(),
        "keys_truncated":visible.len()>512,"projection":"current_parent_and_camera","temporal_easing":"independent"})
}

pub fn sample(
    project: &Project,
    scene: &Scene,
    target: &PositionTarget,
    frame: f64,
) -> Result<Value> {
    ensure(
        frame.is_finite() && frame >= 0. && frame < f64::from(project.frames),
        "position path frame outside composition",
    )?;
    let evaluated = scene.sampled_project(project);
    let object = target.object();
    let layer = project.layers.iter().find(|l| l.id == object);
    ensure(
        object == 0 || layer.is_some(),
        "position path layer missing",
    )?;
    let offset = layer.and_then(|l| l.timeline).map_or(0, |t| t.offset_frame);
    let local = frame - f64::from(offset);
    let sign = Mat4::from_scale(Vec3::new(1., -1., -1.));
    let expressed = project.expressions.iter().any(|e| {
        e.enabled
            && match (&e.target, target) {
                (
                    ExpressionTarget::Property {
                        object: a,
                        property: b,
                        ..
                    },
                    PositionTarget::Property { object, property },
                ) => a == object && b == property,
                (
                    ExpressionTarget::Effect {
                        object: a,
                        effect: b,
                        param: c,
                    },
                    PositionTarget::Effect {
                        object,
                        effect,
                        param,
                    },
                ) => a == object && b == effect && c == param,
                _ => false,
            }
    });
    let editable = !expressed && layer.is_none_or(|l| !l.locked);
    let mut result = match target {
        PositionTarget::Property { property, .. } => {
            ensure(
                matches!(property, Property::Position | Property::Target),
                "not a position property",
            )?;
            let (stored, sampled, spatial) = if let Some(l) = layer {
                ensure(
                    *property == Property::Position,
                    "layer has no target property",
                )?;
                let current = evaluated
                    .layers
                    .iter()
                    .find(|v| v.id == object)
                    .ok_or(Error::Missing(object))?;
                (
                    &l.transform.position,
                    current.transform.position.sample(local),
                    l.three_d,
                )
            } else {
                ensure(project.camera.created, "camera missing")?;
                let t = if *property == Property::Position {
                    &project.camera.position
                } else {
                    &project.camera.target
                };
                let sampled = if *property == Property::Position {
                    evaluated.camera.position.sample(frame)
                } else {
                    evaluated.camera.target.sample(frame)
                };
                (t, sampled, true)
            };
            let mut prefix = crate::hierarchy::prefix(evaluated, object, frame)?;
            if !spatial {
                prefix = crate::scene::flat_matrix(prefix);
            }
            let projection = if spatial {
                scene.camera.view_projection
            } else {
                Mat4::orthographic_rh(
                    -(project.width as f32) / 2.,
                    project.width as f32 / 2.,
                    -(project.height as f32) / 2.,
                    project.height as f32 / 2.,
                    -1.,
                    1.,
                )
            };
            let matrix = projection
                * prefix
                * Mat4::from_translation(Vec3::new(
                    -(project.width as f32) / 2.,
                    project.height as f32 / 2.,
                    0.,
                ))
                * sign;
            geometry(
                stored,
                sampled,
                offset,
                frame,
                project.frames,
                matrix,
                editable,
            )
        }
        PositionTarget::Effect { effect, param, .. } => {
            let layer = layer.unwrap();
            let e = layer
                .effects
                .iter()
                .find(|e| e.id == *effect)
                .ok_or_else(|| Error::Invalid("position effect missing".into()))?;
            let p = e
                .params
                .get(param)
                .ok_or_else(|| Error::Invalid("position parameter missing".into()))?;
            ensure(
                p.implemented
                    && matches!(
                        p.kind,
                        aem_effects::ParamKind::Vec2 | aem_effects::ParamKind::Vec3
                    ),
                "not a vector position parameter",
            )?;
            let current = evaluated
                .layers
                .iter()
                .find(|l| l.id == object)
                .unwrap()
                .effects
                .iter()
                .find(|v| v.id == *effect)
                .unwrap();
            let sampled = current.params[param].sample(local);
            let particle =
                e.effect == "particle_emitter" && param == "position" && e.scene.is_some();
            let matrix = if particle {
                let source = e
                    .scene
                    .as_ref()
                    .and_then(|s| s.source_layer)
                    .unwrap_or(object);
                scene.camera.view_projection
                    * scene.world_matrix(source).ok_or(Error::Missing(source))?
                    * sign
            } else {
                let draw = scene
                    .layers
                    .iter()
                    .find(|l| l.id == object)
                    .ok_or_else(|| Error::Invalid("position layer is not active".into()))?;
                draw.view_projection
                    * draw.model
                    * Mat4::from_translation(Vec3::new(
                        -draw.source_size[0] / 2.,
                        draw.source_size[1] / 2.,
                        0.,
                    ))
                    * sign
            };
            geometry(
                &p.track,
                sampled,
                offset,
                frame,
                project.frames,
                matrix,
                editable && p.animatable,
            )
        }
    };
    result["target"] = serde_json::to_value(target).unwrap();
    result["frame"] = json!(frame);
    let limits = match target {
        PositionTarget::Property { .. } => [-10_000_000., 10_000_000.],
        PositionTarget::Effect { effect, param, .. } => {
            let p = &layer
                .unwrap()
                .effects
                .iter()
                .find(|e| e.id == *effect)
                .unwrap()
                .params[param];
            [p.min, p.max]
        }
    };
    result["minimum"] = json!(limits[0]);
    result["maximum"] = json!(limits[1]);
    Ok(result)
}
