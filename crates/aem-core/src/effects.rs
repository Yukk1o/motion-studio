use crate::{ensure, Ease, Easing, Error, Keyframe, Layer, Result, Track};
use aem_effects::{EffectDefinition, ParamKind, MAX_EFFECTS_PER_LAYER, MAX_PARAMS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub type CurveLut = [[f32; 4]; 256];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDependency {
    pub plugin: String,
    pub version: String,
    pub hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveObject {
    pub channels: [Vec<[f32; 2]>; 5],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled_lut: Option<Vec<[f32; 4]>>,
}
impl Default for CurveObject {
    fn default() -> Self {
        Self {
            channels: std::array::from_fn(|_| vec![[0.0, 0.0], [1.0, 1.0]]),
            sampled_lut: None,
        }
    }
}
impl CurveObject {
    pub fn validate(&self) -> Result<()> {
        if let Some(lut) = &self.sampled_lut {
            ensure(
                lut.len() == 256
                    && lut
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "invalid frozen curve LUT",
            )?;
        }
        for c in &self.channels {
            ensure((2..=64).contains(&c.len()), "curve requires 2..64 points")?;
            ensure(
                c.first().unwrap()[0] == 0.0 && c.last().unwrap()[0] == 1.0,
                "curve must cover the unit interval",
            )?;
            for p in c {
                ensure(
                    p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                    "invalid curve point",
                )?;
            }
            ensure(
                c.windows(2).all(|p| p[0][0] < p[1][0]),
                "curve X coordinates must be strictly increasing",
            )?;
        }
        Ok(())
    }
    fn at(&self, channel: usize, v: f32) -> f32 {
        let points = &self.channels[channel];
        let i = points
            .partition_point(|p| p[0] <= v)
            .clamp(1, points.len() - 1);
        let a = points[i - 1];
        let b = points[i];
        a[1] + (b[1] - a[1]) * ((v - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0)
    }
    pub fn lut(&self) -> CurveLut {
        if let Some(lut) = &self.sampled_lut {
            return std::array::from_fn(|i| lut[i]);
        }
        std::array::from_fn(|i| {
            let v = i as f32 / 255.0;
            let master = self.at(0, v);
            [
                self.at(1, master),
                self.at(2, master),
                self.at(3, master),
                self.at(4, v),
            ]
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveTrack {
    pub value: CurveObject,
    pub keys: Vec<Keyframe<CurveObject>>,
}
impl Default for CurveTrack {
    fn default() -> Self {
        Self {
            value: CurveObject::default(),
            keys: Vec::new(),
        }
    }
}
impl CurveTrack {
    pub fn sample(&self, frame: f64) -> CurveLut {
        if self.keys.is_empty() {
            return self.value.lut();
        }
        let upper = self.keys.partition_point(|k| f64::from(k.frame) <= frame);
        if upper == 0 {
            return self.keys[0].value.lut();
        }
        if upper >= self.keys.len() {
            return self.keys.last().unwrap().value.lut();
        }
        let a = &self.keys[upper - 1];
        let b = &self.keys[upper];
        let t = ((frame - f64::from(a.frame)) / (f64::from(b.frame) - f64::from(a.frame))) as f32;
        let t = a
            .curve
            .map_or_else(|| a.ease.map(t), |c| c.sample(f64::from(t)).progress as f32);
        let aa = a.value.lut();
        let bb = b.value.lut();
        std::array::from_fn(|i| std::array::from_fn(|c| aa[i][c] + (bb[i][c] - aa[i][c]) * t))
    }
    fn validate(&self, _frames: u32) -> Result<()> {
        self.value.validate()?;
        ensure(
            self.keys.len() <= crate::MAX_FRAMES as usize,
            "too many curve keys",
        )?;
        let mut last = None;
        for k in &self.keys {
            ensure(last.is_none_or(|v| v < k.frame), "invalid curve key time")?;
            k.value.validate()?;
            if let Some(c) = k.curve {
                c.validate()?;
            }
            last = Some(k.frame);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectParam {
    pub kind: ParamKind,
    pub animatable: bool,
    pub min: f32,
    pub max: f32,
    pub implemented: bool,
    pub default: [f32; 4],
    pub track: Track<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveTrack>,
}
impl EffectParam {
    pub fn sample(&self, frame: f64) -> [f32; 4] {
        if self.kind.discrete() {
            let i = self
                .track
                .keys
                .partition_point(|k| f64::from(k.frame) <= frame);
            return if self.track.keys.is_empty() {
                self.track.value
            } else {
                self.track.keys[i.saturating_sub(1)].value
            };
        }
        let mut value = self.track.sample(frame);
        for v in &mut value[..self.kind.dimensions()] {
            *v = v.clamp(self.min, self.max);
        }
        value
    }
    fn validate(&self, frames: u32) -> Result<()> {
        ensure(
            self.min.is_finite() && self.max.is_finite() && self.min <= self.max,
            "invalid effect parameter range",
        )?;
        self.track.validate_local()?;
        ensure(
            self.animatable || self.track.keys.is_empty(),
            "effect parameter is not animatable",
        )?;
        for value in
            std::iter::once(self.track.value).chain(self.track.keys.iter().map(|k| k.value))
        {
            ensure(
                self.kind.valid_value(&value, self.min, self.max),
                "effect parameter exceeds range or discrete value is not integral",
            )?;
            if self.kind.discrete() {
                ensure(
                    value[0].fract() == 0.0,
                    "discrete effect parameter must be integral",
                )?;
            }
            ensure(
                self.implemented || value == self.default,
                "this AE parameter is not implemented",
            )?;
        }
        if let Some(c) = &self.curve {
            ensure(
                self.kind == ParamKind::Curve,
                "curve data attached to a non-curve parameter",
            )?;
            c.validate(frames)?;
            ensure(
                self.animatable || c.keys.is_empty(),
                "curve parameter is not animatable",
            )?;
        }
        ensure(
            (self.kind == ParamKind::Curve) == self.curve.is_some(),
            "curve parameter must contain curve data",
        )?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectInstance {
    pub id: u64,
    pub plugin: String,
    pub effect: String,
    pub version: String,
    pub hash: String,
    pub enabled: bool,
    pub seed: u32,
    pub params: BTreeMap<String, EffectParam>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<aem_effects::SceneSettings>,
}
impl EffectInstance {
    pub fn new(
        id: u64,
        plugin: &str,
        version: &str,
        hash: &str,
        definition: &EffectDefinition,
        size: [f32; 2],
    ) -> Self {
        let params = definition
            .params
            .iter()
            .map(|p| {
                let mut value = p.default;
                if let Some(v) = p.relative_default {
                    value[0] = v[0] * size[0];
                    value[1] = v[1] * size[1];
                } else if p.center_default {
                    value[0] = size[0] * 0.5;
                    value[1] = size[1] * 0.5;
                }
                (
                    p.id.clone(),
                    EffectParam {
                        kind: p.kind,
                        animatable: p.animatable,
                        min: p.min,
                        max: p.max,
                        implemented: p.implemented,
                        default: value,
                        track: Track::constant(value),
                        curve: if p.kind == ParamKind::Curve {
                            Some(CurveTrack::default())
                        } else {
                            None
                        },
                    },
                )
            })
            .collect();
        Self {
            id,
            plugin: plugin.into(),
            effect: definition.id.clone(),
            version: version.into(),
            hash: hash.into(),
            enabled: true,
            seed: id as u32,
            params,
            scene: definition.scene.clone(),
        }
    }
    pub fn dependency(&self) -> PluginDependency {
        PluginDependency {
            plugin: self.plugin.clone(),
            version: self.version.clone(),
            hash: self.hash.clone(),
        }
    }
    pub fn validate(&self, frames: u32) -> Result<()> {
        ensure(
            self.id != 0
                && aem_effects::valid_id(&self.plugin)
                && aem_effects::valid_id(&self.effect),
            "invalid effect identity",
        )?;
        ensure(
            self.version.len() <= 64
                && self.hash.len() == 64
                && self.hash.bytes().all(|v| v.is_ascii_hexdigit()),
            "invalid effect version/hash",
        )?;
        ensure(
            self.params.len() <= MAX_PARAMS,
            "too many effect parameters",
        )?;
        if let Some(scene) = &self.scene {
            scene
                .validate()
                .map_err(|e| Error::Invalid(e.to_string()))?;
        }
        for (id, p) in &self.params {
            ensure(aem_effects::valid_id(id), "invalid effect parameter ID")?;
            p.validate(frames)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectAction {
    SetScene {
        effect: u64,
        scene: aem_effects::SceneSettings,
    },
    Seed {
        effect: u64,
        seed: u32,
    },
    Insert {
        instance: EffectInstance,
    },
    Remove {
        effect: u64,
    },
    Duplicate {
        effect: u64,
    },
    Move {
        effect: u64,
        index: usize,
    },
    Enable {
        effect: u64,
        enabled: bool,
    },
    Set {
        effect: u64,
        param: String,
        frame: u32,
        value: [f32; 4],
    },
    Animate {
        effect: u64,
        param: String,
        frame: u32,
        enabled: bool,
    },
    DeleteKey {
        effect: u64,
        param: String,
        frame: u32,
    },
    MoveKey {
        effect: u64,
        param: String,
        from: u32,
        to: u32,
    },
    CopyKey {
        effect: u64,
        param: String,
        from: u32,
        to: u32,
    },
    Curve {
        effect: u64,
        param: String,
        frame: u32,
        easing: Easing,
    },
    SetCurveObject {
        effect: u64,
        param: String,
        frame: u32,
        value: CurveObject,
    },
}
fn instance_mut(layer: &mut Layer, id: u64) -> Result<&mut EffectInstance> {
    layer
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::Invalid("effect instance does not exist".into()))
}
fn param_mut<'a>(layer: &'a mut Layer, id: u64, param: &str) -> Result<&'a mut EffectParam> {
    instance_mut(layer, id)?
        .params
        .get_mut(param)
        .ok_or_else(|| Error::Invalid("effect parameter does not exist".into()))
}
pub(crate) fn apply(layer: &mut Layer, action: EffectAction, frames: u32) -> Result<()> {
    ensure(!layer.locked, "object is locked")?;
    ensure(
        !matches!(layer.content, crate::Content::Null),
        "null objects do not have image effects",
    )?;
    let clip = layer.clip(frames);
    let local = |f| {
        ensure(f < frames, "effect edit frame outside composition")?;
        clip.edit_frame(f)
    };
    match action {
        EffectAction::SetScene { effect, scene } => {
            scene
                .validate()
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let instance = instance_mut(layer, effect)?;
            ensure(instance.scene.is_some(), "effect has no scene editor")?;
            instance.scene = Some(scene);
        }
        EffectAction::Seed { effect, seed } => instance_mut(layer, effect)?.seed = seed,
        EffectAction::Insert { instance } => {
            ensure(
                layer.effects.len() < MAX_EFFECTS_PER_LAYER,
                "layer effect limit exceeded",
            )?;
            instance.validate(frames)?;
            ensure(
                !layer.effects.iter().any(|e| e.id == instance.id),
                "duplicate effect instance ID",
            )?;
            layer.effects.push(instance);
        }
        EffectAction::Remove { effect } => {
            instance_mut(layer, effect)?;
            layer.effects.retain(|e| e.id != effect);
        }
        EffectAction::Duplicate { effect } => {
            ensure(
                layer.effects.len() < MAX_EFFECTS_PER_LAYER,
                "layer effect limit exceeded",
            )?;
            let mut copy = instance_mut(layer, effect)?.clone();
            copy.id = layer
                .effects
                .iter()
                .map(|e| e.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("effect ID space exhausted".into()))?;
            layer.effects.push(copy);
        }
        EffectAction::Move { effect, index } => {
            ensure(index < layer.effects.len(), "invalid effect destination")?;
            let from = layer
                .effects
                .iter()
                .position(|e| e.id == effect)
                .ok_or_else(|| Error::Invalid("effect does not exist".into()))?;
            let e = layer.effects.remove(from);
            layer.effects.insert(index, e);
        }
        EffectAction::Enable { effect, enabled } => instance_mut(layer, effect)?.enabled = enabled,
        EffectAction::Set {
            effect,
            param,
            frame,
            value,
        } => {
            let frame = local(frame)?;
            let p = param_mut(layer, effect, &param)?;
            ensure(
                p.kind != ParamKind::Curve,
                "use set_curve_object for curve parameters",
            )?;
            p.track.set_at(frame, value)?;
            if p.kind.discrete() {
                for k in &mut p.track.keys {
                    k.ease = Ease::Hold;
                    k.curve = None;
                }
            }
        }
        EffectAction::Animate {
            effect,
            param,
            frame,
            enabled,
        } => {
            let frame = local(frame)?;
            let p = param_mut(layer, effect, &param)?;
            ensure(p.animatable, "effect parameter is not animatable")?;
            if let Some(c) = &mut p.curve {
                if enabled && c.keys.is_empty() {
                    c.keys.push(Keyframe {
                        frame,
                        value: c.value.clone(),
                        ease: Ease::Linear,
                        curve: None,
                    });
                } else if !enabled {
                    let frozen = c.sample(f64::from(frame));
                    let upper = c.keys.partition_point(|k| k.frame <= frame);
                    if !c.keys.is_empty() {
                        c.value = c.keys[upper.saturating_sub(1)].value.clone();
                    }
                    c.value.sampled_lut = Some(
                        frozen
                            .iter()
                            .map(|pixel| pixel.map(|v| v.clamp(0.0, 1.0)))
                            .collect(),
                    );
                    c.keys.clear();
                }
            } else {
                p.track.set_animated(frame, enabled)?;
                if p.kind.discrete() {
                    for k in &mut p.track.keys {
                        k.ease = Ease::Hold;
                    }
                }
            }
        }
        EffectAction::DeleteKey {
            effect,
            param,
            frame,
        } => {
            let frame = local(frame)?;
            let p = param_mut(layer, effect, &param)?;
            if let Some(c) = &mut p.curve {
                let i = c
                    .keys
                    .iter()
                    .position(|k| k.frame == frame)
                    .ok_or_else(|| Error::Invalid("curve key not found".into()))?;
                if c.keys.len() == 1 {
                    c.value = c.keys[i].value.clone();
                }
                c.keys.remove(i);
            } else {
                p.track.delete_key(frame)?;
            }
        }
        EffectAction::MoveKey {
            effect,
            param,
            from,
            to,
        } => {
            let from = local(from)?;
            let to = local(to)?;
            let p = param_mut(layer, effect, &param)?;
            if let Some(c) = &mut p.curve {
                let i = c
                    .keys
                    .iter()
                    .position(|k| k.frame == from)
                    .ok_or_else(|| Error::Invalid("curve key not found".into()))?;
                let mut key = c.keys.remove(i);
                key.frame = to;
                match c.keys.binary_search_by_key(&to, |k| k.frame) {
                    Ok(i) => c.keys[i] = key,
                    Err(i) => c.keys.insert(i, key),
                }
            } else {
                p.track.move_key(from, to)?;
            }
        }
        EffectAction::CopyKey {
            effect,
            param,
            from,
            to,
        } => {
            let from = local(from)?;
            let to = local(to)?;
            let p = param_mut(layer, effect, &param)?;
            if let Some(c) = &mut p.curve {
                let mut key = c
                    .keys
                    .iter()
                    .find(|k| k.frame == from)
                    .cloned()
                    .ok_or_else(|| Error::Invalid("curve key not found".into()))?;
                key.frame = to;
                match c.keys.binary_search_by_key(&to, |k| k.frame) {
                    Ok(i) => c.keys[i] = key,
                    Err(i) => c.keys.insert(i, key),
                }
            } else {
                p.track.copy_key(from, to)?;
            }
        }
        EffectAction::Curve {
            effect,
            param,
            frame,
            easing,
        } => {
            let frame = local(frame)?;
            let p = param_mut(layer, effect, &param)?;
            ensure(
                !p.kind.discrete(),
                "discrete parameters only support hold interpolation",
            )?;
            if let Some(c) = &mut p.curve {
                easing.validate()?;
                let i = c
                    .keys
                    .iter()
                    .position(|k| k.frame == frame)
                    .ok_or_else(|| Error::Invalid("curve key not found".into()))?;
                ensure(i + 1 < c.keys.len(), "curve requires adjacent keys")?;
                c.keys[i].ease = easing.ease;
                c.keys[i].curve = easing.curve;
            } else {
                p.track.set_curve(frame, easing)?;
            }
        }
        EffectAction::SetCurveObject {
            effect,
            param,
            frame,
            value,
        } => {
            let frame = local(frame)?;
            value.validate()?;
            let p = param_mut(layer, effect, &param)?;
            let c = p
                .curve
                .as_mut()
                .ok_or_else(|| Error::Invalid("not a curve-object parameter".into()))?;
            if c.keys.is_empty() {
                c.value = value;
            } else {
                let key = Keyframe {
                    frame,
                    value,
                    ease: Ease::Linear,
                    curve: None,
                };
                match c.keys.binary_search_by_key(&frame, |k| k.frame) {
                    Ok(i) => c.keys[i] = key,
                    Err(i) => c.keys.insert(i, key),
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct SampledEffect {
    pub layer: u64,
    pub local_frame: f64,
    pub instance: u64,
    pub plugin: String,
    pub effect: String,
    pub version: String,
    pub hash: String,
    pub enabled: bool,
    pub seed: u32,
    pub param_ids: Vec<String>,
    pub values: [[f32; 4]; MAX_PARAMS],
    pub lut: Option<usize>,
    pub scene: Option<aem_effects::SceneSettings>,
    pub particle_history: Option<std::sync::Arc<crate::particle_history::ParticleHistory>>,
    pub particle_history_error: Option<String>,
}
impl SampledEffect {
    pub(crate) fn new(layer: u64, e: &EffectInstance) -> Self {
        Self {
            layer,
            local_frame: 0.0,
            instance: e.id,
            plugin: e.plugin.clone(),
            effect: e.effect.clone(),
            version: e.version.clone(),
            hash: e.hash.clone(),
            enabled: e.enabled,
            seed: e.seed,
            param_ids: e.params.keys().cloned().collect(),
            values: [[0.0; 4]; MAX_PARAMS],
            lut: None,
            scene: e.scene.clone(),
            particle_history: None,
            particle_history_error: None,
        }
    }
    pub(crate) fn matches(&self, layer: u64, e: &EffectInstance) -> bool {
        self.layer == layer
            && self.instance == e.id
            && self.plugin == e.plugin
            && self.effect == e.effect
            && self.version == e.version
            && self.hash == e.hash
            && self
                .param_ids
                .iter()
                .map(String::as_str)
                .eq(e.params.keys().map(String::as_str))
    }
}
pub(crate) fn dependencies(layers: &[Layer]) -> Vec<PluginDependency> {
    layers
        .iter()
        .flat_map(|l| l.effects.iter().map(EffectInstance::dependency))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
