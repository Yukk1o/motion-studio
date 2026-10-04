use crate::{ensure, Result};
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
}
impl Tween for f32 {
    fn mix(self, other: Self, t: f32) -> Self {
        // Avoid overflow in (other - self) for opposite finite endpoints.
        (f64::from(self) * f64::from(1.0 - t) + f64::from(other) * f64::from(t)) as f32
    }
    fn finite(self) -> bool {
        self.is_finite()
    }
}
impl Tween for [f32; 3] {
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
    pub frame: u32,
    pub value: T,
    #[serde(default)]
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track<T> {
    pub value: T,
    pub keys: Vec<Keyframe<T>>,
}

impl<T: Tween> Track<T> {
    pub fn constant(value: T) -> Self {
        Self {
            value,
            keys: Vec::new(),
        }
    }
    /// Sampling copies only scalars/arrays and allocates no memory.
    pub fn sample(&self, frame: f64) -> T {
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
        let t = ((frame - f64::from(a.frame)) / f64::from(b.frame - a.frame)) as f32;
        a.value.mix(b.value, a.ease.map(t))
    }
    pub fn validate(&self, frame_count: u32) -> Result<()> {
        ensure(
            self.value.finite(),
            "track contains a non-finite static value",
        )?;
        ensure(self.keys.len() <= frame_count as usize, "too many keys")?;
        let mut previous = None;
        for key in &self.keys {
            ensure(key.frame < frame_count, "keyframe outside the composition")?;
            ensure(key.value.finite(), "keyframe contains a non-finite value")?;
            ensure(
                previous.is_none_or(|p| p < key.frame),
                "keyframes must be unique and sorted",
            )?;
            previous = Some(key.frame);
        }
        Ok(())
    }
    pub fn upsert(&mut self, frame: u32, value: T, ease: Ease) -> Result<()> {
        ensure(value.finite(), "non-finite property value")?;
        let key = Keyframe { frame, value, ease };
        match self.keys.binary_search_by_key(&frame, |k| k.frame) {
            Ok(index) => self.keys[index] = key,
            Err(index) => self.keys.insert(index, key),
        }
        Ok(())
    }
    pub fn set_at(&mut self, frame: u32, value: T) -> Result<()> {
        ensure(value.finite(), "non-finite property value")?;
        if self.keys.is_empty() {
            self.value = value;
            return Ok(());
        }
        let ease = self
            .keys
            .iter()
            .find(|k| k.frame == frame)
            .map_or(Ease::Linear, |k| k.ease);
        self.upsert(frame, value, ease)
    }
    pub fn set_animated(&mut self, frame: u32, enabled: bool) -> Result<()> {
        let value = self.sample(f64::from(frame));
        if enabled && self.keys.is_empty() {
            self.upsert(frame, value, Ease::Linear)?;
        } else if !enabled {
            self.keys.clear();
            self.value = value;
        }
        Ok(())
    }
    pub fn move_key(&mut self, from: u32, to: u32) -> Result<()> {
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
        let key = self.keys.remove(index);
        self.upsert(to, key.value, key.ease)
    }
    pub fn delete_key(&mut self, frame: u32) -> Result<()> {
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
    pub fn copy_key(&mut self, from: u32, to: u32) -> Result<()> {
        let key = self
            .keys
            .iter()
            .find(|k| k.frame == from)
            .ok_or_else(|| crate::Error::Invalid("keyframe not found".into()))?
            .clone();
        self.upsert(to, key.value, key.ease)
    }
    pub fn set_ease(&mut self, frame: u32, ease: Ease) -> Result<()> {
        let index = self
            .keys
            .binary_search_by_key(&frame, |k| k.frame)
            .map_err(|_| crate::Error::Invalid("keyframe not found".into()))?;
        self.keys[index].ease = ease;
        Ok(())
    }
}
