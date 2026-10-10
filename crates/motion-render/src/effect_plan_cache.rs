//! One retained preview plan, keyed by sampled inputs rather than document revision.
//! Video PTS and shader clocks advance independently of static pass geometry.
use motion_core::{DrawLayer, SampledEffect, Scene};
use motion_effects::Registry;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct PreviewInputs {
    layers: Vec<DrawLayer>,
    effects: Vec<SampledEffect>,
    registry: Registry,
    errors: BTreeMap<u32, String>,
    camera: motion_core::CameraPose,
    dimensions: [u32; 5],
    composition: String,
    device_dimension: u32,
    assets: Vec<u64>,
}
fn same_layer(a: &DrawLayer, b: &DrawLayer) -> bool {
    // GPU uniforms can distinguish +0.0 from -0.0 (e.g. bitcast/atan2).
    // Compare bytes rather than numeric float equality for cached payloads.
    a.id == b.id
        && bytemuck::bytes_of(&a.model.to_cols_array())
            == bytemuck::bytes_of(&b.model.to_cols_array())
        && bytemuck::bytes_of(&a.size) == bytemuck::bytes_of(&b.size)
        && bytemuck::bytes_of(&a.source_size) == bytemuck::bytes_of(&b.source_size)
        && bytemuck::bytes_of(&a.source_rect) == bytemuck::bytes_of(&b.source_rect)
        && bytemuck::bytes_of(&a.color) == bytemuck::bytes_of(&b.color)
        && a.opacity.to_bits() == b.opacity.to_bits()
        && a.asset == b.asset
        && a.video.as_ref().map(|v| v.asset) == b.video.as_ref().map(|v| v.asset)
        && match (&a.vector, &b.vector) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
        && Arc::ptr_eq(&a.masks, &b.masks)
        && a.adjustment == b.adjustment
        && a.composition == b.composition
        && a.depth.to_bits() == b.depth.to_bits()
        && a.order == b.order
        && a.three_d == b.three_d
        && bytemuck::bytes_of(&a.view_projection.to_cols_array())
            == bytemuck::bytes_of(&b.view_projection.to_cols_array())
}
fn same_effect(a: &SampledEffect, b: &SampledEffect) -> bool {
    a.layer == b.layer
        && a.instance == b.instance
        && a.plugin == b.plugin
        && a.effect == b.effect
        && a.version == b.version
        && a.hash == b.hash
        && a.enabled == b.enabled
        && a.seed == b.seed
        && a.image_input == b.image_input
        && a.param_ids == b.param_ids
        && bytemuck::bytes_of(&a.values) == bytemuck::bytes_of(&b.values)
        && a.lut == b.lut
}
impl PreviewInputs {
    pub fn capture(
        scene: &Scene,
        assets: &[u64],
        width: u32,
        height: u32,
        device_dimension: u32,
        registry: &Registry,
        errors: &BTreeMap<u32, String>,
    ) -> Self {
        Self {
            layers: scene.layers.clone(),
            effects: scene.effects.clone(),
            registry: registry.clone(),
            errors: errors.clone(),
            camera: scene.camera,
            dimensions: [scene.width, scene.height, scene.fps, width, height],
            composition: scene.composition_id.clone(),
            device_dimension,
            assets: assets.to_vec(),
        }
    }
    pub fn registry_matches(&self, registry: &Registry) -> bool {
        self.registry.disabled == registry.disabled
            && self.registry.packages.len() == registry.packages.len()
            && self
                .registry
                .packages
                .iter()
                .zip(&registry.packages)
                .all(|((ak, a), (bk, b))| ak == bk && Arc::ptr_eq(a, b))
    }
    pub fn matches(
        &self,
        scene: &Scene,
        assets: &[u64],
        width: u32,
        height: u32,
        device_dimension: u32,
        registry: &Registry,
        errors: &BTreeMap<u32, String>,
    ) -> bool {
        self.dimensions == [scene.width, scene.height, scene.fps, width, height]
            && self.composition == scene.composition_id
            && self.device_dimension == device_dimension
            && self.assets == assets
            && self.errors == *errors
            && self.registry_matches(registry)
            && self.camera.eye == scene.camera.eye
            && self.camera.target == scene.camera.target
            && self.camera.projection == scene.camera.projection
            && self.camera.view_projection == scene.camera.view_projection
            && self.layers.len() == scene.layers.len()
            && self
                .layers
                .iter()
                .zip(&scene.layers)
                .all(|(a, b)| same_layer(a, b))
            && self.effects.len() == scene.effects.len()
            && self
                .effects
                .iter()
                .zip(&scene.effects)
                .all(|(a, b)| same_effect(a, b))
    }
}
