//! The same frame plan drives wgpu and the MediaCodec/GLES adapter.
use aem_core::{SampledEffect, Scene};
use aem_effects::{
    shader, AlphaMode, BoundsExpr, EdgeMode, EffectDefinition, EffectPackage, Registry,
    WorkingSpace,
};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

pub const PLAN_MAGIC: u32 = 0x46584d53;
pub const PLAN_VERSION: u32 = 2;
pub const DRAW_WORDS: usize = 32;
pub const PASS_WORDS: usize = 10;
pub fn scratch_bytes(width: u32, height: u32, slots: u32) -> u64 {
    u64::from(width)
        * u64::from(height)
        * 4
        * u64::from(slots.count_ones() + u32::from(slots & 128 != 0))
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct EffectUniform {
    pub size: [f32; 4],
    pub region: [f32; 4],
    pub input_region: [f32; 4],
    pub source_region: [f32; 4],
    pub clock: [f32; 4],
    pub mode: [f32; 4],
    pub output_mode: [f32; 4],
    pub params: [[f32; 4]; 32],
}
#[derive(Clone)]
pub struct EffectProgram {
    pub key: String,
    pub shader: Arc<shader::CompiledShader>,
    pub package: Option<Arc<EffectPackage>>,
    pub resources: Vec<String>,
}
#[derive(Clone, Copy, Debug)]
pub struct EffectPass {
    pub program: u32,
    pub input: i32,
    pub source: i32,
    pub output: u32,
    pub width: u32,
    pub height: u32,
    pub lut: i32,
    pub uniform: EffectUniform,
    pub sprite: bool,
    pub sprite_start: u32,
    pub sprite_count: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct PlannedDraw {
    pub layer: u64,
    pub words: [f32; DRAW_WORDS],
    pub pass_start: usize,
    pub pass_end: usize,
}
#[derive(Default, Debug)]
pub struct EffectFramePlan {
    pub draws: Vec<PlannedDraw>,
    pub passes: Vec<EffectPass>,
    pub width: u32,
    pub height: u32,
    pub slots: u32,
    pub diagnostics: Vec<String>,
    pub sprites: Vec<crate::scene_generator::Sprite>,
    pub generator_stats: crate::scene_generator::GeneratorStats,
}
impl EffectFramePlan {
    pub fn buffer_bytes(&self, scene: &Scene) -> usize {
        64 + self.draws.len() * 128
            + self.passes.len() * 40
            + self.passes.len() * shader::UNIFORM_BYTES
            + scene.curve_luts.len() * 1024
            + self.sprites.len() * 48
    }
    pub fn write(&self, scene: &Scene, out: &mut [u8]) -> Result<usize, String> {
        let size = self.buffer_bytes(scene);
        if out.len() < size {
            return Err(format!("render plan buffer requires {size} bytes"));
        }
        let draw_offset = 64;
        let pass_offset = draw_offset + self.draws.len() * 128;
        let uniform_offset = pass_offset + self.passes.len() * 40;
        let lut_offset = uniform_offset + self.passes.len() * shader::UNIFORM_BYTES;
        let sprite_offset = lut_offset + scene.curve_luts.len() * 1024;
        let header = [
            PLAN_MAGIC,
            PLAN_VERSION,
            self.draws.len() as u32,
            self.passes.len() as u32,
            draw_offset as u32,
            pass_offset as u32,
            uniform_offset as u32,
            size as u32,
            self.width,
            self.height,
            self.slots,
            lut_offset as u32,
            scene.curve_luts.len() as u32,
            sprite_offset as u32,
            self.sprites.len() as u32,
            48,
        ];
        out[..64].copy_from_slice(bytemuck::cast_slice(&header));
        for (i, d) in self.draws.iter().enumerate() {
            out[draw_offset + i * 128..draw_offset + (i + 1) * 128]
                .copy_from_slice(bytemuck::cast_slice(&d.words));
        }
        for (i, p) in self.passes.iter().enumerate() {
            let data = [
                p.program,
                p.input as u32,
                p.source as u32,
                p.output,
                p.width,
                p.height,
                (uniform_offset + i * shader::UNIFORM_BYTES) as u32,
                if p.lut < 0 {
                    u32::MAX
                } else {
                    (lut_offset + p.lut as usize * 1024) as u32
                },
                p.sprite_start,
                p.sprite_count,
            ];
            out[pass_offset + i * 40..pass_offset + (i + 1) * 40]
                .copy_from_slice(bytemuck::cast_slice(&data));
            out[uniform_offset + i * shader::UNIFORM_BYTES
                ..uniform_offset + (i + 1) * shader::UNIFORM_BYTES]
                .copy_from_slice(bytemuck::bytes_of(&p.uniform));
        }
        for (i, lut) in scene.curve_luts.iter().enumerate() {
            for (j, pixel) in lut.iter().enumerate() {
                for c in 0..4 {
                    out[lut_offset + i * 1024 + j * 4 + c] =
                        (pixel[c].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
        out[sprite_offset..size].copy_from_slice(bytemuck::cast_slice(&self.sprites));
        Ok(size)
    }
}
struct Resolved {
    signature: (String, String, String, String),
    definition: EffectDefinition,
    programs: Vec<u32>,
    mapping: Vec<usize>,
    error: Option<String>,
}
pub struct PlanBuilder {
    pub registry: Registry,
    pub device_dimension: u32,
    pub program_errors: std::collections::BTreeMap<u32, String>,
    pub programs: Vec<EffectProgram>,
    resolved: Vec<Option<Resolved>>,
    pub frame: EffectFramePlan,
    pub generator_scratch: crate::scene_generator::GeneratorScratch,
    pub alpha_images: std::collections::HashMap<u64, crate::scene_generator::AlphaImage>,
}
fn utility(code: &str) -> Result<Arc<shader::CompiledShader>, String> {
    shader::compile(code, "main_fx")
        .map(Arc::new)
        .map_err(|e| e.to_string())
}
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn bounds(expr: &BoundsExpr, fx: &SampledEffect, depth: u32) -> Result<f32, String> {
    if depth > 16 {
        return Err("bounds expression exceeds depth budget".into());
    }
    let v = match expr {
        BoundsExpr::Constant { value } => *value,
        BoundsExpr::Parameter { id, component } => {
            let i = fx
                .param_ids
                .iter()
                .position(|p| p == id)
                .ok_or("unknown bounds parameter")?;
            *fx.values[i]
                .get(*component)
                .ok_or("invalid bounds component")?
        }
        BoundsExpr::Add { a, b } => bounds(a, fx, depth + 1)? + bounds(b, fx, depth + 1)?,
        BoundsExpr::Multiply { a, b } => bounds(a, fx, depth + 1)? * bounds(b, fx, depth + 1)?,
        BoundsExpr::Max { a, b } => bounds(a, fx, depth + 1)?.max(bounds(b, fx, depth + 1)?),
        BoundsExpr::Abs { value } => bounds(value, fx, depth + 1)?.abs(),
        BoundsExpr::Ceil { value } => bounds(value, fx, depth + 1)?.ceil(),
    };
    if !v.is_finite() || v.abs() > 32768.0 {
        return Err("invalid effect output bounds".into());
    }
    Ok(v)
}
impl PlanBuilder {
    pub fn set_alpha(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), String> {
        let size = u64::from(width) * u64::from(height);
        let previous = self
            .alpha_images
            .get(&id)
            .map_or(0, |a| a.pixels.len() as u64);
        let total = self
            .alpha_images
            .values()
            .map(|a| a.pixels.len() as u64)
            .sum::<u64>();
        if width == 0
            || height == 0
            || rgba.len() as u64 != size * 4
            || total - previous + size > 32 * 1024 * 1024
        {
            return Err("occlusion alpha cache exceeds 32 MiB or has invalid dimensions".into());
        }
        self.alpha_images.insert(
            id,
            crate::scene_generator::AlphaImage {
                width,
                height,
                pixels: rgba.chunks_exact(4).map(|p| p[3]).collect(),
            },
        );
        Ok(())
    }
    pub fn synchronize_alpha(
        &mut self,
        project: &aem_core::Project,
        root: &std::path::Path,
    ) -> Result<(), String> {
        if !project.layers.iter().any(|l| {
            l.effects
                .iter()
                .any(|e| e.enabled && e.scene.as_ref().is_some_and(|s| s.occlusion))
        }) {
            self.alpha_images.clear();
            return Ok(());
        }
        for asset in &project.assets {
            if !self.alpha_images.contains_key(&asset.id) {
                let image = image::open(root.join(&asset.path))
                    .map_err(|e| e.to_string())?
                    .into_rgba8();
                if image.width() != asset.width || image.height() != asset.height {
                    return Err("occlusion asset metadata mismatch".into());
                }
                self.set_alpha(asset.id, asset.width, asset.height, image.as_raw())?;
            }
        }
        self.alpha_images
            .retain(|id, _| project.assets.iter().any(|a| a.id == *id));
        Ok(())
    }
    pub fn preflight_project(&self, project: &aem_core::Project) -> Result<(), String> {
        for layer in &project.layers {
            for (chain_index, e) in layer.effects.iter().filter(|e| e.enabled).enumerate() {
                let location = format!("layer {}, effect {} ({})", layer.id, e.id, e.effect);
                let package = self
                    .registry
                    .resolve(&e.plugin, &e.version, &e.hash)
                    .map_err(|err| format!("{location}: {err}"))?;
                let definition = package
                    .manifest
                    .effects
                    .iter()
                    .find(|d| d.id == e.effect)
                    .ok_or_else(|| format!("{location}: effect definition missing"))?;
                if definition.renderer != aem_effects::RendererKind::Image && chain_index != 0 {
                    return Err(format!(
                        "{location}: scene generator must be first enabled effect"
                    ));
                }
                if (definition.renderer != aem_effects::RendererKind::Image) != e.scene.is_some() {
                    return Err(format!(
                        "{location}: saved scene contract differs from plugin"
                    ));
                }
                if let Some(source) = e.scene.as_ref().and_then(|s| s.source_layer) {
                    if !project.layers.iter().any(|l| l.id == source) {
                        return Err(format!("{location}: source layer {source} missing"));
                    }
                }
                if definition.compatibility == aem_effects::Compatibility::Unsupported {
                    return Err(format!("{location}: effect is unsupported"));
                }
                if definition.params.len() != e.params.len() {
                    return Err(format!(
                        "{location}: saved parameter layout differs from the plugin"
                    ));
                }
                for p in &definition.params {
                    let saved = e
                        .params
                        .get(&p.id)
                        .ok_or_else(|| format!("{location}: parameter {} missing", p.id))?;
                    if saved.kind != p.kind
                        || saved.animatable != p.animatable
                        || saved.min != p.min
                        || saved.max != p.max
                        || saved.implemented != p.implemented
                    {
                        return Err(format!(
                            "{location}: parameter {} contract differs from the plugin",
                            p.id
                        ));
                    }
                    if !p.implemented
                        && (saved.track.value != p.default
                            || saved.track.keys.iter().any(|k| k.value != p.default))
                    {
                        return Err(format!("{location}: parameter {} is not implemented", p.id));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn new(registry: Registry) -> Result<Self, String> {
        let raster=utility("fn main_fx(p:vec2<f32>)->vec4<f32>{let c=sample_input(p);return vec4(c.rgb*fx.params[0].rgb*fx.params[0].a,c.a*fx.params[0].a);}")?;
        let convert=utility("fn main_fx(p:vec2<f32>)->vec4<f32>{return convert_pixel(sample_input(p),fx.mode.yz,fx.output_mode.xy);}")?;
        Ok(Self {
            registry,
            device_dimension: 8192,
            program_errors: Default::default(),
            programs: vec![
                EffectProgram {
                    key: "sdk-raster".into(),
                    shader: raster,
                    package: None,
                    resources: vec![],
                },
                EffectProgram {
                    key: "sdk-convert".into(),
                    shader: convert,
                    package: None,
                    resources: vec![],
                },
            ],
            resolved: Vec::new(),
            frame: EffectFramePlan::default(),
            alpha_images: Default::default(),
            generator_scratch: Default::default(),
        })
    }
    pub fn set_registry(&mut self, registry: Registry) {
        self.registry = registry;
        self.resolved.clear();
        self.program_errors.clear();
        self.programs.truncate(2);
        self.frame = EffectFramePlan::default();
    }
    pub fn synchronize(&mut self, scene: &Scene) -> Result<(), String> {
        self.resolved.resize_with(scene.effects.len(), || None);
        for (index, e) in scene.effects.iter().enumerate() {
            if self.resolved[index].as_ref().is_some_and(|r| {
                r.signature.0 == e.plugin
                    && r.signature.1 == e.version
                    && r.signature.2 == e.hash
                    && r.signature.3 == e.effect
            }) {
                continue;
            }
            let signature = (
                e.plugin.clone(),
                e.version.clone(),
                e.hash.clone(),
                e.effect.clone(),
            );
            let result = (|| {
                let package = self
                    .registry
                    .resolve(&e.plugin, &e.version, &e.hash)
                    .map_err(|e| e.to_string())?;
                let definition = package
                    .manifest
                    .effects
                    .iter()
                    .find(|d| d.id == e.effect)
                    .cloned()
                    .ok_or("effect definition is missing")?;
                if (definition.renderer != aem_effects::RendererKind::Image) != e.scene.is_some() {
                    return Err("saved scene contract differs from plugin".into());
                }
                if definition.compatibility == aem_effects::Compatibility::Unsupported {
                    return Err("effect is declared unsupported".into());
                }
                if definition.params.len() != e.param_ids.len() {
                    return Err("saved parameters do not match plugin definition".into());
                }
                let mapping = definition
                    .params
                    .iter()
                    .map(|p| {
                        e.param_ids
                            .iter()
                            .position(|id| id == &p.id)
                            .ok_or("saved parameter ID is missing".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut programs = Vec::new();
                for i in 0..definition.passes.len() {
                    let key = format!("{}:{}:{i}", package.hash, definition.id);
                    let id = if let Some(j) = self.programs.iter().position(|p| p.key == key) {
                        j
                    } else {
                        let shader = Arc::new(package.shaders[&(definition.id.clone(), i)].clone());
                        self.programs.push(EffectProgram {
                            key,
                            shader,
                            package: Some(package.clone()),
                            resources: definition.resources.clone(),
                        });
                        self.programs.len() - 1
                    };
                    programs.push(id as u32);
                }
                Ok(Resolved {
                    signature: signature.clone(),
                    definition,
                    programs,
                    mapping,
                    error: None,
                })
            })();
            self.resolved[index] = Some(match result {
                Ok(v) => v,
                Err(err) => Resolved {
                    signature,
                    definition: aem_effects::builtin::manifest().effects.remove(0),
                    programs: vec![],
                    mapping: vec![],
                    error: Some(err),
                },
            });
        }
        Ok(())
    }
    pub fn build(
        &mut self,
        scene: &Scene,
        assets: &[u64],
        width: u32,
        height: u32,
        strict: bool,
    ) -> Result<&EffectFramePlan, String> {
        self.synchronize(scene)?;
        self.frame.sprites.clear();
        self.frame.generator_stats = Default::default();
        self.frame.width = 0;
        self.frame.height = 0;
        self.frame.slots = 0;
        self.frame.draws.clear();
        self.frame.passes.clear();
        self.frame.diagnostics.clear();
        if !scene.effects.iter().any(|e| e.enabled) {
            self.frame.width = 0;
            self.frame.height = 0;
            self.frame.slots = 0;
        }
        // Dependencies on hidden/transparent layers are still required by formal output.
        if strict {
            let mut preceding = std::collections::BTreeSet::new();
            for (index, e) in scene.effects.iter().enumerate().filter(|(_, e)| e.enabled) {
                let resolved = self.resolved[index].as_ref().unwrap();
                if resolved.definition.renderer != aem_effects::RendererKind::Image
                    && preceding.contains(&e.layer)
                {
                    return Err(format!(
                        "layer {}, effect {}: scene generator must be first enabled effect",
                        e.layer, e.instance
                    ));
                }
                preceding.insert(e.layer);
                if let Some(err) = &resolved.error {
                    return Err(format!("layer {}, effect {}: {err}", e.layer, e.instance));
                }
                if resolved.definition.renderer != aem_effects::RendererKind::Image {
                    crate::scene_generator::validate_settings(e, resolved.definition.renderer)
                        .map_err(|error| {
                            format!("layer {}, effect {}: {error}", e.layer, e.instance)
                        })?;
                    if let Some(source) = e.scene.as_ref().and_then(|s| s.source_layer) {
                        if scene.world_matrix(source).is_none() {
                            return Err(format!(
                                "layer {}, effect {}: source layer {source} missing",
                                e.layer, e.instance
                            ));
                        }
                    }
                }
                for id in &resolved.programs {
                    if let Some(error) = self.program_errors.get(id) {
                        return Err(format!("layer {}, effect {}: {error}", e.layer, e.instance));
                    }
                }
                for (i, p) in resolved.definition.params.iter().enumerate() {
                    let value = e.values[resolved.mapping[i]];
                    if value[..p.kind.dimensions()]
                        .iter()
                        .any(|v| !v.is_finite() || *v < p.min || *v > p.max)
                        || (!p.implemented && value != p.default)
                    {
                        return Err(format!(
                            "layer {}, effect {}, parameter {} is invalid or unsupported",
                            e.layer, e.instance, p.id
                        ));
                    }
                }
            }
        }
        let scale = (width as f32 / scene.width as f32)
            .min(height as f32 / scene.height as f32)
            .min(1.0)
            .max(0.001);
        for layer in &scene.layers {
            let asset = assets
                .iter()
                .position(|id| *id == layer.asset.unwrap_or(0))
                .ok_or("image asset is not loaded")?;
            let pass_start = self.frame.passes.len();
            let mut region = [0.0, 0.0, layer.size[0], layer.size[1]];
            let mut materialized = false;
            let mut overlay = false;
            let mut additive = false;
            for (index, e) in scene
                .effects
                .iter()
                .enumerate()
                .filter(|(_, e)| e.layer == layer.id && e.enabled)
            {
                let resolved = self.resolved[index].as_ref().unwrap();
                let result = (|| -> Result<(), String> {
                    if let Some(err) = &resolved.error {
                        return Err(err.clone());
                    }
                    for id in &resolved.programs {
                        if let Some(error) = self.program_errors.get(id) {
                            return Err(error.clone());
                        }
                    }
                    let def = &resolved.definition;
                    let mut params = [[0.0; 4]; 32];
                    for (i, p) in def.params.iter().enumerate() {
                        let v = e.values[resolved.mapping[i]];
                        if v[..p.kind.dimensions()]
                            .iter()
                            .any(|v| !v.is_finite() || *v < p.min || *v > p.max)
                        {
                            return Err(format!("parameter {} exceeds plugin range", p.id));
                        }
                        if !p.implemented && v != p.default {
                            return Err(format!("AE parameter {} is not implemented", p.id));
                        }
                        params[i] = v;
                    }
                    if def.renderer != aem_effects::RendererKind::Image {
                        if materialized {
                            return Err(
                                "a scene generator must be the first enabled effect on its layer"
                                    .into(),
                            );
                        }
                        let w = (((scene.width as f32 * scale).ceil() as u32).div_ceil(128) * 128)
                            .max(self.frame.width);
                        let h = (((scene.height as f32 * scale).ceil() as u32).div_ceil(128) * 128)
                            .max(self.frame.height);
                        let slots = self.frame.slots | 129;
                        if w > self.device_dimension
                            || h > self.device_dimension
                            || scratch_bytes(w, h, slots) > aem_effects::SCRATCH_BUDGET
                        {
                            return Err("scene generator target exceeds texture budget or device dimensions".into());
                        }
                        let start = self.frame.sprites.len();
                        let stats = crate::scene_generator::generate(
                            scene,
                            layer,
                            e,
                            def.renderer,
                            &self.alpha_images,
                            &mut self.frame.sprites,
                            &mut self.generator_scratch,
                        )?;
                        self.frame.generator_stats.alive += stats.alive;
                        self.frame.generator_stats.visible += stats.visible;
                        self.frame.generator_stats.culled += stats.culled;
                        region = [0., 0., scene.width as f32, scene.height as f32];
                        self.frame.passes.push(EffectPass {
                            program: resolved.programs[0],
                            input: -(asset as i32) - 1,
                            source: -(asset as i32) - 1,
                            output: 7,
                            width: (region[2] * scale).ceil() as u32,
                            height: (region[3] * scale).ceil() as u32,
                            lut: e.lut.map_or(-1, |index| index as i32),
                            uniform: EffectUniform {
                                size: [
                                    region[2],
                                    region[3],
                                    (region[2] * scale).ceil(),
                                    (region[3] * scale).ceil(),
                                ],
                                region,
                                input_region: region,
                                source_region: region,
                                clock: [
                                    (e.local_frame / scene.fps as f64) as f32,
                                    e.local_frame as f32,
                                    0.,
                                    e.seed as f32,
                                ],
                                mode: [0.; 4],
                                output_mode: [0., 0., 1., scale],
                                params,
                            },
                            sprite: true,
                            sprite_start: start as u32,
                            sprite_count: (self.frame.sprites.len() - start) as u32,
                        });
                        let mut conversion = self.frame.passes.last().unwrap().uniform;
                        conversion.output_mode = [0., 0., 1., scale];
                        self.frame.passes.push(EffectPass {
                            program: 1,
                            input: 7,
                            source: 7,
                            output: 0,
                            width: (region[2] * scale).ceil() as u32,
                            height: (region[3] * scale).ceil() as u32,
                            lut: -1,
                            uniform: conversion,
                            sprite: false,
                            sprite_start: 0,
                            sprite_count: 0,
                        });
                        self.frame.width = w;
                        self.frame.height = h;
                        self.frame.slots = slots;
                        materialized = true;
                        overlay = true;
                        additive = def.blend == aem_effects::SpriteBlend::Additive;
                        return Ok(());
                    }
                    let pad = bounds(&def.padding, e, 0)?;
                    if pad < 0.0 {
                        return Err("negative output padding".into());
                    }
                    let next = [
                        region[0] - pad,
                        region[1] - pad,
                        region[2] + 2.0 * pad,
                        region[3] + 2.0 * pad,
                    ];
                    let work = if def.working_space == WorkingSpace::Srgb {
                        1
                    } else {
                        4
                    };
                    let slots = self.frame.slots | 1 | (7 << work);
                    let w = (((next[2] * scale).ceil() as u32).div_ceil(128) * 128)
                        .max(self.frame.width);
                    let h = (((next[3] * scale).ceil() as u32).div_ceil(128) * 128)
                        .max(self.frame.height);
                    if w > self.device_dimension
                        || h > self.device_dimension
                        || scratch_bytes(w, h, slots) > aem_effects::SCRATCH_BUDGET
                    {
                        return Err(
                            "effect scratch textures exceed 64 MiB or device dimensions".into()
                        );
                    }
                    let working = [
                        if def.working_space == WorkingSpace::Srgb {
                            1.0
                        } else {
                            0.0
                        },
                        if def.alpha_mode == AlphaMode::Straight {
                            1.0
                        } else {
                            0.0
                        },
                    ];
                    let mut edge = match def.edge_mode {
                        EdgeMode::Transparent => 0.0,
                        EdgeMode::Clamp => 1.0,
                        EdgeMode::Repeat => 2.0,
                        EdgeMode::Mirror => 3.0,
                    };
                    if let Some(id) = &def.edge_param {
                        let i = e
                            .param_ids
                            .iter()
                            .position(|p| p == id)
                            .ok_or("unknown edge parameter")?;
                        edge = if e.values[i][0] > 0.5 { 1.0 } else { 0.0 };
                    }
                    let uniform = |out: [f32; 4], input: [f32; 4], source: [f32; 4], pass: f32| {
                        EffectUniform {
                            size: [
                                layer.size[0],
                                layer.size[1],
                                (out[2] * scale).ceil(),
                                (out[3] * scale).ceil(),
                            ],
                            region: out,
                            input_region: input,
                            source_region: source,
                            clock: [
                                (e.local_frame / f64::from(scene.fps)) as f32,
                                e.local_frame as f32,
                                pass,
                                e.seed as f32,
                            ],
                            mode: [edge, working[0], working[1], 0.0],
                            output_mode: [working[0], working[1], 1.0, scale],
                            params,
                        }
                    };
                    let push = |frame: &mut EffectFramePlan,
                                program: u32,
                                input: i32,
                                source: i32,
                                output: u32,
                                u: EffectUniform,
                                lut: i32| {
                        frame.passes.push(EffectPass {
                            program,
                            input,
                            source,
                            output,
                            width: u.size[2] as u32,
                            height: u.size[3] as u32,
                            lut,
                            uniform: u,
                            sprite: false,
                            sprite_start: 0,
                            sprite_count: 0,
                        });
                    };
                    if !materialized {
                        let mut u = uniform(region, region, region, 0.0);
                        u.mode = [0.0, 0.0, 0.0, 1.0];
                        u.params[0] = [
                            linear(layer.color[0]),
                            linear(layer.color[1]),
                            linear(layer.color[2]),
                            layer.color[3],
                        ];
                        push(
                            &mut self.frame,
                            0,
                            -(asset as i32) - 1,
                            -(asset as i32) - 1,
                            0,
                            u,
                            -1,
                        );
                    }
                    let mut u = uniform(region, region, region, 0.0);
                    u.mode = [0.0, 0.0, 0.0, 0.0];
                    push(&mut self.frame, 1, 0, 0, work, u, -1);
                    let source_region = region;
                    let mut input = work as i32;
                    let mut input_region = region;
                    for (i, program) in resolved.programs.iter().enumerate() {
                        let output = work + 1 + (i as u32 % 2);
                        let mut u = uniform(next, input_region, source_region, i as f32);
                        if i + 1 == resolved.programs.len() {
                            u.output_mode[2] = def
                                .params
                                .iter()
                                .position(|p| p.id == "effect_opacity")
                                .map_or(1.0, |i| params[i][0] / 100.0);
                        }
                        push(
                            &mut self.frame,
                            *program,
                            input,
                            work as i32,
                            output,
                            u,
                            e.lut.map_or(-1, |v| v as i32),
                        );
                        input = output as i32;
                        input_region = next;
                    }
                    let mut u = uniform(next, next, source_region, 0.0);
                    u.output_mode = [0.0, 0.0, 1.0, scale];
                    push(&mut self.frame, 1, input, input, 0, u, -1);
                    self.frame.width = w;
                    self.frame.height = h;
                    self.frame.slots = slots;
                    region = next;
                    materialized = true;
                    Ok(())
                })();
                if let Err(err) = result {
                    let message = format!(
                        "layer {}, effect {} ({}): {err}",
                        layer.id, e.instance, e.effect
                    );
                    if strict {
                        return Err(message);
                    }
                    self.frame.diagnostics.push(message);
                }
            }
            let mut words = [0.0; 32];
            let mvp = if overlay {
                glam::Mat4::from_scale(glam::Vec3::new(
                    2. / scene.width as f32,
                    2. / scene.height as f32,
                    1.,
                ))
            } else {
                scene.camera.view_projection * layer.model
            };
            words[..16].copy_from_slice(&mvp.to_cols_array());
            words[16..20].copy_from_slice(&if materialized {
                [1.0; 4]
            } else {
                [
                    linear(layer.color[0]),
                    linear(layer.color[1]),
                    linear(layer.color[2]),
                    layer.color[3],
                ]
            });
            words[20] = region[2];
            words[21] = region[3];
            words[22] = layer.opacity;
            words[23] = if additive { 1. } else { 0. };
            words[24] = asset as f32;
            words[25] = 1.0;
            words[26] = 1.0;
            words[27] = if materialized { 0.0 } else { -1.0 };
            words[28] = pass_start as f32;
            words[29] = self.frame.passes.len() as f32;
            self.frame.draws.push(PlannedDraw {
                layer: layer.id,
                words,
                pass_start,
                pass_end: self.frame.passes.len(),
            });
        }
        if self.frame.width > 0 {
            for d in &mut self.frame.draws {
                if d.words[27] >= 0.0 {
                    d.words[25] = (d.words[20] * scale).ceil() / self.frame.width as f32;
                    d.words[26] = (d.words[21] * scale).ceil() / self.frame.height as f32;
                }
            }
        }
        Ok(&self.frame)
    }
}
