//! Stateless scene sampling shared by wgpu preview and GLES export.
use motion_core::{DrawLayer, SampledEffect, Scene};
use motion_effects::{RendererKind, MAX_PARTICLES, MAX_SPRITES};
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Sprite {
    /// NDC centre and full width/height; the shader sees local UV coordinates.
    pub rect: [f32; 4],
    pub color: [f32; 4],
    pub style: [f32; 4],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct GeneratorStats {
    pub alive: u32,
    pub visible: u32,
    pub culled: u32,
    /// Birth poses actually sampled this call (overlapping history is reused).
    pub births_sampled: u32,
}
#[derive(Default)]
pub struct GeneratorScratch {
    pub(crate) sorted: Vec<(f32, i64, Sprite)>,
    pub(crate) births: HashMap<(u64, u64), crate::particle_emitter::BirthCache>,
}
impl GeneratorScratch {
    pub(crate) fn retain(&mut self, scene: &Scene) {
        self.births.retain(|&(layer, instance), _| scene.layers.iter().any(|l| l.id == layer)
            && scene.effects.iter().any(|e| e.layer == layer && e.instance == instance && e.enabled && e.particle_history.is_some()));
    }
}
pub struct AlphaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
impl AlphaImage {
    fn sample(&self, uv: Vec2) -> f32 {
        // Match the compositor's linear filtering and clamped texel centres.
        let p = uv * Vec2::new(self.width as f32, self.height as f32) - Vec2::splat(0.5);
        let a = p.floor();
        let t = p - a;
        let at = |x: f32, y: f32| {
            self.pixels[(y.clamp(0., self.height as f32 - 1.) as u32 * self.width
                + x.clamp(0., self.width as f32 - 1.) as u32) as usize] as f32
                / 255.
        };
        let x0 = at(a.x, a.y) * (1. - t.x) + at(a.x + 1., a.y) * t.x;
        let x1 = at(a.x, a.y + 1.) * (1. - t.x) + at(a.x + 1., a.y + 1.) * t.x;
        x0 * (1. - t.y) + x1 * t.y
    }
}
fn param(e: &SampledEffect, id: &str) -> Result<[f32; 4], String> {
    e.param_ids
        .iter()
        .position(|p| p == id)
        .map(|i| e.values[i])
        .ok_or_else(|| format!("generator parameter {id} missing"))
}
pub(crate) fn random(seed: u32, id: u32, channel: u32) -> f32 {
    let mut v = seed ^ id.wrapping_mul(0x9e3779b9) ^ channel.wrapping_mul(0x85ebca6b);
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846ca68b);
    v ^= v >> 16;
    (v >> 8) as f32 / 16777216.
}
pub(crate) fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub(crate) fn visible(rect: [f32; 4]) -> bool {
    rect[0] + rect[2] * 0.5 >= -1.
        && rect[0] - rect[2] * 0.5 <= 1.
        && rect[1] + rect[3] * 0.5 >= -1.
        && rect[1] - rect[3] * 0.5 <= 1.
}
pub fn validate_settings(e: &SampledEffect, kind: RendererKind) -> Result<(), String> {
    let settings = e.scene.as_ref().ok_or("scene generator settings missing")?;
    settings.validate().map_err(|e| e.to_string())?;
    if kind != RendererKind::ParticleEmitter && (settings.particle_space.is_some() || settings.sprite_asset.is_some()) {
        return Err("legacy generators cannot use world-birth particle settings".into());
    }
    if matches!(kind, RendererKind::Particles | RendererKind::ParticleEmitter) {
        if (kind == RendererKind::Particles && settings.source_layer.is_some()) || settings.occlusion || !settings.elements.is_empty() {
            return Err("particle generator does not accept lens settings".into());
        }
        let rate = param(e, "rate")?[0] as f64;
        let life = param(e, "lifetime")?[0] as f64;
        if !(0. ..=10000.).contains(&rate) || !(0.001..=120.).contains(&life) {
            return Err("invalid particle birth rate or lifetime".into());
        }
        if (rate * life).ceil() as usize > MAX_PARTICLES {
            return Err(format!(
                "particle capacity exceeds {MAX_PARTICLES}; reduce rate or lifetime"
            ));
        }
        if kind == RendererKind::ParticleEmitter {
            if let Some(error) = &e.particle_history_error { return Err(error.clone()); }
            if settings.particle_space != Some(motion_effects::ParticleSpace::WorldBirth) || e.particle_history.is_none() {
                return Err("world-birth particle history missing".into());
            }
        }
    }
    Ok(())
}
fn sphere_visible(vp: glam::Mat4, centre: Vec3, radius: f32) -> bool {
    let m = vp.transpose();
    [
        m.w_axis + m.x_axis,
        m.w_axis - m.x_axis,
        m.w_axis + m.y_axis,
        m.w_axis - m.y_axis,
        m.z_axis,
        m.w_axis - m.z_axis,
    ]
    .into_iter()
    .all(|p| p.dot(centre.extend(1.)) >= -radius * p.truncate().length())
}
pub fn generate(
    scene: &Scene,
    layer: &DrawLayer,
    e: &SampledEffect,
    kind: RendererKind,
    alpha: &HashMap<u64, AlphaImage>,
    out: &mut Vec<Sprite>,
    scratch: &mut GeneratorScratch,
) -> Result<GeneratorStats, String> {
    let start = out.len();
    let result = generate_inner(scene, layer, e, kind, alpha, out, scratch);
    if result.is_err() {
        out.truncate(start);
    }
    result
}
fn generate_inner(
    scene: &Scene,
    layer: &DrawLayer,
    e: &SampledEffect,
    kind: RendererKind,
    alpha: &HashMap<u64, AlphaImage>,
    out: &mut Vec<Sprite>,
    scratch: &mut GeneratorScratch,
) -> Result<GeneratorStats, String> {
    validate_settings(e, kind)?;
    if kind == RendererKind::ParticleEmitter {
        return crate::particle_emitter::generate(scene, layer, e, out, scratch);
    }
    let settings = e.scene.as_ref().ok_or("scene generator settings missing")?;
    settings.validate().map_err(|e| e.to_string())?;
    let start = out.len();
    let mut stats = GeneratorStats::default();
    if kind == RendererKind::Particles {
        if settings.source_layer.is_some() || settings.occlusion || !settings.elements.is_empty() {
            return Err("particle generator does not accept lens settings".into());
        }
        let rate = param(e, "rate")?[0] as f64;
        let life = param(e, "lifetime")?[0] as f64;
        if !(0. ..=10000.).contains(&rate) || !(0.001..=120.).contains(&life) {
            return Err("invalid particle birth rate or lifetime".into());
        }
        if (rate * life).ceil() as usize > MAX_PARTICLES {
            return Err(format!(
                "particle capacity exceeds {MAX_PARTICLES}; reduce rate or lifetime"
            ));
        }
        if rate == 0. {
            return Ok(stats);
        }
        let t = e.local_frame / scene.fps as f64;
        if t < 0. {
            return Ok(stats);
        }
        let prewarm = param(e, "prewarm")?[0] > 0.5;
        let first =
            (((t - life) * rate).floor() as i64 + 1).max(if prewarm { i64::MIN } else { 0 });
        let last = (t * rate).floor() as i64;
        let speed = param(e, "speed")?[0];
        let spread = param(e, "spread")?[0];
        let gravity = Vec3::from_array(param(e, "gravity")?[..3].try_into().unwrap());
        let extent = Vec3::from_array(param(e, "extent")?[..3].try_into().unwrap());
        let shape = param(e, "shape")?[0] as u32;
        let size = param(e, "size")?[0];
        let end_size = param(e, "end_size")?[0];
        let color = param(e, "color")?;
        let end_color = param(e, "end_color")?;
        let fade = param(e, "fade")?[0].clamp(0., 0.5);
        if shape > 2
            || size < 0.
            || end_size < 0.
            || extent.min_element() < 0.
            || ![speed, spread, size, end_size]
                .into_iter()
                .all(f32::is_finite)
        {
            return Err("invalid particle geometry".into());
        }
        let matrix = scene
            .world_matrix(layer.id)
            .ok_or("emitter transform missing")?;
        let scale = matrix
            .x_axis
            .truncate()
            .length()
            .max(matrix.y_axis.truncate().length())
            .max(matrix.z_axis.truncate().length());
        let forward = (scene.camera.target - scene.camera.eye).normalize();
        let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        // Camera roll is encoded in the VP matrix; recover the actual view-plane basis.
        let vp = layer.view_projection;
        let row0 = Vec3::new(vp.x_axis.x, vp.y_axis.x, vp.z_axis.x).normalize_or_zero();
        let row1 = Vec3::new(vp.x_axis.y, vp.y_axis.y, vp.z_axis.y).normalize_or_zero();
        let right = if row0.length_squared() > 0. {
            row0
        } else {
            right
        };
        let alive = (last - first + 1).max(0) as u32;
        let matrix_norm = (matrix.x_axis.truncate().length_squared()
            + matrix.y_axis.truncate().length_squared()
            + matrix.z_axis.truncate().length_squared())
        .sqrt();
        let radius = (extent.length() * 0.5
            + (speed.abs() + spread) * life as f32
            + gravity.length() * 0.5 * (life * life) as f32
            + size.max(end_size) * 0.5)
            * matrix_norm;
        if !radius.is_finite() {
            return Err("emitter bounds exceed numeric range".into());
        }
        if !sphere_visible(vp, matrix.w_axis.truncate(), radius) {
            return Ok(GeneratorStats {
                alive,
                visible: 0,
                culled: alive,
                births_sampled: 0,
            });
        }
        let sampled = &mut scratch.sorted;
        sampled.clear();
        sampled.reserve(alive as usize);
        for id in first..=last {
            let age = (t - id as f64 / rate) as f32;
            let progress = (age / life as f32).clamp(0., 1.);
            let birth_id = id;
            let id = id as u32;
            let r = |c| random(e.seed, id, c);
            let z = 2. * r(0) - 1.;
            let theta = r(1) * std::f32::consts::TAU;
            let direction = Vec3::new(
                (1. - z * z).sqrt() * theta.cos(),
                z,
                (1. - z * z).sqrt() * theta.sin(),
            );
            let birth = match shape {
                1 => Vec3::new(r(2) - 0.5, r(3) - 0.5, r(4) - 0.5) * extent,
                2 => direction * r(2).cbrt() * extent * 0.5,
                _ => Vec3::ZERO,
            };
            let velocity = Vec3::new(
                direction.x * spread,
                -speed + direction.y * spread,
                direction.z * spread,
            );
            let local = birth + velocity * age + gravity * (0.5 * age * age);
            let world = matrix.transform_point3(Vec3::new(local.x, -local.y, -local.z));
            let clip = vp * world.extend(1.);
            stats.alive += 1;
            if !clip.is_finite() {
                return Err("particle trajectory exceeds numeric range".into());
            }
            if clip.w <= 1e-6 || clip.z < 0. || clip.z > clip.w {
                continue;
            }
            let diameter = (size + (end_size - size) * progress) * scale;
            let dx = vp * (right * diameter).extend(0.);
            let dy = vp * (row1 * diameter).extend(0.);
            let rect = [
                clip.x / clip.w,
                clip.y / clip.w,
                dx.x.abs() / clip.w,
                dy.y.abs() / clip.w,
            ];
            if !rect.into_iter().all(f32::is_finite) {
                return Err("particle size exceeds numeric range".into());
            }
            let fading = if fade > 0. {
                (progress / fade).min((1. - progress) / fade).clamp(0., 1.)
            } else {
                1.
            };
            let c =
                std::array::from_fn::<_, 4, _>(|i| color[i] + (end_color[i] - color[i]) * progress);
            if c[3] * fading <= 0. || diameter <= 0. || !visible(rect) {
                continue;
            }
            sampled.push((
                (world - scene.camera.eye).dot(forward),
                birth_id,
                Sprite {
                    rect,
                    color: [
                        linear(c[0]) * c[3] * fading,
                        linear(c[1]) * c[3] * fading,
                        linear(c[2]) * c[3] * fading,
                        c[3] * fading,
                    ],
                    style: [0., 0., 0., progress],
                },
            ));
        }
        sampled.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        out.extend(sampled.drain(..).map(|(_, _, s)| s));
    } else {
        let position = param(e, "position")?;
        let mut world = if let Some(source) = settings.source_layer {
            scene
                .world_matrix(source)
                .ok_or_else(|| format!("source layer {source} missing"))?
                .w_axis
                .truncate()
        } else {
            motion_core::to_world(position[..3].try_into().unwrap(), scene.width, scene.height)
        };
        let source_spatial = settings.source_layer.map_or(layer.three_d, |id| {
            scene.node_is_spatial(id).unwrap_or(false)
        });
        if !source_spatial {
            world.z = 0.;
        }
        let vp = if source_spatial {
            scene.camera.view_projection
        } else {
            glam::Mat4::orthographic_rh(
                -(scene.width as f32) * 0.5,
                scene.width as f32 * 0.5,
                -(scene.height as f32) * 0.5,
                scene.height as f32 * 0.5,
                -1.,
                1.,
            )
        };
        let clip = vp * world.extend(1.);
        if !clip.is_finite() {
            return Err("lens source exceeds numeric range".into());
        }
        if clip.w <= 1e-6 || clip.z < 0. || clip.z > clip.w {
            return Ok(stats);
        }
        let centre = Vec2::new(clip.x / clip.w, clip.y / clip.w);
        let mut intensity = param(e, "intensity")?[0];
        if param(e, "attenuation")?[0] > 0.5 {
            let distance = (world - scene.camera.eye).length();
            let reference = param(e, "reference_distance")?[0];
            intensity *= (reference / distance.max(1.)).powi(2).min(32.);
        }
        if settings.occlusion {
            intensity *= visibility(
                scene,
                layer.id,
                world,
                param(e, "occlusion_radius")?[0],
                alpha,
                vp,
            )?;
        }
        let scale = param(e, "scale")?[0] / 100.;
        for element in settings.elements.iter().filter(|e| e.enabled) {
            stats.alive += 1;
            let p = centre * (1. - element.offset);
            let rect = [
                p.x,
                p.y,
                element.size[0] * scale * 2. / scene.width as f32,
                element.size[1] * scale * 2. / scene.height as f32,
            ];
            let a = element.color[3] * intensity * element.intensity;
            if !visible(rect) || a <= 0. || scale <= 0. {
                continue;
            }
            out.push(Sprite {
                rect,
                color: [
                    linear(element.color[0]) * a,
                    linear(element.color[1]) * a,
                    linear(element.color[2]) * a,
                    a.clamp(0., 1.),
                ],
                style: [
                    element.shape.code(),
                    element.rays as f32,
                    element.chromatic,
                    1.,
                ],
            });
        }
    }
    if out.len() > MAX_SPRITES {
        out.truncate(start);
        return Err(format!("frame sprite budget exceeds {MAX_SPRITES}"));
    }
    stats.visible = (out.len() - start) as u32;
    stats.culled = stats.alive - stats.visible;
    Ok(stats)
}

