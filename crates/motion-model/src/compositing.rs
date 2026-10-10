//! Layer transfer and track-matte references, independent of source masks.
use crate::{ensure, Content, Error, Layer, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
}
impl BlendMode {
    pub fn code(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Add => 1,
            Self::Multiply => 2,
            Self::Screen => 3,
            Self::Overlay => 4,
            Self::Darken => 5,
            Self::Lighten => 6,
            Self::Difference => 7,
            Self::Exclusion => 8,
            Self::Subtract => 9,
            Self::Divide => 10,
            Self::ColorDodge => 11,
            Self::ColorBurn => 12,
            Self::HardLight => 13,
            Self::SoftLight => 14,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendSpace {
    #[default]
    Linear,
    Srgb,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerBlend {
    #[serde(default)]
    pub mode: BlendMode,
    #[serde(default)]
    pub space: BlendSpace,
}
pub fn default_blend(value: &LayerBlend) -> bool {
    *value == LayerBlend::default()
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatteMode {
    #[default]
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}
impl MatteMode {
    pub fn inverted(self) -> bool {
        matches!(self, Self::AlphaInverted | Self::LumaInverted)
    }
    pub fn luma(self) -> bool {
        matches!(self, Self::Luma | Self::LumaInverted)
    }
    pub fn code(self) -> u32 {
        match self {
            Self::Alpha => 1,
            Self::AlphaInverted => 2,
            Self::Luma => 3,
            Self::LumaInverted => 4,
        }
    }
}
fn yes() -> bool {
    true
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackMatte {
    pub source: u64,
    #[serde(default)]
    pub mode: MatteMode,
    #[serde(default = "yes")]
    pub hide_source: bool,
}
pub const MAX_MATTE_DEPTH: usize = 8;
pub fn visual(layer: &Layer) -> bool {
    !matches!(
        layer.content,
        Content::Null | Content::Audio { .. } | Content::Adjustment
    )
}
/// References are local to one composition. Visibility does not break a matte.
pub fn validate(layers: &[Layer]) -> Result<()> {
    let by_id: HashMap<_, _> = layers.iter().map(|l| (l.id, l)).collect();
    for l in layers {
        ensure(
            visual(l) || l.track_matte.is_none() && l.blend == LayerBlend::default(),
            "compositing requires an ordinary visual layer",
        )?;
        let mut seen = HashSet::new();
        let mut current = l;
        let mut depth = 0;
        while let Some(m) = current.track_matte {
            ensure(
                m.source != current.id,
                "track matte cannot reference itself",
            )?;
            ensure(seen.insert(current.id), "track matte dependency cycle")?;
            current = by_id.get(&m.source).copied().ok_or_else(|| {
                Error::Invalid(format!(
                    "layer {} track matte source {} missing",
                    l.id, m.source
                ))
            })?;
            ensure(visual(current), "track matte source must be a visual layer")?;
            depth += 1;
            ensure(
                depth <= MAX_MATTE_DEPTH,
                "track matte dependency depth exceeds eight",
            )?;
        }
    }
    Ok(())
}
pub fn matte_sources(layers: &[Layer]) -> HashSet<u64> {
    layers
        .iter()
        .filter_map(|l| l.track_matte.map(|m| m.source))
        .collect()
}
pub fn hidden_sources(layers: &[Layer]) -> HashSet<u64> {
    layers
        .iter()
        .filter_map(|l| l.track_matte.filter(|m| m.hide_source).map(|m| m.source))
        .collect()
}
/// Premultiplied source-over with a separable transfer in the requested space.
/// CPU reference is also used by analytical verification, never frame conversion.
pub fn transfer(mode: BlendMode, backdrop: f32, source: f32) -> f32 {
    let b = backdrop.clamp(0., 1.);
    let s = source.clamp(0., 1.);
    match mode {
        BlendMode::Normal => s,
        BlendMode::Add => (b + s).min(1.),
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::Overlay => {
            if b <= 0.5 {
                2. * b * s
            } else {
                1. - 2. * (1. - b) * (1. - s)
            }
        }
        BlendMode::HardLight => {
            if s <= 0.5 {
                2. * b * s
            } else {
                1. - 2. * (1. - b) * (1. - s)
            }
        }
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2. * b * s,
        BlendMode::Subtract => (b - s).max(0.),
        BlendMode::Divide => {
            if s == 0. {
                1.
            } else {
                (b / s).min(1.)
            }
        }
        BlendMode::ColorDodge => {
            if b == 0. {
                0.
            } else if s == 1. {
                1.
            } else {
                (b / (1. - s)).min(1.)
            }
        }
        BlendMode::ColorBurn => {
            if b == 1. {
                1.
            } else if s == 0. {
                0.
            } else {
                1. - ((1. - b) / s).min(1.)
            }
        }
        BlendMode::SoftLight => {
            if s <= 0.5 {
                b - (1. - 2. * s) * b * (1. - b)
            } else {
                let d = if b <= 0.25 {
                    ((16. * b - 12.) * b + 4.) * b
                } else {
                    b.sqrt()
                };
                b + (2. * s - 1.) * (d - b)
            }
        }
    }
}
