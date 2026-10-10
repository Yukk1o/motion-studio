//! Selection uses the same affine mapping as supported image effects, leaving
//! stored transforms and the logical anchor unchanged.
use motion_model::SampledEffect;
use crate::{DrawLayer, Scene};
use motion_effects::Registry;
use glam::{Mat4, Vec2, Vec3};

const SHAKE: &[u8] = include_bytes!("../../motion-effects/library/shaders/shake.wgsl");

fn hash(mut v: u32) -> u32 {
    v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
    v = (v ^ (v >> 15)).wrapping_mul(0x846ca68b);
    v ^ (v >> 16)
}
fn noise(time: f32, seed: u32, salt: u32) -> f32 {
    let epoch = time.floor() as i32;
    let f = time - time.floor();
    let weight = f * f * (3. - 2. * f);
    // WGSL bitcasts clock.w, whose ABI stores the seed as a numeric float.
    let sample = |epoch: i32| {
        (hash((epoch as u32).wrapping_mul(0xc2b2ae35) ^ (seed as f32).to_bits() ^ salt) >> 8) as f32
            / 16777216.
    };
    (sample(epoch) * (1. - weight) + sample(epoch.wrapping_add(1)) * weight) * 2. - 1.
}
fn value(effect: &SampledEffect, id: &str) -> Option<[f32; 4]> {
    effect
        .param_ids
        .iter()
        .position(|p| p == id)
        .map(|i| effect.values[i])
}
fn shake(effect: &SampledEffect, fps: u32, registry: &Registry) -> Option<Mat4> {
    if !effect.enabled || effect.effect != "shake" {
        return None;
    }
    let package = registry
        .resolve(&effect.plugin, &effect.version, &effect.hash)
        .ok()?;
    // An ID alone is insufficient: custom or historical implementations may
    // have different geometry. Package bytes and shader behavior stay pinned.
    if package.files.get("shaders/shake.wgsl")?.as_slice() != SHAKE {
        return None;
    }
    let amount = value(effect, "amount")?[0];
    let opacity = value(effect, "effect_opacity").map_or(100., |v| v[0]);
    if amount == 0. || opacity <= 0. {
        return None;
    }
    let translation = value(effect, "translation")?;
    let time = (effect.local_frame / f64::from(fps)) as f32 * value(effect, "frequency")?[0];
    let offset = Vec3::new(
        noise(time, effect.seed, 13) * translation[0] * amount,
        noise(time, effect.seed, 37) * translation[1] * amount,
        0.,
    );
    let angle =
        (noise(time, effect.seed, 59) * value(effect, "rotation")?[0] * amount).to_radians();
    let zoom = 1. + noise(time, effect.seed, 71) * value(effect, "zoom")?[0] * amount;
    let c = value(effect, "center")?;
    let center = Vec3::new(c[0], c[1], 0.);
    Some(
        Mat4::from_translation(center + offset)
            * Mat4::from_rotation_z(angle)
            * Mat4::from_scale(Vec3::new(zoom, zoom, 1.))
            * Mat4::from_translation(-center),
    )
}

fn hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let half = |points: &[Vec2]| {
        let mut result: Vec<Vec2> = Vec::new();
        for &point in points {
            while result.len() > 1 {
                let n = result.len();
                if (result[n - 1] - result[n - 2]).perp_dot(point - result[n - 1]) > 0. {
                    break;
                }
                result.pop();
            }
            result.push(point);
        }
        result.pop();
        result
    };
    let mut result = half(&points);
    points.reverse();
    result.extend(half(&points));
    result
}
fn pixel_polygon(layer: &DrawLayer, scene: &Scene, registry: &Registry) -> Vec<Vec2> {
    let rect = layer.source_rect;
    let mut points = vec![
        Vec2::new(rect[0], rect[1]),
        Vec2::new(rect[0] + rect[2], rect[1]),
        Vec2::new(rect[0] + rect[2], rect[1] + rect[3]),
        Vec2::new(rect[0], rect[1] + rect[3]),
    ];
    for effect in scene.effects.iter().filter(|e| e.layer == layer.id) {
        if let Some(transform) = shake(effect, scene.fps, registry) {
            let partial = value(effect, "effect_opacity").is_some_and(|v| v[0] < 100.);
            let next: Vec<_> = points
                .iter()
                .map(|v| transform.transform_point3(v.extend(0.)).truncate())
                .collect();
            if partial {
                points.extend(next);
                points = hull(points);
            } else {
                points = next;
            }
            // UI outlines may be conservative, but never omit visible output.
            if points.len() > 256 {
                let min = points
                    .iter()
                    .fold(Vec2::splat(f32::INFINITY), |a, p| a.min(*p));
                let max = points
                    .iter()
                    .fold(Vec2::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
                points = vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
            }
        }
    }
    points
}

pub fn picking_layer(layer: &DrawLayer, scene: &Scene, registry: &Registry) -> DrawLayer {
    let mut transform = Mat4::IDENTITY;
    let mut partial = false;
    for effect in scene.effects.iter().filter(|e| e.layer == layer.id) {
        if let Some(next) = shake(effect, scene.fps, registry) {
            transform = next * transform;
            partial |= value(effect, "effect_opacity").is_some_and(|v| v[0] < 100.);
        }
    }
    let mut result = layer.clone();
    if partial {
        let points = pixel_polygon(layer, scene, registry);
        let min = points
            .iter()
            .fold(Vec2::splat(f32::INFINITY), |a, p| a.min(*p));
        let max = points
            .iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
        let center = (min + max) * 0.5;
        result.size = (max - min).to_array();
        result.model = layer.model
            * Mat4::from_translation(Vec3::new(
                center.x - layer.source_size[0] / 2.,
                layer.source_size[1] / 2. - center.y,
                0.,
            ));
    } else {
        let pixels = Mat4::from_translation(Vec3::new(
            -layer.source_size[0] / 2.,
            layer.source_size[1] / 2.,
            0.,
        )) * Mat4::from_scale(Vec3::new(1., -1., 1.));
        result.model = layer.model * pixels * transform * pixels.inverse();
    }
    result
}

pub fn polygon(layer: &DrawLayer, scene: &Scene, registry: &Registry) -> Option<Vec<[f32; 2]>> {
    let mut points = Vec::new();
    let matrix = layer.view_projection * layer.model;
    for point in pixel_polygon(layer, scene, registry) {
        let c = matrix
            * glam::Vec4::new(
                point.x - layer.source_size[0] / 2.,
                layer.source_size[1] / 2. - point.y,
                0.,
                1.,
            );
        if !c.is_finite() || c.w <= 0. {
            return None;
        }
        points.push(Vec2::new(
            (c.x / c.w * 0.5 + 0.5) * scene.width as f32,
            (0.5 - c.y / c.w * 0.5) * scene.height as f32,
        ));
    }
    if points.len() == 4 {
        return Some(points.into_iter().map(|v| v.to_array()).collect());
    }
    Some(hull(points).into_iter().map(|v| v.to_array()).collect())
}
