use crate::{
    ensure, Asset, CameraMode, Content, Ease, Easing, Error, Layer, Project, Result, Track,
};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    Position,
    Rotation,
    Scale,
    Opacity,
    Target,
    Roll,
    Fov,
    Radius,
    Azimuth,
    Elevation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Effect {
        object: u64,
        action: crate::EffectAction,
    },
    SeparateDimensions {
        object: u64,
        property: Property,
    },
    SetComponent {
        object: u64,
        property: Property,
        axis: crate::Axis,
        frame: u32,
        value: f32,
    },
    MoveLayerClip {
        object: u64,
        in_frame: u32,
    },
    TrimLayerClip {
        object: u64,
        in_frame: u32,
        out_frame: u32,
    },
    SplitLayerClip {
        object: u64,
        frame: u32,
    },
    Curve {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        easing: Easing,
    },
    CreateCamera,
    Remove {
        object: u64,
        frame: u32,
    },
    Parent {
        object: u64,
        parent: Option<u64>,
        frame: u32,
    },
    CopyKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        from: u32,
        to: u32,
    },
    RegisterAsset {
        asset: Asset,
    },
    Content {
        object: u64,
        content: Content,
        size: [f32; 2],
    },
    SetVector {
        object: u64,
        property: Property,
        frame: u32,
        value: [f32; 3],
    },
    SetScalar {
        object: u64,
        property: Property,
        frame: u32,
        value: f32,
    },
    Animate {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        enabled: bool,
    },
    MoveKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        from: u32,
        to: u32,
    },
    DeleteKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
    },
    Ease {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        ease: Ease,
    },
    Add {
        layer: Layer,
    },
    Delete {
        object: u64,
    },
    Duplicate {
        object: u64,
    },
    Rename {
        object: u64,
        name: String,
    },
    Reorder {
        object: u64,
        index: usize,
    },
    Flags {
        object: u64,
        visible: bool,
        locked: bool,
    },
    Anchor {
        object: u64,
        anchor: [f32; 2],
    },
    CameraMode {
        mode: CameraMode,
    },
    Dolly {
        frame: u32,
        amount: f32,
    },
    Pan {
        frame: u32,
        x: f32,
        y: f32,
    },
}

/// Composition routing is explicit even while the engine has one composition.
/// Omitting it always means comp-main, never an editor's current selection.
pub fn parse_commands(text: &str) -> Result<Vec<Command>> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let requests = match value {
        serde_json::Value::Array(values) => values,
        value => vec![value],
    };
    requests
        .into_iter()
        .map(|mut value| {
            let object = value
                .as_object_mut()
                .ok_or_else(|| Error::Invalid("command must be an object".into()))?;
            if let Some(composition) = object.remove("composition") {
                ensure(
                    composition.as_str() == Some("comp-main"),
                    "composition does not exist; this engine supports comp-main only",
                )?;
            }
            Ok(serde_json::from_value(value)?)
        })
        .collect()
}

enum Channel<'a> {
    Scalar(&'a mut Track<f32>),
    Vector(&'a mut Track<[f32; 3]>),
}
impl Channel<'_> {
    fn curve(self, frame: i32, easing: Easing) -> Result<()> {
        match self {
            Self::Scalar(t) => t.set_curve(frame, easing),
            Self::Vector(t) => t.set_curve(frame, easing),
        }
    }
    fn copy_key(self, from: i32, to: i32) -> Result<()> {
        match self {
            Self::Scalar(t) => t.copy_key(from, to),
            Self::Vector(t) => t.copy_key(from, to),
        }
    }
    fn animate(self, frame: i32, enabled: bool) -> Result<()> {
        match self {
            Self::Scalar(t) => t.set_animated(frame, enabled),
            Self::Vector(t) => t.set_animated(frame, enabled),
        }
    }
    fn move_key(self, from: i32, to: i32) -> Result<()> {
        match self {
            Self::Scalar(t) => t.move_key(from, to),
            Self::Vector(t) => t.move_key(from, to),
        }
    }
    fn delete_key(self, frame: i32) -> Result<()> {
        match self {
            Self::Scalar(t) => t.delete_key(frame),
            Self::Vector(t) => t.delete_key(frame),
        }
    }
    fn ease(self, frame: i32, ease: Ease) -> Result<()> {
        match self {
            Self::Scalar(t) => t.set_ease(frame, ease),
            Self::Vector(t) => t.set_ease(frame, ease),
        }
    }
}