fn visibility(
    scene: &Scene,
    owner: u64,
    light: Vec3,
    radius: f32,
    alpha: &HashMap<u64, AlphaImage>,
    vp: glam::Mat4,
) -> Result<f32, String> {
    let right = Vec3::new(vp.x_axis.x, vp.y_axis.x, vp.z_axis.x).normalize_or_zero();
    let up = Vec3::new(vp.x_axis.y, vp.y_axis.y, vp.z_axis.y).normalize_or_zero();
    let clip = vp * light.extend(1.);
    let world_radius =
        radius * clip.w / ((vp * right.extend(0.)).x.abs() * scene.width as f32 * 0.5).max(1e-6);
    let mut sum = 0.;
    for (x, y) in [
        (0., 0.),
        (-1., 0.),
        (1., 0.),
        (0., -1.),
        (0., 1.),
        (-0.7, -0.7),
        (0.7, -0.7),
        (-0.7, 0.7),
        (0.7, 0.7),
    ] {
        let end = light + (right * x + up * y) * world_radius;
        let end_clip = vp * end.extend(1.);
        let near = vp.inverse()
            * glam::Vec4::new(end_clip.x / end_clip.w, end_clip.y / end_clip.w, 0., 1.);
        let ray_origin = near.truncate() / near.w;
        let mut transmission = 1.;
        for layer in &scene.layers {
            if layer.id == owner
                || scene
                    .effects
                    .iter()
                    .any(|e| e.layer == layer.id && e.enabled && e.scene.is_some())
            {
                continue;
            }
            if layer.model.determinant().abs() < 1e-8 {
                continue;
            }
            let inv = layer.model.inverse();
            let origin = inv.transform_point3(ray_origin);
            let target = inv.transform_point3(end);
            let delta = target - origin;
            if delta.z.abs() < 1e-6 {
                continue;
            }
            let t = -origin.z / delta.z;
            if t <= 0. || t >= 1. {
                continue;
            }
            let p = origin + delta * t;
            let uv = Vec2::new(p.x / layer.size[0] + 0.5, 0.5 - p.y / layer.size[1]);
            if uv.min_element() < 0. || uv.max_element() > 1. {
                continue;
            }
            let texture = if let Some(asset) = layer.asset {
                alpha
                    .get(&asset)
                    .ok_or_else(|| format!("occlusion alpha for asset {asset} is not loaded"))?
                    .sample(uv)
            } else {
                1.
            };
            transmission *= 1. - (texture * layer.color[3] * layer.opacity).clamp(0., 1.);
        }
        sum += transmission;
    }
    Ok(sum / 9.)
}
