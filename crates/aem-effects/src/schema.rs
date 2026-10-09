use crate::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SDK_VERSION: u32 = 6;
pub const MAX_PARAMS: usize = 32;
pub const MAX_PASSES: usize = 8;
pub const MAX_EFFECTS_PER_LAYER: usize = 16;
pub const SCRATCH_BUDGET_FLOOR: u64 = 64 * 1024 * 1024;
/// Compatibility name for SDK callers; the host's actual device policy can be higher.
pub const SCRATCH_BUDGET: u64 = SCRATCH_BUDGET_FLOOR;

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
    /// Package, persisted tracks, and sampled render plans use the same value rules.
    pub fn valid_value(self, value: &[f32; 4], min: f32, max: f32) -> bool {
        value.iter().all(|v| v.is_finite())
            && value[..self.dimensions()]
                .iter()
                .all(|v| *v >= min && *v <= max)
            && (!self.discrete() || value[0].fract() == 0.0)
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
    /// Geometry of this effect's incoming image, in full-resolution layer pixels.
    InputSize {
        component: usize,
    },
    InputOrigin {
        component: usize,
    },
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
    Min {
        a: Box<Self>,
        b: Box<Self>,
    },
    Divide {
        a: Box<Self>,
        b: Box<Self>,
    },
    Hypot {
        a: Box<Self>,
        b: Box<Self>,
    },
    Sqrt {
        value: Box<Self>,
    },
    Sin {
        value: Box<Self>,
    },
    Select {
        condition: Box<Self>,
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
    pub fn requires_sdk4(&self) -> bool {
        match self {
            Self::Min { .. }
            | Self::Divide { .. }
            | Self::Hypot { .. }
            | Self::Sqrt { .. }
            | Self::Sin { .. }
            | Self::Select { .. } => true,
            Self::Add { a, b } | Self::Multiply { a, b } | Self::Max { a, b } => {
                a.requires_sdk4() || b.requires_sdk4()
            }
            Self::Abs { value } | Self::Ceil { value } => value.requires_sdk4(),
            _ => false,
        }
    }
    pub fn evaluate(&self, params: &BTreeMap<String, [f32; 4]>) -> Result<f32> {
        self.evaluate_with(
            |id, component| params.get(id)?.get(component).copied(),
            [0., 0., 1., 1.],
        )
    }
    pub fn uses_geometry(&self) -> bool {
        match self {
            Self::InputSize { .. } | Self::InputOrigin { .. } => true,
            Self::Add { a, b }
            | Self::Multiply { a, b }
            | Self::Max { a, b }
            | Self::Min { a, b }
            | Self::Divide { a, b }
            | Self::Hypot { a, b } => a.uses_geometry() || b.uses_geometry(),
            Self::Abs { value }
            | Self::Ceil { value }
            | Self::Sqrt { value }
            | Self::Sin { value } => value.uses_geometry(),
            Self::Select { condition, a, b } => {
                condition.uses_geometry() || a.uses_geometry() || b.uses_geometry()
            }
            _ => false,
        }
    }
    /// Shared by package validation and the wgpu/GLES execution-plan builder.
    pub fn evaluate_with(
        &self,
        lookup: impl Fn(&str, usize) -> Option<f32>,
        input: [f32; 4],
    ) -> Result<f32> {
        self.eval(&lookup, input, 0)
    }
    fn eval(
        &self,
        lookup: &impl Fn(&str, usize) -> Option<f32>,
        input: [f32; 4],
        depth: usize,
    ) -> Result<f32> {
        ensure(depth <= 16, "bounds expression is too deep")?;
        let v = match self {
            Self::InputSize { component } | Self::InputOrigin { component } => {
                ensure(*component < 2, "invalid bounds geometry component")?;
                input[*component
                    + if matches!(self, Self::InputSize { .. }) {
                        2
                    } else {
                        0
                    }]
            }
            Self::Constant { value } => *value,
            Self::Parameter { id, component } => lookup(id, *component)
                .ok_or_else(|| crate::Error::Invalid("unknown bounds parameter".into()))?,
            Self::Add { a, b } => {
                a.eval(lookup, input, depth + 1)? + b.eval(lookup, input, depth + 1)?
            }
            Self::Multiply { a, b } => {
                a.eval(lookup, input, depth + 1)? * b.eval(lookup, input, depth + 1)?
            }
            Self::Max { a, b } => {
                a.eval(lookup, input, depth + 1)?
                    .max(b.eval(lookup, input, depth + 1)?)
            }
            Self::Min { a, b } => {
                a.eval(lookup, input, depth + 1)?
                    .min(b.eval(lookup, input, depth + 1)?)
            }
            Self::Divide { a, b } => {
                let denominator = b.eval(lookup, input, depth + 1)?;
                ensure(denominator != 0., "division by zero in bounds expression")?;
                a.eval(lookup, input, depth + 1)? / denominator
            }
            Self::Hypot { a, b } => {
                a.eval(lookup, input, depth + 1)?
                    .hypot(b.eval(lookup, input, depth + 1)?)
            }
            Self::Sqrt { value } => {
                let value = value.eval(lookup, input, depth + 1)?;
                ensure(value >= 0., "negative square root in bounds expression")?;
                value.sqrt()
            }
            Self::Sin { value } => value.eval(lookup, input, depth + 1)?.sin(),
            Self::Select { condition, a, b } => {
                if condition.eval(lookup, input, depth + 1)? > 0. {
                    a.eval(lookup, input, depth + 1)?
                } else {
                    b.eval(lookup, input, depth + 1)?
                }
            }
            Self::Abs { value } => value.eval(lookup, input, depth + 1)?.abs(),
            Self::Ceil { value } => value.eval(lookup, input, depth + 1)?.ceil(),
        };
        ensure(
            v.is_finite() && v.abs() <= 32768.0,
            "bounds exceed supported numeric range",
        )?;
        Ok(v)
    }
}
/// An absolute rectangle in layer space. It replaces symmetric padding when present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputBounds {
    pub x: BoundsExpr,
    pub y: BoundsExpr,
    pub width: BoundsExpr,
    pub height: BoundsExpr,
}
impl OutputBounds {
    pub fn evaluate_with(
        &self,
        lookup: impl Fn(&str, usize) -> Option<f32>,
        input: [f32; 4],
    ) -> Result<[f32; 4]> {
        let rect = [
            self.x.evaluate_with(&lookup, input)?,
            self.y.evaluate_with(&lookup, input)?,
            self.width.evaluate_with(&lookup, input)?,
            self.height.evaluate_with(&lookup, input)?,
        ];
        ensure(
            rect[2] >= 1.0 && rect[3] >= 1.0,
            "effect output dimensions must be at least one pixel",
        )?;
        Ok(rect)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bounds: Option<OutputBounds>,
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
    #[serde(default)]
    pub renderer: crate::RendererKind,
    #[serde(default)]
    pub blend: crate::SpriteBlend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<crate::EditorDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_editor: Option<crate::NativeEditorDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<crate::SceneSettings>,
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
            self.format_version == 1 && (1..=SDK_VERSION).contains(&self.sdk_version),
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
            !self.effects.is_empty() && self.effects.len() <= if self.sdk_version >= 6 {128}else{64},
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
            if e.required_capabilities.iter().any(|c| c == "image_input") {
                ensure(self.sdk_version >= 6 && e.renderer == crate::RendererKind::Image
                    && e.params.len() <= 30 && e.resources.len() == 1,
                    "image inputs require SDK 6, an image effect, a resource texture and two reserved parameter slots")?;
            }
            if let Some(editor) = &e.editor {
                ensure(self.sdk_version >= 2, "plugin editors require SDK 2")?;
                editor.validate()?;
            }
            if let Some(editor) = &e.native_editor {
                ensure(self.sdk_version >= 5 && e.editor.is_none() && e.required_capabilities.iter().any(|c|c=="native_plugin_editor"), "native editors require SDK 5, native_plugin_editor and an exclusive presentation")?;
                editor.validate(&e.params,e.renderer)?;
            }
            if let Some(scene) = &e.scene {
                scene.validate()?;
            }
            if e.renderer == crate::RendererKind::Image {
                ensure(
                    e.scene.is_none() && e.blend == crate::SpriteBlend::Alpha,
                    "image effects cannot declare scene state or sprite blending",
                )?;
            }
            if e.renderer != crate::RendererKind::Image {
                ensure(self.sdk_version >= 2 && e.passes.len() == 1 && e.working_space == WorkingSpace::Linear && e.alpha_mode == AlphaMode::Premultiplied, "scene generators require SDK 2, one sprite shader and linear premultiplied output")?;
                ensure(e.scene.is_some(), "scene generator settings missing")?;
                if e.renderer == crate::RendererKind::ParticleEmitter {
                    ensure(self.sdk_version >= 5
                        && e.scene.as_ref().and_then(|s| s.particle_space) == Some(crate::ParticleSpace::WorldBirth)
                        && e.required_capabilities.iter().any(|c| c == "particle_birth_history"),
                        "world-birth emitters require SDK 5 and particle_birth_history")?;
                } else {
                    ensure(e.scene.as_ref().and_then(|s| s.particle_space).is_none(), "legacy generators cannot declare particle space")?;
                }
                let required: &[&str] = if matches!(e.renderer, crate::RendererKind::Particles | crate::RendererKind::ParticleEmitter) {
                    &[
                        "rate",
                        "lifetime",
                        "speed",
                        "spread",
                        "gravity",
                        "extent",
                        "shape",
                        "size",
                        "end_size",
                        "color",
                        "end_color",
                        "fade",
                        "prewarm",
                    ]
                } else {
                    &[
                        "position",
                        "intensity",
                        "scale",
                        "attenuation",
                        "reference_distance",
                        "occlusion_radius",
                    ]
                };
                for id in required {
                    ensure(
                        e.params.iter().any(|p| p.id == *id),
                        format!("generator parameter {id} missing"),
                    )?;
                }
                crate::scene::validate_generator_contract(e.renderer, &e.params)?;
            }
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
                    p.kind.valid_value(&p.default, p.min, p.max),
                    "parameter default exceeds range",
                )?;
                ensure(
                    p.kind != ParamKind::Bool || (p.min == 0.0 && p.max == 1.0),
                    "boolean range must be 0..1",
                )?;
                ensure(
                    p.kind != ParamKind::Enum
                        || (p.min.fract() == 0.0
                            && p.max.fract() == 0.0
                            && p.max - p.min + 1.0 == p.options.len() as f32),
                    "enum range must match its contiguous integer options",
                )?;
                ensure(
                    self.sdk_version < 2
                        || p.kind != ParamKind::Color
                        || (p.min >= 0.0 && p.max <= 1.0),
                    "SDK 2 colors require normalized 0..1 bounds",
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
            if e.padding.uses_geometry() || e.output_bounds.is_some() {
                ensure(self.sdk_version >= 3, "geometry bounds require SDK 3")?;
            }
            if e.padding.requires_sdk4()
                || e.output_bounds.as_ref().is_some_and(|r| {
                    [&r.x, &r.y, &r.width, &r.height]
                        .iter()
                        .any(|expr| expr.requires_sdk4())
                })
            {
                ensure(
                    self.sdk_version >= 4,
                    "spatial bounds arithmetic requires SDK 4",
                )?;
            }
            if let Some(rect) = &e.output_bounds {
                ensure(
                    e.renderer == crate::RendererKind::Image,
                    "rectangle bounds require an image effect",
                )?;
                ensure(
                    e.padding == BoundsExpr::default(),
                    "output_bounds and padding are mutually exclusive",
                )?;
                rect.evaluate_with(
                    |id, component| defaults.get(id)?.get(component).copied(),
                    [0., 0., 1., 1.],
                )?;
            }
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
                        "rect_bounds",
                        "spatial_bounds",
                        "color_profile",
                        "scene_projection",
                        "sprite_instances",
                        "alpha_occlusion",
                        "plugin_editor",
                        "particle_birth_history",
                        "native_plugin_editor",
                        "image_input",
                    ]
                    .contains(&cap.as_str()),
                    format!("unsupported host capability: {cap}"),
                )?;
            }
        }
        Ok(())
    }
}