fn channel(project: &mut Project, object: u64, property: Property) -> Result<Channel<'_>> {
    if object == 0 {
        let c = &mut project.camera;
        ensure(c.created, "camera does not exist")?;
        return Ok(match property {
            Property::Position => {
                ensure(
                    c.mode == CameraMode::Position,
                    "position is driven by the orbit path",
                )?;
                Channel::Vector(&mut c.position)
            }
            Property::Target => Channel::Vector(&mut c.target),
            Property::Roll => Channel::Scalar(&mut c.roll),
            Property::Fov => Channel::Scalar(&mut c.fov),
            Property::Radius => Channel::Scalar(&mut c.radius),
            Property::Azimuth => Channel::Scalar(&mut c.azimuth),
            Property::Elevation => Channel::Scalar(&mut c.elevation),
            _ => {
                return Err(Error::Invalid(
                    "property is not available on a camera".into(),
                ))
            }
        });
    }
    let layer = project.layer_mut(object)?;
    if layer.locked {
        return Err(Error::Locked(object));
    }
    Ok(match property {
        Property::Position => Channel::Vector(&mut layer.transform.position),
        Property::Rotation => Channel::Vector(&mut layer.transform.rotation),
        Property::Scale => Channel::Vector(&mut layer.transform.scale),
        Property::Opacity => Channel::Scalar(&mut layer.transform.opacity),
        _ => {
            return Err(Error::Invalid(
                "property is not available on a layer".into(),
            ))
        }
    })
}
fn axis_channel(
    project: &mut Project,
    object: u64,
    property: Property,
    axis: Option<crate::Axis>,
) -> Result<Channel<'_>> {
    let c = channel(project, object, property)?;
    match (c, axis) {
        (Channel::Vector(t), Some(axis)) => Ok(Channel::Scalar(t.axis_mut(axis)?)),
        (Channel::Scalar(_), Some(_)) => {
            Err(Error::Invalid("scalar property has no XYZ axis".into()))
        }
        (c, None) => Ok(c),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditResult {
    SplitLayerClip { left_object: u64, right_object: u64 },
}

fn editable_clip(project: &mut Project, object: u64) -> Result<&mut Layer> {
    ensure(object != 0, "camera has no editable clip")?;
    let layer = project.layer_mut(object)?;
    if layer.locked {
        return Err(Error::Locked(object));
    }
    Ok(layer)
}

fn apply_to(project: &mut Project, command: Command) -> Result<Option<EditResult>> {
    let valid_frame = |frame| ensure(frame < project.frames, "edit frame outside the composition");
    let mut result = None;
    match command {
        Command::Effect { object, action } => {
            let frames = project.frames;
            crate::effects::apply(project.layer_mut(object)?, action, frames)?;
        }
        Command::SeparateDimensions { object, property } => {
            match channel(project, object, property)? {
                Channel::Vector(t) => t.separate()?,
                _ => {
                    return Err(Error::Invalid(
                        "only vector properties can separate dimensions".into(),
                    ))
                }
            }
        }
        Command::SetComponent {
            object,
            property,
            axis,
            frame,
            value,
        } => {
            let frame = project.edit_frame(object, frame)?;
            match axis_channel(project, object, property, Some(axis))? {
                Channel::Scalar(t) => t.set_at(frame, value)?,
                _ => unreachable!(),
            }
        }
        Command::MoveLayerClip { object, in_frame } => {
            let frames = project.frames;
            let layer = editable_clip(project, object)?;
            let old = layer.clip(frames);
            let delta = i64::from(in_frame) - i64::from(old.in_frame);
            let out_frame = u32::try_from(i64::from(old.out_frame) + delta)
                .map_err(|_| Error::Invalid("clip interval overflow".into()))?;
            let offset_frame = i32::try_from(i64::from(old.offset_frame) + delta)
                .map_err(|_| Error::Invalid("clip offset overflow".into()))?;
            let clip = crate::LayerTimeline {
                in_frame,
                out_frame,
                offset_frame,
            };
            clip.validate(frames)?;
            if clip != old {
                layer.timeline = Some(clip);
            }
        }
        Command::TrimLayerClip {
            object,
            in_frame,
            out_frame,
        } => {
            let frames = project.frames;
            let layer = editable_clip(project, object)?;
            let clip = crate::LayerTimeline {
                in_frame,
                out_frame,
                ..layer.clip(frames)
            };
            clip.validate(frames)?;
            if clip != layer.clip(frames) {
                layer.timeline = Some(clip);
            }
        }
        Command::SplitLayerClip { object, frame } => {
            ensure(
                project.layers.len() < crate::MAX_LAYERS,
                "layer limit exceeded",
            )?;
            let frames = project.frames;
            let mut right = editable_clip(project, object)?.clone();
            let clip = right.clip(frames);
            ensure(
                clip.in_frame < frame && frame < clip.out_frame,
                "split must be inside the layer clip",
            )?;
            right.id = project
                .layers
                .iter()
                .map(|l| l.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("layer ID space exhausted".into()))?;
            right.timeline = Some(crate::LayerTimeline {
                in_frame: frame,
                ..clip
            });
            let right_object = right.id;
            let index = project
                .layers
                .iter()
                .position(|l| l.id == object)
                .ok_or(Error::Missing(object))?;
            project.layers[index].timeline = Some(crate::LayerTimeline {
                out_frame: frame,
                ..clip
            });
            project.layers.insert(index + 1, right);
            result = Some(EditResult::SplitLayerClip {
                left_object: object,
                right_object,
            });
        }
        Command::Curve {
            object,
            property,
            axis,
            frame,
            easing,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            axis_channel(project, object, property, axis)?.curve(frame, easing)?;
        }
        Command::CopyKey {
            object,
            property,
            axis,
            from,
            to,
        } => {
            valid_frame(from)?;
            valid_frame(to)?;
            let from = project.edit_frame(object, from)?;
            let to = project.edit_frame(object, to)?;
            axis_channel(project, object, property, axis)?.copy_key(from, to)?;
        }
        Command::SetVector {
            object,
            property,
            frame,
            value,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            match channel(project, object, property)? {
                Channel::Vector(t) => t.set_at(frame, value)?,
                _ => return Err(Error::Invalid("expected a scalar property".into())),
            }
        }
        Command::SetScalar {
            object,
            property,
            frame,
            value,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            match channel(project, object, property)? {
                Channel::Scalar(t) => t.set_at(frame, value)?,
                _ => return Err(Error::Invalid("expected a vector property".into())),
            }
        }
        Command::Animate {
            object,
            property,
            axis,
            frame,
            enabled,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            axis_channel(project, object, property, axis)?.animate(frame, enabled)?;
        }
        Command::MoveKey {
            object,
            property,
            axis,
            from,
            to,
        } => {
            valid_frame(from)?;
            valid_frame(to)?;
            let from = project.edit_frame(object, from)?;
            let to = project.edit_frame(object, to)?;
            axis_channel(project, object, property, axis)?.move_key(from, to)?;
        }
        Command::DeleteKey {
            object,
            property,
            axis,
            frame,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            axis_channel(project, object, property, axis)?.delete_key(frame)?;
        }
        Command::Ease {
            object,
            property,
            axis,
            frame,
            ease,
        } => {
            valid_frame(frame)?;
            let frame = project.edit_frame(object, frame)?;
            axis_channel(project, object, property, axis)?.ease(frame, ease)?;
        }
        Command::RegisterAsset { asset } => project.assets.push(asset),
        Command::Content {
            object,
            content,
            size,
        } => {
            let layer = project.layer_mut(object)?;
            if layer.locked {
                return Err(Error::Locked(object));
            }
            layer.content = content;
            layer.size = size;
        }
        Command::Add { mut layer } => {
            layer
                .timeline
                .get_or_insert_with(|| crate::LayerTimeline::full(project.frames));
            project.layers.push(layer);
        }
        Command::CreateCamera => {
            ensure(!project.camera.created, "camera already exists")?;
            project.camera = crate::Camera::new(project.width, project.height);
        }
        Command::Remove { object, frame } => {
            valid_frame(frame)?;
            if object == 0 {
                ensure(project.camera.created, "camera does not exist")?;
            }
            if object != 0 {
                ensure(!project.layer_mut(object)?.locked, "object is locked")?;
            }
            let children: Vec<_> = project
                .layers
                .iter()
                .filter(|l| l.parent.as_ref().and_then(|p| p.object) == Some(object))
                .map(|l| l.id)
                .collect();
            for child in children {
                crate::hierarchy::reparent(project, child, None, frame)?;
            }
            if project.camera.parent.as_ref().and_then(|p| p.object) == Some(object) {
                crate::hierarchy::reparent(project, 0, None, frame)?;
            }
            if object == 0 {
                project.camera = crate::Camera::new(project.width, project.height);
                project.camera.created = false;
            } else {
                project.layers.retain(|l| l.id != object);
            }
        }
        Command::Parent {
            object,
            parent,
            frame,
        } => {
            valid_frame(frame)?;
            crate::hierarchy::reparent(project, object, parent, frame)?;
        }
        Command::Delete { object } => {
            if project.layer_mut(object)?.locked {
                return Err(Error::Locked(object));
            }
            project.layers.retain(|l| l.id != object);
        }
        Command::Duplicate { object } => {
            let mut layer = project.layer_mut(object)?.clone();
            layer.id = project
                .layers
                .iter()
                .map(|l| l.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("layer ID space exhausted".into()))?;
            layer.name = format!("{} 副本", layer.name);
            layer.locked = false;
            project.layers.push(layer);
        }
        Command::Rename { object, name } => project.layer_mut(object)?.name = name,
        Command::Flags {
            object,
            visible,
            locked,
        } => {
            let layer = project.layer_mut(object)?;
            layer.visible = visible;
            layer.locked = locked;
        }
        Command::Reorder { object, index } => {
            ensure(index < project.layers.len(), "invalid layer destination")?;
            let from = project
                .layers
                .iter()
                .position(|l| l.id == object)
                .ok_or(Error::Missing(object))?;
            let layer = project.layers.remove(from);
            project.layers.insert(index, layer);
        }
        Command::CameraMode { mode } => {
            ensure(project.camera.created, "camera does not exist")?;
            project.camera.convert_mode(mode, project.frames)?;
        }
        Command::Dolly { frame, amount } => {
            valid_frame(frame)?;
            ensure(project.camera.created, "camera does not exist")?;
            ensure(amount.is_finite(), "invalid dolly movement")?;
            if project.camera.mode == CameraMode::Orbit {
                let radius = project.camera.radius.sample(f64::from(frame));
                project
                    .camera
                    .radius
                    .set_at(frame, (radius - amount).max(1.0))?;
            } else {
                let eye = Vec3::from_array(project.camera.position.sample(f64::from(frame)));
                let target = Vec3::from_array(project.camera.target.sample(f64::from(frame)));
                let distance = eye.distance(target);
                ensure(distance >= 1.0, "camera is too close to its target")?;
                let step = amount.min(distance - 1.0);
                project
                    .camera
                    .position
                    .set_at(frame, (eye + (target - eye).normalize() * step).to_array())?;
            }
        }
        Command::Pan { frame, x, y } => {
            valid_frame(frame)?;
            ensure(project.camera.created, "camera does not exist")?;
            ensure(x.is_finite() && y.is_finite(), "invalid camera pan")?;
            let pose = project
                .camera
                .pose(f64::from(frame), project.width, project.height);
            let forward = (pose.target - pose.eye).normalize();
            let basis_up = if forward.dot(Vec3::Y).abs() > 0.999 {
                Vec3::Z
            } else {
                Vec3::Y
            };
            let rolled_up = Quat::from_axis_angle(
                forward,
                project.camera.roll.sample(f64::from(frame)).to_radians(),
            ) * basis_up;
            let right = forward.cross(rolled_up).normalize();
            let up = right.cross(forward).normalize();
            let delta_world = right * x + up * y;
            let delta = [delta_world.x, -delta_world.y, -delta_world.z];
            let pos = project.camera.position.sample(f64::from(frame));
            let target = project.camera.target.sample(f64::from(frame));
            let sum = |v: [f32; 3]| std::array::from_fn(|i| v[i] + delta[i]);
            if project.camera.mode == CameraMode::Position {
                project.camera.position.set_at(frame, sum(pos))?;
            }
            project.camera.target.set_at(frame, sum(target))?;
        }
        Command::Anchor { object, anchor } => {
            ensure(
                anchor
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= 1000.0),
                "invalid anchor",
            )?;
            let old = project.layer_mut(object)?.clone();
            if old.locked {
                return Err(Error::Locked(object));
            }
            let local_delta = Vec3::new(
                (anchor[0] - old.transform.anchor[0]) * old.size[0],
                (old.transform.anchor[1] - anchor[1]) * old.size[1],
                0.0,
            );
            let compensate = |frame: f64| {
                let r = old.transform.rotation.sample(frame);
                let rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    r[0].to_radians(),
                    -r[1].to_radians(),
                    -r[2].to_radians(),
                );
                let delta = rotation
                    * (Vec3::from_array(old.transform.scale.sample(frame)) / 100.0 * local_delta);
                let pos = old.transform.position.sample(frame);
                [pos[0] + delta.x, pos[1] - delta.y, pos[2] - delta.z]
            };
            let animated = old.transform.position.is_animated()
                || old.transform.rotation.is_animated()
                || old.transform.scale.is_animated();
            let mut position = Track::constant(compensate(0.0));
            if animated {
                let first = old.clip(project.frames).edit_frame(0)?;
                let last = old.clip(project.frames).edit_frame(project.frames - 1)?;
                let key_frames = old
                    .transform
                    .position
                    .key_frames()
                    .chain(old.transform.rotation.key_frames())
                    .chain(old.transform.scale.key_frames());
                let (first, last) =
                    key_frames.fold((first, last), |(a, b), f| (a.min(f), b.max(f)));
                ensure(
                    i64::from(last) - i64::from(first) < i64::from(crate::MAX_FRAMES),
                    "anchor compensation exceeds key budget",
                )?;
                for frame in first..=last {
                    position.upsert(frame, compensate(f64::from(frame)), Ease::Linear)?;
                }
            }
            if old.transform.position.axes.is_some() {
                position.separate()?;
            }
            let layer = project.layer_mut(object)?;
            layer.transform.position = position;
            layer.transform.anchor = anchor;
        }
    }
    project.rebuild_plugin_dependencies();
    project.validate()?;
    Ok(result)
}

