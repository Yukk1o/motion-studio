//! Reproducible six-second moving-emitter demonstration and per-second capture timing.
use motion_core::{Ease, EffectInstance, Keyframe, Layer, Project, Scene};
use motion_render::Renderer;
use std::{fs, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("artifacts/particle-emitter".into()),
    );
    fs::create_dir_all(&out)?;
    let package = motion_effects::builtin::particle_package()?;
    let mut p = Project::new(960, 540, 30, 180)?;
    p.background = [0.008, 0.015, 0.03, 1.];
    let mut l = Layer::solid(1, "运动发射器", [960., 540.], [480., 270., 0.], [0.; 4]);
    for frame in (0..=180).step_by(3) {
        let t = frame as f32 / 30.;
        let a = t * std::f32::consts::TAU / 4.;
        l.transform.position.keys.push(Keyframe {
            frame,
            value: [480. + 300. * a.sin(), 270. + 155. * (a * 2.).sin(), 0.],
            ease: Ease::Linear,
            curve: None,
            spatial: None,
        });
    }
    let mut e = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        &package.manifest.effects[0],
        [960., 540.],
    );
    for (id, v) in [
        ("rate", 1800.),
        ("lifetime", 1.6),
        ("speed", 20.),
        ("spread", 30.),
        ("inherit_velocity", 0.35),
        ("drag", 1.8),
        ("size", 20.),
        ("end_size", 1.),
    ] {
        e.params.get_mut(id).unwrap().track.value[0] = v;
    }
    e.seed = 812;
    let sprite = std::env::args().any(|a| a == "--sprite");
    if sprite {
        let sprite_root = out.join("assets");
        fs::create_dir_all(&sprite_root)?;
        let points: Vec<_> = (0..10)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.;
                let r = if i % 2 == 0 { 29. } else { 12. };
                (32. + r * a.cos(), 32. + r * a.sin())
            })
            .collect();
        let mut png = image::RgbaImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let mut samples = 0;
                for sy in 0..2 {
                    for sx in 0..2 {
                        let px = x as f32 + (sx as f32 + 0.5) / 2.;
                        let py = y as f32 + (sy as f32 + 0.5) / 2.;
                        let mut inside = false;
                        for i in 0..10 {
                            let (ax, ay) = points[i];
                            let (bx, by) = points[(i + 1) % 10];
                            if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax
                            {
                                inside = !inside;
                            }
                        }
                        if inside {
                            samples += 1;
                        }
                    }
                }
                png.put_pixel(
                    x,
                    y,
                    image::Rgba([255, 255, 255, (samples * 255 / 4) as u8]),
                );
            }
        }
        png.save(sprite_root.join("sprite.png"))?;
        p.assets.push(motion_core::Asset {
            id: 1,
            path: "assets/sprite.png".into(),
            width: 64,
            height: 64,
        });
        e.scene.as_mut().unwrap().sprite_asset = Some(1);
        e.params.get_mut("rate").unwrap().track.value[0] = 260.;
        e.params.get_mut("size").unwrap().track.value[0] = 32.;
    }
    l.effects.push(e);
    p.layers.push(l);
    p.rebuild_plugin_dependencies();
    p.validate()?;
    fs::write(
        out.join("moving-emitter-project.json"),
        serde_json::to_vec_pretty(&p)?,
    )?;
    let mut r = pollster::block_on(Renderer::headless())?;
    if sprite {
        let png = image::open(out.join("assets/sprite.png"))?.into_rgba8();
        r.upload_image(1, 64, 64, png.as_raw())?;
    }
    let target = r.capture_target(960, 540)?;
    let mut s = Scene::new(&p);
    let mut report = Vec::new();
    let mut elapsed = 0.;
    let mut prepare = 0.;
    for frame in 0..180 {
        let start = Instant::now();
        s.sample(&p, frame as f64, None)?;
        let (pixels, stats) = r.capture(&s, &target)?;
        elapsed += start.elapsed().as_secs_f64();
        prepare += stats.cpu_prepare_us as f64;
        image::save_buffer(
            out.join(format!("frame-{frame:04}.png")),
            &pixels,
            960,
            540,
            image::ColorType::Rgba8,
        )?;
        if frame % 30 == 29 {
            report.push(serde_json::json!({"second":frame/30,"frames":30,"render_capture_fps":30./elapsed,"mean_cpu_prepare_us":prepare/30.}));
            elapsed = 0.;
            prepare = 0.;
        }
    }
    let report = serde_json::json!({"adapter":r.adapter_info.name,"backend":format!("{:?}",r.adapter_info.backend),"width":960,"height":540,"fps":30,"seconds":6,"package_hash":package.hash,
        "timing_scope":"Desktop synchronous scene sampling + wgpu render + RGBA readback; PNG encoding and video encoding excluded. Not phone playback FPS.","per_second":report});
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
