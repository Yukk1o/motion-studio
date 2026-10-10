//! Layer-local source masks. Coordinates are untransformed pixels, Y down,
//! with the source's top-left at (0, 0); masks precede image effects.
use crate::{
    ensure,
    vector::{VectorContent, VectorPath, VectorSource},
    Easing, Error, Result, Track,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_MASKS: usize = 16;
pub const MAX_MASK_NODES: usize = 2048;
pub const MAX_MASK_DISTANCE: f32 = 32000.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    None,
    #[default]
    Add,
    Subtract,
    Intersect,
    Lighten,
    Darken,
    Difference,
}
impl MaskMode {
    pub fn code(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Add => 1,
            Self::Subtract => 2,
            Self::Intersect => 3,
            Self::Lighten => 4,
            Self::Darken => 5,
            Self::Difference => 6,
        }
    }
    pub fn initial(self) -> f32 {
        if matches!(self, Self::Subtract | Self::Intersect | Self::Darken) {
            1.
        } else {
            0.
        }
    }
}
fn hundred() -> Track<f32> {
    Track::constant(100.)
}
fn zero() -> Track<f32> {
    Track::constant(0.)
}
fn feather() -> Track<[f32; 2]> {
    Track::constant([0.; 2])
}
fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerMask {
    pub id: u64,
    pub name: String,
    pub path: VectorPath,
    #[serde(default)]
    pub mode: MaskMode,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub inverted: bool,
    #[serde(default = "hundred")]
    pub opacity: Track<f32>,
    #[serde(default = "feather")]
    pub feather: Track<[f32; 2]>,
    #[serde(default = "zero")]
    pub expansion: Track<f32>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SampledMask {
    pub id: u64,
    pub nodes: Vec<[f32; 6]>,
    pub mode: MaskMode,
    pub inverted: bool,
    pub opacity: f32,
    pub feather: [f32; 2],
    pub expansion: f32,
}
impl LayerMask {
    pub fn new(id: u64, path: VectorPath) -> Self {
        Self {
            id,
            name: "Mask".into(),
            path,
            mode: MaskMode::Add,
            enabled: true,
            inverted: false,
            opacity: hundred(),
            feather: feather(),
            expansion: zero(),
        }
    }
    pub fn animated(&self) -> bool {
        !self.opacity.keys.is_empty()
            || !self.feather.keys.is_empty()
            || !self.expansion.keys.is_empty()
            || self.path.nodes.iter().any(|n| !n.geometry.keys.is_empty())
    }
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.id != 0 && self.name.len() <= 4096,
            "invalid mask ID or name",
        )?;
        VectorContent {
            trim: None,
            source: VectorSource::Paths {
                paths: vec![self.path.clone()],
            },
            fill: None,
            fill_rule: crate::vector::FillRule::NonZero,
            stroke: None,
        }
        .validate()?;
        self.opacity.validate_local()?;
        self.feather.validate_local()?;
        self.expansion.validate_local()?;
        ensure(
            self.opacity.axes.is_none()
                && self.feather.axes.is_none()
                && self.expansion.axes.is_none(),
            "mask parameter tracks do not support separated axes",
        )?;
        for v in
            std::iter::once(self.opacity.value).chain(self.opacity.keys.iter().map(|k| k.value))
        {
            ensure(
                (0. ..=100.).contains(&v),
                "mask opacity must be 0..100 percent",
            )?;
        }
        for v in std::iter::once(self.feather.value)
            .chain(self.feather.keys.iter().map(|k| k.value))
            .flatten()
        {
            ensure(
                (0. ..=MAX_MASK_DISTANCE).contains(&v),
                "mask feather must be 0..32000 pixels",
            )?;
        }
        self.expansion.validate_bound(MAX_MASK_DISTANCE)?;
        Ok(())
    }
    pub fn sample(&self, frame: f64) -> Result<Option<SampledMask>> {
        if !self.enabled || self.mode == MaskMode::None || !self.path.closed {
            return Ok(None);
        }
        let opacity = self.opacity.sample(frame);
        let feather = self.feather.sample(frame);
        let expansion = self.expansion.sample(frame);
        ensure(
            opacity.is_finite(),
            "nonfinite sampled mask opacity",
        )?;
        ensure(
            feather
                .iter()
                .all(|v|v.is_finite()),
            "nonfinite sampled mask feather",
        )?;
        ensure(
            expansion.is_finite() && expansion.abs() <= MAX_MASK_DISTANCE,
            "sampled mask expansion outside range",
        )?;
        let nodes: Vec<_> = self
            .path
            .nodes
            .iter()
            .map(|n| n.geometry.sample(frame))
            .collect();
        ensure(
            nodes
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 131072.),
            "sampled mask path outside coordinate range",
        )?;
        Ok(Some(SampledMask {
            id: self.id,
            nodes,
            mode: self.mode,
            inverted: self.inverted,
            opacity: opacity.clamp(0.,100.) / 100.,
            feather:feather.map(|v|v.clamp(0.,MAX_MASK_DISTANCE)),
            expansion,
        }))
    }
}
pub fn validate(masks: &[LayerMask]) -> Result<()> {
    ensure(masks.len() <= MAX_MASKS, "at most 16 masks per layer")?;
    ensure(
        masks.iter().map(|m| m.path.nodes.len()).sum::<usize>() <= MAX_MASK_NODES,
        "mask node budget exceeds 2048 per layer",
    )?;
    let mut ids = HashSet::new();
    for mask in masks {
        ensure(ids.insert(mask.id), "duplicate mask ID")?;
        mask.validate()
            .map_err(|e| Error::Invalid(format!("mask {}: {e}", mask.id)))?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskProperty {
    Opacity,
    Feather,
    Expansion,
    Node(u64),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaskAction {
    Add {
        mask: LayerMask,
    },
    Replace {
        mask: LayerMask,
    },
    Remove {
        mask: u64,
    },
    Reorder {
        masks: Vec<u64>,
    },
    Options {
        mask: u64,
        #[serde(default)] mode: Option<MaskMode>,
        #[serde(default)] inverted: Option<bool>,
        #[serde(default)] enabled: Option<bool>,
    },
    Path {
        mask: u64,
        path: VectorPath,
    },
    Set {
        mask: u64,
        property: MaskProperty,
        frame: u32,
        value: Vec<f32>,
        #[serde(default)] animated: Option<bool>,
    },
    Animate { mask:u64, property:MaskProperty, frame:u32, enabled:bool },
    DeleteKey {
        mask: u64,
        property: MaskProperty,
        frame: u32,
    },
    MoveKey {
        mask: u64,
        property: MaskProperty,
        from: u32,
        to: u32,
    },
    CopyKey { mask:u64, property:MaskProperty, from:u32, to:u32 },
    Curve {
        mask: u64,
        property: MaskProperty,
        frame: u32,
        easing: Easing,
    },
}
impl MaskAction {
    pub fn frames(&self) -> Vec<u32> {
        match self {
            Self::Set { frame, .. } | Self::Animate { frame, .. } | Self::DeleteKey { frame, .. } | Self::Curve { frame, .. } => {
                vec![*frame]
            }
            Self::MoveKey { from, to, .. } | Self::CopyKey {from,to,..}=> vec![*from, *to],
            _ => vec![],
        }
    }
}
pub fn edit(masks: &mut Vec<LayerMask>, action: MaskAction, offset: i32) -> Result<()> {
    let id = match &action {
        MaskAction::Add { mask } | MaskAction::Replace { mask } => mask.id,
        MaskAction::Remove { mask }
        | MaskAction::Options { mask, .. }
        | MaskAction::Path { mask, .. }
        | MaskAction::Set { mask, .. }
        | MaskAction::Animate { mask, .. }
        | MaskAction::DeleteKey { mask, .. }
        | MaskAction::MoveKey { mask, .. }
        | MaskAction::CopyKey { mask, .. }
        | MaskAction::Curve { mask, .. } => *mask,
        MaskAction::Reorder { .. } => 0,
    };
    let index = masks.iter().position(|m| m.id == id);
    if !matches!(action, MaskAction::Add { .. } | MaskAction::Reorder { .. }) {
        ensure(index.is_some(), "mask does not exist")?;
    }
    let local = |f: u32| {
        i32::try_from(i64::from(f) - i64::from(offset))
            .map_err(|_| Error::Invalid("mask local frame overflow".into()))
    };
    match action {
        MaskAction::Add { mask } => {
            ensure(index.is_none(), "mask ID already exists")?;
            masks.push(mask);
        }
        MaskAction::Replace { mask } => masks[index.unwrap()] = mask,
        MaskAction::Remove { .. } => {
            masks.remove(index.unwrap());
        }
        MaskAction::Reorder { masks: order } => {
            let ids: HashSet<_> = order.iter().copied().collect();
            ensure(
                order.len() == masks.len()
                    && ids.len() == order.len()
                    && masks.iter().all(|m| ids.contains(&m.id)),
                "mask order must include each existing ID once",
            )?;
            masks.sort_by_key(|m| order.iter().position(|id| *id == m.id).unwrap());
        }
        MaskAction::Options {
            mode,
            inverted,
            enabled,
            ..
        } => {
            let m = &mut masks[index.unwrap()];
            if let Some(mode)=mode {m.mode=mode;}
            if let Some(inverted)=inverted {m.inverted=inverted;}
            if let Some(enabled)=enabled {m.enabled=enabled;}
        }
        MaskAction::Path { path, .. } => masks[index.unwrap()].path = path,
        action => {
            let property = match &action {
                MaskAction::Set { property, .. }
                | MaskAction::Animate { property, .. }
                | MaskAction::DeleteKey { property, .. }
                | MaskAction::MoveKey { property, .. }
                | MaskAction::CopyKey { property, .. }
                | MaskAction::Curve { property, .. } => property,
                _ => unreachable!(),
            };
            macro_rules! update {
                ($track:expr,$len:expr) => {{
                    let t = &mut $track;
                    match &action {
                        MaskAction::Set {
                            frame,
                            value,
                            animated,
                            ..
                        } => {
                            ensure(
                                value.len() == $len,
                                "mask property value dimension mismatch",
                            )?;
                            if let Some(enabled)=animated {t.set_animated(local(*frame)?, *enabled)?;}
                            t.set_at(
                                local(*frame)?,
                                value
                                    .as_slice()
                                    .try_into()
                                    .map_err(|_| Error::Invalid("mask value dimension".into()))?,
                            )?;
                        }
                        MaskAction::DeleteKey { frame, .. } => t.delete_key(local(*frame)?)?,
                        MaskAction::Animate {frame,enabled,..}=>t.set_animated(local(*frame)?,*enabled)?,
                        MaskAction::CopyKey {from,to,..}=>t.copy_key(local(*from)?,local(*to)?)?,
                        MaskAction::MoveKey { from, to, .. } => {
                            t.move_key(local(*from)?, local(*to)?)?
                        }
                        MaskAction::Curve { frame, easing, .. } => {
                            t.set_curve(local(*frame)?, *easing)?
                        }
                        _ => unreachable!(),
                    }
                }};
            }
            // Scalar tracks use the same animation/curve commands as vectors.
            let mask = &mut masks[index.unwrap()];
            match property {
                MaskProperty::Feather => update!(mask.feather, 2),
                MaskProperty::Node(node) => {
                    let n = mask
                        .path
                        .nodes
                        .iter_mut()
                        .find(|n| n.id == *node)
                        .ok_or_else(|| Error::Invalid("mask node missing".into()))?;
                    update!(n.geometry, 6);
                }
                MaskProperty::Opacity | MaskProperty::Expansion => {
                    let t = if matches!(property, MaskProperty::Opacity) {
                        &mut mask.opacity
                    } else {
                        &mut mask.expansion
                    };
                    match &action {
                        MaskAction::Set {
                            frame,
                            value,
                            animated,
                            ..
                        } => {
                            ensure(value.len() == 1, "scalar mask value requires one number")?;
                            if let Some(enabled)=animated {t.set_animated(local(*frame)?, *enabled)?;}
                            t.set_at(local(*frame)?, value[0])?;
                        }
                        MaskAction::DeleteKey { frame, .. } => t.delete_key(local(*frame)?)?,
                        MaskAction::Animate {frame,enabled,..}=>t.set_animated(local(*frame)?,*enabled)?,
                        MaskAction::CopyKey {from,to,..}=>t.copy_key(local(*from)?,local(*to)?)?,
                        MaskAction::MoveKey { from, to, .. } => {
                            t.move_key(local(*from)?, local(*to)?)?
                        }
                        MaskAction::Curve { frame, easing, .. } => {
                            t.set_curve(local(*frame)?, *easing)?
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
    validate(masks)
}