struct History {
    project: Project,
    bytes: usize,
}
pub struct Engine {
    project: Project,
    revision: u64,
    undo: VecDeque<History>,
    redo: VecDeque<History>,
    gesture: Option<Project>,
    history_budget: usize,
}
impl Engine {
    pub fn new(project: Project) -> Result<Self> {
        let project = project.migrate()?;
        Ok(Self {
            project,
            revision: 0,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            gesture: None,
            history_budget: 32 * 1024 * 1024,
        })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn snapshot(&self) -> Project {
        self.project.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() && self.gesture.is_none()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty() && self.gesture.is_none()
    }
    pub fn begin_gesture(&mut self) -> Result<()> {
        if self.gesture.is_some() {
            return Err(Error::Gesture);
        }
        self.gesture = Some(self.project.clone());
        Ok(())
    }
    pub fn end_gesture(&mut self, commit: bool) -> Result<()> {
        let before = self.gesture.take().ok_or(Error::Gesture)?;
        if before != self.project {
            if commit {
                self.push_undo(before);
                self.redo.clear();
            } else {
                self.project = before;
                self.revision = self.revision.wrapping_add(1);
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, command: Command) -> Result<()> {
        self.apply_batch(vec![command]).map(|_| ())
    }
    /// All edits either commit together or restore the pre-edit state.
    pub fn apply_batch(&mut self, commands: Vec<Command>) -> Result<Vec<EditResult>> {
        let before = self.project.clone();
        let mut results = Vec::new();
        for command in commands {
            match apply_to(&mut self.project, command) {
                Ok(Some(result)) => results.push(result),
                Ok(None) => {}
                Err(error) => {
                    self.project = before;
                    return Err(error);
                }
            }
        }
        if before != self.project {
            if self.gesture.is_none() {
                self.push_undo(before);
                self.redo.clear();
            }
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(results)
    }
    pub fn undo(&mut self) -> Result<bool> {
        if self.gesture.is_some() {
            return Err(Error::Gesture);
        }
        let Some(entry) = self.undo.pop_back() else {
            return Ok(false);
        };
        let previous = std::mem::replace(&mut self.project, entry.project);
        self.redo.push_back(History {
            bytes: previous.estimated_bytes(),
            project: previous,
        });
        self.revision = self.revision.wrapping_add(1);
        self.trim();
        Ok(true)
    }
    pub fn redo(&mut self) -> Result<bool> {
        if self.gesture.is_some() {
            return Err(Error::Gesture);
        }
        let Some(entry) = self.redo.pop_back() else {
            return Ok(false);
        };
        let previous = std::mem::replace(&mut self.project, entry.project);
        self.push_undo(previous);
        self.revision = self.revision.wrapping_add(1);
        self.trim();
        Ok(true)
    }
    fn push_undo(&mut self, project: Project) {
        self.undo.push_back(History {
            bytes: project.estimated_bytes(),
            project,
        });
        self.trim();
    }
    fn trim(&mut self) {
        while self.undo.len() + self.redo.len() > 128
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .map(|h| h.bytes)
                .sum::<usize>()
                > self.history_budget
        {
            if self.undo.pop_front().is_none() {
                self.redo.pop_front();
            }
        }
    }
}
