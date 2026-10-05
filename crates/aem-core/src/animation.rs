use crate::{ensure, Curve, Easing, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    In,
    Out,
    InOut,
    Hold,
}

impl Ease {
    pub fn map(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::In => t * t,
            Self::Out => 1.0 - (1.0 - t) * (1.0 - t),
            Self::InOut => t * t * (3.0 - 2.0 * t),
            Self::Hold => {
                if t < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }
}

pub trait Tween: Copy + PartialEq {
    fn mix(self, other: Self, t: f32) -> Self;
    fn finite(self) -> bool;
    fn bounded(self, bound: f32) -> bool;
    fn components(self) -> Option<[f32; 3]> {
        None
    }
    fn from_components(_: [f32; 3]) -> Option<Self> {
        None
    }
}
impl Tween for f32 {
    fn bounded(self, bound: f32) -> bool {
        self.is_finite() && self.abs() <= bound
    }
    fn mix(self, other: Self, t: f32) -> Self {
        // Avoid overflow in (other - self) for opposite finite endpoints.
        (f64::from(self) * f64::from(1.0 - t) + f64::from(other) * f64::from(t)) as f32
    }
    fn finite(self) -> bool {
        self.is_finite()
    }
}
impl Tween for [f32; 3] {
    fn bounded(self, bound: f32) -> bool {
        self.into_iter().all(|v| v.is_finite() && v.abs() <= bound)
    }
    fn components(self) -> Option<[f32; 3]> {
        Some(self)
    }
    fn from_components(value: [f32; 3]) -> Option<Self> {
        Some(value)
    }
    fn mix(self, other: Self, t: f32) -> Self {
        std::array::from_fn(|i| self[i].mix(other[i], t))
    }
    fn finite(self) -> bool {
        self.into_iter().all(f32::is_finite)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe<T> {
    pub frame: i32,
    pub value: T,
    #[serde(default)]
    pub ease: Ease,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

#[derive(Clone, Debug)]
pub struct Track<T> {
    pub value: T,
    pub keys: Vec<Keyframe<T>>,
    /// Present only after an explicit user edit. These scalar tracks are then
    /// authoritative; legacy vector keys are removed and are not serialized.
    pub axes: Option<Box<AxisTracks>>,
}
impl<T: PartialEq> PartialEq for Track<T> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.axes, &other.axes) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.value == other.value && self.keys == other.keys,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisTracks {
    pub x: Track<f32>,
    pub y: Track<f32>,
    pub z: Track<f32>,
}
impl AxisTracks {
    pub fn get(&self, axis: Axis) -> &Track<f32> {
        match axis {
            Axis::X => &self.x,
            Axis::Y => &self.y,
            Axis::Z => &self.z,
        }
    }
    pub fn get_mut(&mut self, axis: Axis) -> &mut Track<f32> {
        match axis {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
            Axis::Z => &mut self.z,
        }
    }
    pub fn tracks(&self) -> [&Track<f32>; 3] {
        [&self.x, &self.y, &self.z]
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoupledTrack<T> {
    value: T,
    keys: Vec<Keyframe<T>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeparatedTrack {
    axes: Box<AxisTracks>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum TrackRepresentation<T> {
    Coupled(CoupledTrack<T>),
    Separated(SeparatedTrack),
}
impl<T: Tween + Serialize> Serialize for Track<T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(if self.axes.is_some() { 1 } else { 2 }))?;
        if let Some(axes) = &self.axes {
            map.serialize_entry("axes", axes)?;
        } else {
            map.serialize_entry("value", &self.value)?;
            map.serialize_entry("keys", &self.keys)?;
        }
        map.end()
    }
}
impl<'de, T: Tween + Deserialize<'de>> Deserialize<'de> for Track<T> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        match TrackRepresentation::<T>::deserialize(deserializer)? {
            TrackRepresentation::Coupled(t) => Ok(Self {
                value: t.value,
                keys: t.keys,
                axes: None,
            }),
            TrackRepresentation::Separated(t) => {
                let value = T::from_components([t.axes.x.value, t.axes.y.value, t.axes.z.value])
                    .ok_or_else(|| {
                        serde::de::Error::custom("only vector properties can separate dimensions")
                    })?;
                Ok(Self {
                    value,
                    keys: Vec::new(),
                    axes: Some(t.axes),
                })
            }
        }
    }
}

impl<T: Tween> Track<T> {
    pub fn static_value(&self) -> T {
        self.axes.as_ref().map_or(self.value, |a| {
            T::from_components([a.x.value, a.y.value, a.z.value]).unwrap()
        })
    }
    pub fn is_animated(&self) -> bool {
        !self.keys.is_empty()
            || self
                .axes
                .as_ref()
                .is_some_and(|a| a.tracks().iter().any(|t| !t.keys.is_empty()))
    }
    pub fn key_frames(&self) -> impl Iterator<Item = i32> + '_ {
        self.keys.iter().map(|k| k.frame).chain(
            self.axes
                .iter()
                .flat_map(|a| a.tracks())
                .flat_map(|t| t.keys.iter().map(|k| k.frame)),
        )
    }
    pub fn constant(value: T) -> Self {
        Self {
            value,
            keys: Vec::new(),
            axes: None,
        }
    }
    /// Sampling copies only scalars/arrays and allocates no memory.
    pub fn sample(&self, frame: f64) -> T {
        if let Some(a) = &self.axes {
            return T::from_components([a.x.sample(frame), a.y.sample(frame), a.z.sample(frame)])
                .expect("validated vector dimensions");
        }
        let Some(first) = self.keys.first() else {
            return self.value;
        };
        if frame <= f64::from(first.frame) {
            return first.value;
        }
        let upper = self.keys.partition_point(|k| f64::from(k.frame) <= frame);
        if upper >= self.keys.len() {
            return self.keys.last().unwrap().value;
        }
        let a = &self.keys[upper - 1];
        let b = &self.keys[upper];
        let t = ((frame - f64::from(a.frame)) / (f64::from(b.frame) - f64::from(a.frame))) as f32;
        let progress = a.curve.map_or_else(
            || a.ease.map(t),
            |curve| curve.sample(f64::from(t)).progress as f32,
        );
        a.value.mix(b.value, progress)
    }
    pub fn validate(&self, frame_count: u32) -> Result<()> {
        self.validate_range(frame_count as usize, Some(frame_count))
    }
    /// Layer keys use signed local time and survive trimming or moving outside
    /// the composition. Their count remains bounded independently of the clip.
    pub fn validate_local(&self) -> Result<()> {
        self.validate_range(crate::MAX_FRAMES as usize, None)
    }
    fn validate_range(&self, max_keys: usize, frame_count: Option<u32>) -> Result<()> {
        if let Some(axes) = &self.axes {
            ensure(
                T::from_components([0.0; 3]).is_some(),
                "only vector properties can separate dimensions",
            )?;
            ensure(
                self.keys.is_empty(),
                "separated tracks cannot contain vector keys",
            )?;
            for t in axes.tracks() {
                t.validate_range(max_keys, frame_count)?;
            }
            return Ok(());
        }
        ensure(
            self.value.finite(),
            "track contains a non-finite static value",
        )?;
        ensure(self.keys.len() <= max_keys, "too many keys")?;
        let mut previous = None;
        for key in &self.keys {
            if let Some(count) = frame_count {
                ensure(
                    key.frame >= 0 && i64::from(key.frame) < i64::from(count),
                    "keyframe outside the composition",
                )?;
            }
            ensure(key.value.finite(), "keyframe contains a non-finite value")?;
            if let Some(curve) = key.curve {
                curve.validate()?;
            }
            ensure(
                previous.is_none_or(|p| p < key.frame),
                "keyframes must be unique and sorted",
            )?;
            previous = Some(key.frame);
        }
        Ok(())
    }
    pub fn upsert(&mut self, frame: impl TryInto<i32>, value: T, ease: Ease) -> Result<()> {
        let frame = stored_frame(frame)?;
        ensure(value.finite(), "non-finite property value")?;
        if let Some(axes) = &mut self.axes {
            let v = value
                .components()
                .ok_or_else(|| crate::Error::Invalid("expected vector value".into()))?;
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                axes.get_mut(axis).upsert(frame, v[axis.index()], ease)?;
            }
            return Ok(());
        }
        let key = Keyframe {
            frame,
            value,
            ease,
            curve: None,
        };
        match self.keys.binary_search_by_key(&frame, |k| k.frame) {
            Ok(index) => self.keys[index] = key,
            Err(index) => self.keys.insert(index, key),
        }
        Ok(())
    }
    pub fn set_at(&mut self, frame: impl TryInto<i32>, value: T) -> Result<()> {
        let frame = stored_frame(frame)?;
        ensure(value.finite(), "non-finite property value")?;
        if let Some(axes) = &mut self.axes {
            let v = value
                .components()
                .ok_or_else(|| crate::Error::Invalid("expected vector value".into()))?;
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                axes.get_mut(axis).set_at(frame, v[axis.index()])?;
            }
            return Ok(());
        }
        if self.keys.is_empty() {
            self.value = value;
            return Ok(());
        }
        if let Ok(index) = self.keys.binary_search_by_key(&frame, |k| k.frame) {
            self.keys[index].value = value;
            Ok(())
        } else {
            self.upsert(frame, value, Ease::Linear)
        }
    }
    pub fn set_animated(&mut self, frame: impl TryInto<i32>, enabled: bool) -> Result<()> {
        let frame = stored_frame(frame)?;
        if let Some(a) = &self.axes {
            if enabled {
                ensure(
                    a.tracks().iter().all(|t| t.is_animated())
                        || a.tracks().iter().all(|t| !t.is_animated()),
                    "whole animation edit requires matching axis animation states",
                )?;
            }
        }
        if self.edit_axes(|t| t.set_animated(frame, enabled))? {
            return Ok(());
        }
        let value = self.sample(f64::from(frame));
        if enabled && self.keys.is_empty() {
            self.upsert(frame, value, Ease::Linear)?;
        } else if !enabled {
            self.keys.clear();
            self.value = value;
        }
        Ok(())
    }
    pub fn move_key(&mut self, from: i32, to: i32) -> Result<()> {
        self.validate_axis_collision(to)?;
        if self.edit_axes(|t| t.move_key(from, to))? {
            return Ok(());
        }
        if from == to {
            return ensure(
                self.keys.iter().any(|k| k.frame == from),
                "keyframe not found",
            );
        }
        let index = self
            .keys
            .binary_search_by_key(&from, |k| k.frame)
            .map_err(|_| crate::Error::Invalid("keyframe not found".into()))?;
        let mut key = self.keys.remove(index);
        key.frame = to;
        self.insert_key(key);
        Ok(())
    }
    pub fn delete_key(&mut self, frame: i32) -> Result<()> {
        if self.edit_axes(|t| t.delete_key(frame))? {
            return Ok(());
        }
        let index = self
            .keys
            .binary_search_by_key(&frame, |k| k.frame)
            .map_err(|_| crate::Error::Invalid("keyframe not found".into()))?;
        // Removing the last key keeps its sampled value instead of jumping to
        // the obsolete static value from before animation was enabled.
        if self.keys.len() == 1 {
            self.value = self.keys[index].value;
        }
        self.keys.remove(index);
        Ok(())
    }
    pub fn copy_key(&mut self, from: i32, to: i32) -> Result<()> {
        self.validate_axis_collision(to)?;
        if self.edit_axes(|t| t.copy_key(from, to))? {
            return Ok(());
        }
        let mut key = self
            .keys
            .iter()
            .find(|k| k.frame == from)
            .ok_or_else(|| crate::Error::Invalid("keyframe not found".into()))?
            .clone();
        key.frame = to;
        self.insert_key(key);
        Ok(())
    }
    pub fn set_ease(&mut self, frame: i32, ease: Ease) -> Result<()> {
        if self.edit_axes(|t| t.set_ease(frame, ease))? {
            return Ok(());
        }
        let index = self
            .keys
            .binary_search_by_key(&frame, |k| k.frame)
            .map_err(|_| crate::Error::Invalid("keyframe not found".into()))?;
        self.keys[index].ease = ease;
        self.keys[index].curve = None;
        Ok(())
    }
    fn insert_key(&mut self, key: Keyframe<T>) {
        match self.keys.binary_search_by_key(&key.frame, |k| k.frame) {
            Ok(index) => self.keys[index] = key,
            Err(index) => self.keys.insert(index, key),
        }
    }
    pub fn set_curve(&mut self, frame: i32, easing: Easing) -> Result<()> {
        if self.edit_axes(|t| t.set_curve(frame, easing))? {
            return Ok(());
        }
        easing.validate()?;
        let index = self
            .keys
            .binary_search_by_key(&frame, |k| k.frame)
            .map_err(|_| crate::Error::Invalid("keyframe not found".into()))?;
        ensure(
            index + 1 < self.keys.len(),
            "curve needs two adjacent keyframes",
        )?;
        self.keys[index].ease = easing.ease;
        self.keys[index].curve = easing.curve;
        Ok(())
    }
    fn validate_axis_collision(&self, frame: i32) -> Result<()> {
        if let Some(a) = &self.axes {
            let occupied = a
                .tracks()
                .iter()
                .filter(|t| t.keys.iter().any(|k| k.frame == frame))
                .count();
            ensure(
                occupied == 0 || occupied == 3,
                "whole key edit would overwrite only some axes",
            )?;
        }
        Ok(())
    }
    /// Validate/edit a candidate before replacing any axis. No partial whole
    /// operation and no artificial keys to make a missing source key exist.
    fn edit_axes(&mut self, edit: impl Fn(&mut Track<f32>) -> Result<()>) -> Result<bool> {
        let Some(axes) = &self.axes else {
            return Ok(false);
        };
        let mut next = axes.clone();
        edit(&mut next.x)?;
        edit(&mut next.y)?;
        edit(&mut next.z)?;
        self.axes = Some(next);
        Ok(true)
    }
    pub fn separate(&mut self) -> Result<()> {
        if self.axes.is_some() {
            return Ok(());
        }
        let value = self.value.components().ok_or_else(|| {
            crate::Error::Invalid("only vector properties can separate dimensions".into())
        })?;
        let component = |index: usize| Track {
            value: value[index],
            axes: None,
            keys: self
                .keys
                .iter()
                .map(|k| Keyframe {
                    frame: k.frame,
                    value: k.value.components().unwrap()[index],
                    ease: k.ease,
                    curve: k.curve,
                })
                .collect(),
        };
        self.axes = Some(Box::new(AxisTracks {
            x: component(0),
            y: component(1),
            z: component(2),
        }));
        self.keys.clear();
        Ok(())
    }
    pub fn axis_mut(&mut self, axis: Axis) -> Result<&mut Track<f32>> {
        self.axes.as_mut().map(|a| a.get_mut(axis)).ok_or_else(|| {
            crate::Error::Invalid("separate dimensions before editing an axis".into())
        })
    }
    pub fn validate_bound(&self, bound: f32) -> Result<()> {
        if let Some(axes) = &self.axes {
            for t in axes.tracks() {
                t.validate_bound(bound)?;
            }
        } else {
            for v in std::iter::once(self.value).chain(self.keys.iter().map(|k| k.value)) {
                ensure(
                    v.bounded(bound),
                    "transform value exceeds its numeric range",
                )?;
            }
        }
        Ok(())
    }
}
fn stored_frame(frame: impl TryInto<i32>) -> Result<i32> {
    frame
        .try_into()
        .map_err(|_| crate::Error::Invalid("local keyframe time overflow".into()))
}
