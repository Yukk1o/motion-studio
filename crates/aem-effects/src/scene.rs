//! SDK 2 scene generators and independently hosted plugin editors.
use crate::{ensure, validate_path, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_SPRITES: usize = 65_536;
pub const MAX_PARTICLES: usize = 20_000;
pub const EDITOR_PROTOCOL: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererKind {
    #[default]
    Image,
    Particles,
    /// SDK 5: birth-time world poses, independently moving particles.
    ParticleEmitter,
    LensFlare,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpriteBlend {
    #[default]
    Alpha,
    Additive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorDefinition {
    pub id: String,
    pub protocol: u32,
    /// Local HTML entry point; a frontend mounts this in an isolated WebView.
    pub entry: String,
    pub files: Vec<String>,
    pub title: String,
}
impl EditorDefinition {
    pub fn validate(&self) -> Result<()> {
        ensure(
            crate::valid_id(&self.id) && self.protocol == EDITOR_PROTOCOL,
            "unsupported editor protocol or ID",
        )?;
        ensure(
            self.title.len() <= 256 && (1..=32).contains(&self.files.len()),
            "invalid editor metadata",
        )?;
        ensure(
            self.entry.ends_with(".html") && self.files.contains(&self.entry),
            "editor entry must reference a packaged HTML file",
        )?;
        let mut names = BTreeSet::new();
        for path in &self.files {
            validate_path(path)?;
            ensure(
                path.starts_with("ui/") && names.insert(path),
                "invalid or duplicate editor resource",
            )?;
            ensure(
                editor_mime(path).is_some(),
                "unsupported editor resource type",
            )?;
        }
        Ok(())
    }
}
pub fn editor_mime(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()? {
        "html" => Some("text/html"),
        "js" => Some("text/javascript"),
        "css" => Some("text/css"),
        "json" => Some("application/json"),
        "png" => Some("image/png"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LensShape {
    Glow,
    Halo,
    Ghost,
    Streak,
    Star,
}
impl LensShape {
    pub fn code(self) -> f32 {
        match self {
            Self::Glow => 0.,
            Self::Halo => 1.,
            Self::Ghost => 2.,
            Self::Streak => 3.,
            Self::Star => 4.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensElement {
    pub id: u32,
    pub shape: LensShape,
    pub enabled: bool,
    /// Ghost position along the light-to-image-centre line; zero is the source.
    pub offset: f32,
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub intensity: f32,
    pub rays: u32,
    pub chromatic: f32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneSettings {
    /// A shared project image; particles preserve its aspect ratio and alpha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_asset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub particle_space: Option<ParticleSpace>,
    /// Follow a layer/null's world-space pivot instead of the position parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_layer: Option<u64>,
    #[serde(default)]
    pub occlusion: bool,
    #[serde(default)]
    pub elements: Vec<LensElement>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleSpace {
    WorldBirth,
}
impl SceneSettings {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.source_layer != Some(0) && self.sprite_asset != Some(0) && self.elements.len() <= 64,
            "invalid scene source or too many lens elements",
        )?;
        let mut ids = BTreeSet::new();
        for e in &self.elements {
            ensure(
                e.id != 0 && ids.insert(e.id),
                "invalid or duplicate lens element ID",
            )?;
            ensure(
                e.offset.is_finite()
                    && e.offset.abs() <= 8.
                    && e.size
                        .iter()
                        .all(|v| v.is_finite() && *v > 0. && *v <= 8192.),
                "invalid lens element geometry",
            )?;
            ensure(
                e.color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                    && e.intensity.is_finite()
                    && (0. ..=32.).contains(&e.intensity)
                    && (2..=32).contains(&e.rays)
                    && e.chromatic.is_finite()
                    && (0. ..=0.5).contains(&e.chromatic),
                "invalid lens element appearance",
            )?;
        }
        Ok(())
    }
}

pub(crate) fn validate_generator_contract(
    kind: RendererKind,
    params: &[crate::ParamDefinition],
) -> Result<()> {
    use crate::ParamKind::*;
    let contract: &[(&str, crate::ParamKind, f32, f32, bool)] = if kind == RendererKind::Particles {
        &[
            ("rate", Float, 0., 10000., false),
            ("lifetime", Float, 0.001, 120., false),
            ("speed", Float, -10000., 10000., false),
            ("spread", Float, 0., 10000., false),
            ("gravity", Vec3, -10000., 10000., false),
            ("extent", Vec3, 0., 20000., false),
            ("shape", Enum, 0., 2., false),
            ("size", Float, 0., 4096., true),
            ("end_size", Float, 0., 4096., true),
            ("color", Color, 0., 1., true),
            ("end_color", Color, 0., 1., true),
            ("fade", Float, 0., 0.5, true),
            ("prewarm", Bool, 0., 1., false),
        ]
    } else if kind == RendererKind::ParticleEmitter {
        &[
            ("rate", Float, 0., 10000., false),
            ("lifetime", Float, 0.001, 120., false),
            ("position", Vec3, -100000., 100000., true),
            ("direction", Vec3, -1., 1., true),
            ("speed", Float, -10000., 10000., true),
            ("spread", Float, 0., 10000., true),
            ("inherit_velocity", Float, 0., 4., true),
            ("gravity", Vec3, -10000., 10000., false),
            ("wind", Vec3, -10000., 10000., false),
            ("drag", Float, 0., 100., false),
            ("extent", Vec3, 0., 20000., true),
            ("shape", Enum, 0., 2., false),
            ("size", Float, 0., 4096., true),
            ("end_size", Float, 0., 4096., true),
            ("color", Color, 0., 1., true),
            ("end_color", Color, 0., 1., true),
            ("fade", Float, 0., 0.5, true),
            ("prewarm", Bool, 0., 1., false),
        ]
    } else {
        &[
            ("position", Vec3, -100000., 100000., true),
            ("intensity", Float, 0., 32., true),
            ("scale", Float, 0., 1000., true),
            ("attenuation", Bool, 0., 1., false),
            ("reference_distance", Float, 1., 100000., true),
            ("occlusion_radius", Float, 0., 128., true),
        ]
    };
    for &(id, ty, min, max, animatable) in contract {
        let p = params
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::Error::Invalid(format!("generator parameter {id} missing")))?;
        ensure(
            p.kind == ty
                && p.min >= min
                && p.max <= max
                && (!p.animatable || animatable)
                && p.implemented,
            format!(
                "generator parameter {id} has an incompatible type, range or animation contract"
            ),
        )?;
        if ty == Enum {
            ensure(
                p.options.len() == 3 && p.min == 0. && p.max == 2.,
                "emitter shapes must be point, box, sphere indexed 0..2",
            )?;
        }
    }
    Ok(())
}
