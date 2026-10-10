//! SDK 5 world-birth particles: bounded pose cache + seek-independent analytic motion.
use crate::scene_generator::{random, GeneratorScratch, GeneratorStats, Sprite};
use motion_core::{particle_history::ParticleHistory, DrawLayer, SampledEffect, Scene};
use glam::{Mat4, Vec3};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone)]
struct Birth {
    id: i64,
    position: Vec3,
    velocity: Vec3,
    size: [f32; 2],
    color: [[f32; 4]; 2],
    fade: f32,
}
pub(crate) struct BirthCache {
    history: Arc<ParticleHistory>,
    seed: u32,
    particles: VecDeque<Birth>,
    world: Vec<Mat4>,
    states: Vec<u8>,
}
impl BirthCache {
    pub(crate) fn capacity(&self) -> usize {
        self.particles.capacity()
    }
}
fn project_vector(v: [f32; 4]) -> Vec3 {
    Vec3::new(v[0], -v[1], -v[2])
}
fn read(h: &ParticleHistory, name: &str, t: f64) -> Result<[f32; 4], String> {
    h.parameter(name, t).map_err(|e| e.to_string())
}
fn centre(c: &mut BirthCache, t: f64) -> Result<Vec3, String> {
    let m = c
        .history
        .matrix_at(t, &mut c.world, &mut c.states)
        .map_err(|e| e.to_string())?;
    Ok(m.transform_point3(project_vector(read(&c.history, "position", t)?)))
}
fn birth(c: &mut BirthCache, id: i64, rate: f64) -> Result<Birth, String> {
    let t = id as f64 / rate;
    let h = c.history.clone();
    let m = h
        .matrix_at(t, &mut c.world, &mut c.states)
        .map_err(|e| e.to_string())?;
    let r = |channel| random(c.seed, id as u32, channel);
    let z = 2. * r(0) - 1.;
    let theta = r(1) * std::f32::consts::TAU;
    let random_direction = Vec3::new(
        (1. - z * z).sqrt() * theta.cos(),
        z,
        (1. - z * z).sqrt() * theta.sin(),
    );
    let extent = Vec3::from_array(read(&h, "extent", t)?[..3].try_into().unwrap());
    let local = match read(&h, "shape", t)?[0] as u32 {
        0 => Vec3::ZERO,
        1 => Vec3::new(r(2) - 0.5, r(3) - 0.5, r(4) - 0.5) * extent,
        2 => random_direction * r(2).cbrt() * extent * 0.5,
        _ => return Err("invalid particle emitter shape".into()),
    };
    let offset = read(&h, "position", t)?;
    let position = m.transform_point3(project_vector([
        offset[0] + local.x,
        offset[1] + local.y,
        offset[2] + local.z,
        0.,
    ]));
    let direction = project_vector(read(&h, "direction", t)?).normalize_or_zero();
    let speed = read(&h, "speed", t)?[0];
    let spread = read(&h, "spread", t)?[0];
    let mut velocity = m.transform_vector3(direction * speed + random_direction * spread);
    let inherit = read(&h, "inherit_velocity", t)?[0];
    if inherit > 0. {
        const H: f64 = 1. / 120.;
        velocity += (centre(c, t + H)? - centre(c, t - H)?) * (inherit / (2. * H) as f32);
    }
    let scale = m
        .x_axis
        .truncate()
        .length()
        .max(m.y_axis.truncate().length())
        .max(m.z_axis.truncate().length());
    let result = Birth {
        id,
        position,
        velocity,
        size: [
            read(&h, "size", t)?[0] * scale,
            read(&h, "end_size", t)?[0] * scale,
        ],
        color: [read(&h, "color", t)?, read(&h, "end_color", t)?],
        fade: read(&h, "fade", t)?[0],
    };
    if !position.is_finite() || !velocity.is_finite() || !result.size.iter().all(|v| v.is_finite())
    {
        return Err("particle birth pose exceeds numeric range".into());
    }
    Ok(result)
}
pub(crate) fn generate(
    scene: &Scene,
    layer: &DrawLayer,
    e: &SampledEffect,
    out: &mut Vec<Sprite>,
    scratch: &mut GeneratorScratch,
) -> Result<GeneratorStats, String> {
    let history = e
        .particle_history
        .as_ref()
        .ok_or("particle history missing")?;
    let rate = read(history, "rate", 0.)?[0] as f64;
    let life = read(history, "lifetime", 0.)?[0] as f64;
    let t = e.local_frame / scene.fps as f64;
    let mut stats = GeneratorStats::default();
    if rate == 0. || t < 0. {
        scratch.births.remove(&(e.layer, e.instance));
        return Ok(stats);
    }
    let first =
        (((t - life) * rate).floor() as i64 + 1).max(if read(history, "prewarm", 0.)?[0] > 0.5 {
            i64::MIN
        } else {
            0
        });
    let last = (t * rate).floor() as i64;
    stats.alive = (last - first + 1).max(0) as u32;
    let cache = scratch
        .births
        .entry((e.layer, e.instance))
        .or_insert_with(|| BirthCache {
            history: history.clone(),
            seed: e.seed,
            particles: VecDeque::new(),
            world: Vec::new(),
            states: Vec::new(),
        });
    if !Arc::ptr_eq(&cache.history, history) || cache.seed != e.seed {
        cache.history = history.clone();
        cache.seed = e.seed;
        cache.particles.clear();
    }
    while cache.particles.front().is_some_and(|b| b.id < first) {
        cache.particles.pop_front();
    }
    while cache.particles.back().is_some_and(|b| b.id > last) {
        cache.particles.pop_back();
    }
    let old_first = cache.particles.front().map_or(last + 1, |b| b.id);
    for id in (first..old_first).rev() {
        let b = birth(cache, id, rate)?;
        cache.particles.push_front(b);
        stats.births_sampled += 1;
    }
    let old_last = cache.particles.back().map_or(first - 1, |b| b.id);
    for id in old_last + 1..=last {
        let b = birth(cache, id, rate)?;
        cache.particles.push_back(b);
        stats.births_sampled += 1;
    }
    let gravity = project_vector(read(history, "gravity", 0.)?);
    let wind = project_vector(read(history, "wind", 0.)?);
    let drag = read(history, "drag", 0.)?[0] as f64;
    let vp = layer.view_projection;
    let sprite_asset = e.scene.as_ref().and_then(|s| s.sprite_asset);
    let aspect = if let Some(asset) = sprite_asset {
        let size = scene
            .sprite_assets
            .get(&asset)
            .ok_or_else(|| format!("particle sprite image {asset} missing"))?;
        let longest = size[0].max(size[1]) as f32;
        [size[0] as f32 / longest, size[1] as f32 / longest]
    } else {
        [1., 1.]
    };
    let right = Vec3::new(vp.x_axis.x, vp.y_axis.x, vp.z_axis.x).normalize_or_zero();
    let up = Vec3::new(vp.x_axis.y, vp.y_axis.y, vp.z_axis.y).normalize_or_zero();
    let forward = (scene.camera.target - scene.camera.eye).normalize_or_zero();
    scratch.sorted.clear();
    for b in &cache.particles {
        let age = t - b.id as f64 / rate;
        let progress = (age / life).clamp(0., 1.) as f32;
        // Exact solution of dv/dt = gravity + drag * (wind - v).
        let (duration, acceleration_time) = if drag == 0. {
            (age, 0.5 * age * age)
        } else {
            let d = -(-drag * age).exp_m1() / drag;
            let g = if drag * age < 1e-4 {
                age * age * (0.5 - drag * age / 6.)
            } else {
                (age - d) / drag
            };
            (d, g)
        };
        let mut world = b.position
            + b.velocity * duration as f32
            + wind * (age - duration) as f32
            + gravity * acceleration_time as f32;
        if !layer.three_d {
            world.z = 0.;
        }
        let clip = vp * world.extend(1.);
        if !clip.is_finite() {
            return Err("particle trajectory exceeds numeric range".into());
        }
        if clip.w <= 1e-6 || clip.z < 0. || clip.z > clip.w {
            continue;
        }
        let diameter = b.size[0] + (b.size[1] - b.size[0]) * progress;
        let dx = vp * (right * diameter * aspect[0]).extend(0.);
        let dy = vp * (up * diameter * aspect[1]).extend(0.);
        let rect = [
            clip.x / clip.w,
            clip.y / clip.w,
            dx.x.abs() / clip.w,
            dy.y.abs() / clip.w,
        ];
        if !rect.iter().all(|v| v.is_finite()) {
            return Err("particle size exceeds numeric range".into());
        }
        let fading = if b.fade > 0. {
            (progress / b.fade)
                .min((1. - progress) / b.fade)
                .clamp(0., 1.)
        } else {
            1.
        };
        let c = std::array::from_fn::<_, 4, _>(|i| {
            b.color[0][i] + (b.color[1][i] - b.color[0][i]) * progress
        });
        if diameter <= 0. || c[3] * fading <= 0. || !super::scene_generator::visible(rect) {
            continue;
        }
        scratch.sorted.push((
            (world - scene.camera.eye).dot(forward),
            b.id,
            Sprite {
                rect,
                color: [
                    super::scene_generator::linear(c[0]) * c[3] * fading,
                    super::scene_generator::linear(c[1]) * c[3] * fading,
                    super::scene_generator::linear(c[2]) * c[3] * fading,
                    c[3] * fading,
                ],
                style: [
                    if sprite_asset.is_some() { 6. } else { 0. },
                    0.,
                    0.,
                    progress,
                ],
            },
        ));
    }
    if out.len() + scratch.sorted.len() > motion_effects::MAX_SPRITES {
        return Err("frame sprite capacity exceeded".into());
    }
    scratch
        .sorted
        .sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    stats.visible = scratch.sorted.len() as u32;
    stats.culled = stats.alive - stats.visible;
    out.extend(scratch.sorted.drain(..).map(|(_, _, s)| s));
    // Bound all retained birth storage, including culled systems. Eviction only
    // changes the work needed on the next sample, never the resulting particles.
    let current = (e.layer, e.instance);
    let mut remaining =
        motion_effects::MAX_SPRITES.saturating_sub(scratch.births[&current].capacity());
    scratch.births.retain(|key, cache| {
        if *key == current {
            true
        } else if cache.capacity() <= remaining {
            remaining -= cache.capacity();
            true
        } else {
            false
        }
    });
    Ok(stats)
}
