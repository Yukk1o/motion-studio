use crate::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SDK_VERSION: u32 = 1;
pub const MAX_PARAMS: usize = 32;
pub const MAX_PASSES: usize = 8;
pub const MAX_EFFECTS_PER_LAYER: usize = 16;
pub const SCRATCH_BUDGET: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Float,
    Vec2,
    Vec3,
    Color,
    Bool,
    Enum,
    Curve,
}
impl ParamKind {
    pub fn dimensions(self) -> usize {
        match self {
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Color => 4,
            _ => 1,
        }
    }
    pub fn discrete(self) -> bool {
        matches!(self, Self::Bool | Self::Enum)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamDefinition {
    pub id: String,
    pub name: String,
    pub kind: ParamKind,
    pub default: [f32; 4],
    pub min: f32,
    pub max: f32,
    pub step: f32,
    #[serde(default)]
    pub units: String,
    #[serde(default)]
    pub animatable: bool,
    #[serde(default)]
    pub center_default: bool,
    #[serde(default)]
    pub relative_default: Option<[f32; 2]>,
    #[serde(default = "yes")]
    pub implemented: bool,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub reference_match_name: String,
}
fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeMode {
    #[default]
    Transparent,
    Clamp,
    Repeat,
    Mirror,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSpace {
    #[default]
    Linear,
    Srgb,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    #[default]
    Premultiplied,
    Straight,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    Verified,
    #[default]
    Approximate,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum BoundsExpr {
    Constant {
        value: f32,
    },
    Parameter {
        id: String,
        #[serde(default)]
        component: usize,
    },
    Add {
        a: Box<Self>,
        b: Box<Self>,
    },
    Multiply {
        a: Box<Self>,
        b: Box<Self>,
    },
    Max {
        a: Box<Self>,
        b: Box<Self>,
    },
    Abs {
        value: Box<Self>,
    },
    Ceil {
        value: Box<Self>,
    },
}
impl Default for BoundsExpr {
    fn default() -> Self {
        Self::Constant { value: 0.0 }
    }
}
impl BoundsExpr {
    pub fn evaluate(&self, params: &BTreeMap<String, [f32; 4]>) -> Result<f32> {
        self.eval(params, 0)
    }
    fn eval(&self, p: &BTreeMap<String, [f32; 4]>, depth: usize) -> Result<f32> {
        ensure(depth <= 16, "bounds expression is too deep")?;
        let v = match self {
            Self::Constant { value } => *value,
            Self::Parameter { id, component } => *p
                .get(id)
                .and_then(|v| v.get(*component))
                .ok_or_else(|| crate::Error::Invalid("unknown bounds parameter".into()))?,
            Self::Add { a, b } => a.eval(p, depth + 1)? + b.eval(p, depth + 1)?,
            Self::Multiply { a, b } => a.eval(p, depth + 1)? * b.eval(p, depth + 1)?,
            Self::Max { a, b } => a.eval(p, depth + 1)?.max(b.eval(p, depth + 1)?),
            Self::Abs { value } => value.eval(p, depth + 1)?.abs(),
            Self::Ceil { value } => value.eval(p, depth + 1)?.ceil(),
        };
        ensure(
            v.is_finite() && v.abs() <= 32768.0,
            "bounds exceed supported numeric range",
        )?;
        Ok(v)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassDefinition {
    pub shader: String,
    pub entry: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDefinition {
    pub id: String,
    pub name: String,
    pub english_name: String,
    pub category: String,
    pub params: Vec<ParamDefinition>,
    pub passes: Vec<PassDefinition>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub padding: BoundsExpr,
    #[serde(default)]
    pub edge_mode: EdgeMode,
    #[serde(default)]
    pub edge_param: Option<String>,
    #[serde(default)]
    pub working_space: WorkingSpace,
    #[serde(default)]
    pub alpha_mode: AlphaMode,
    #[serde(default)]
    pub compatibility: Compatibility,
    #[serde(default)]
    pub compatibility_profile: String,
    #[serde(default)]
    pub reference_match_name: String,
    #[serde(default)]
    pub reference_version: String,
    #[serde(default)]
    pub known_differences: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub format_version: u32,
    pub sdk_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub effects: Vec<EffectDefinition>,
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
pub fn validate_path(path: &str) -> Result<()> {
    ensure(
        path.len() <= 256
            && !path.contains(['\\', ':', '\0'])
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "invalid package path",
    )
}
impl PluginManifest {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.format_version == 1 && self.sdk_version == SDK_VERSION,
            "incompatible effect package/SDK version",
        )?;
        ensure(valid_id(&self.id), "invalid plugin ID")?;
        ensure(
            semver::Version::parse(&self.version).is_ok(),
            "invalid plugin version",
        )?;
        ensure(
            self.name.len() <= 256 && self.author.len() <= 256 && self.license.len() <= 256,
            "plugin metadata too long",
        )?;
        ensure(
            !self.effects.is_empty() && self.effects.len() <= 64,
            "invalid effect count",
        )?;
        let mut ids = BTreeSet::new();
        for e in &self.effects {
            ensure(
                valid_id(&e.id) && ids.insert(&e.id),
                "invalid or duplicate effect ID",
            )?;
            ensure(
                e.params.len() <= MAX_PARAMS
                    && !e.passes.is_empty()
                    && e.passes.len() <= MAX_PASSES,
                "effect exceeds parameter/pass limits",
            )?;
            ensure(
                e.name.len() <= 256 && e.english_name.len() <= 256,
                "effect name too long",
            )?;
            ensure(
                e.resources.len() <= 4,
                "at most four resource textures are supported",
            )?;
            let mut params = BTreeSet::new();
            let mut defaults = BTreeMap::new();
            for p in &e.params {
                ensure(
                    valid_id(&p.id) && params.insert(&p.id),
                    "invalid or duplicate parameter ID",
                )?;
                ensure(
                    p.min.is_finite()
                        && p.max.is_finite()
                        && p.min <= p.max
                        && p.step.is_finite()
                        && p.step > 0.0,
                    "invalid parameter range",
                )?;
                ensure(
                    p.default.iter().all(|v| v.is_finite()),
                    "non-finite parameter default",
                )?;
                ensure(
                    p.default[..p.kind.dimensions()]
                        .iter()
                        .all(|v| *v >= p.min && *v <= p.max),
                    "parameter default exceeds range",
                )?;
                ensure(
                    !matches!(p.kind, ParamKind::Enum)
                        || (!p.options.is_empty() && p.options.len() <= 64),
                    "invalid enum options",
                )?;
                defaults.insert(p.id.clone(), p.default);
            }
            ensure(
                e.params
                    .iter()
                    .filter(|p| p.kind == ParamKind::Curve)
                    .count()
                    <= 1,
                "one five-channel curve object per effect is supported",
            )?;
            ensure(
                e.padding.evaluate(&defaults)? >= 0.0,
                "negative effect padding",
            )?;
            if let Some(id) = &e.edge_param {
                ensure(params.contains(id), "unknown edge mode parameter")?;
            }
            for pass in &e.passes {
                validate_path(&pass.shader)?;
                ensure(
                    pass.shader.starts_with("shaders/")
                        && pass.shader.ends_with(".wgsl")
                        && valid_id(&pass.entry),
                    "invalid shader declaration",
                )?;
            }
            for path in &e.resources {
                validate_path(path)?;
                ensure(
                    path.starts_with("assets/") && path.ends_with(".png"),
                    "effect resources must be PNGs inside assets/",
                )?;
            }
            for cap in &e.required_capabilities {
                ensure(
                    [
                        "single_frame",
                        "multipass",
                        "param_lut",
                        "dynamic_bounds",
                        "color_profile",
                    ]
                    .contains(&cap.as_str()),
                    format!("unsupported host capability: {cap}"),
                )?;
            }
        }
        Ok(())
    }
}
