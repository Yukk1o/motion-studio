use motion_core::{Content, EffectInstance, Layer, Project, Scene};
use motion_render::Renderer;
use serde_json::Value;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("artifacts/ae-reference/18.0.1".into()),
    );
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(root.join("cases.json"))?)?;
    let package = motion_effects::builtin::package()?;
    let mut renderer = pollster::block_on(Renderer::headless())?;
    let mut records = vec![];
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let width = case["width"].as_u64().unwrap_or(256) as u32;
        let height = case["height"].as_u64().unwrap_or(256) as u32;
        let pixels = image::open(root.join(case["input"].as_str().unwrap()))?.into_rgba8();
        let source_size = [pixels.width() as f32, pixels.height() as f32];
        let target = renderer.capture_target(width, height)?;
        let def = package
            .manifest
            .effects
            .iter()
            .find(|e| e.id == case["effect"])
            .unwrap();
        let mut p = Project::new(width, height, 30, 180)?;
        p.background = [0.0; 4];
        let mut layer = Layer::solid(
            1,
            "Reference",
            source_size,
            [width as f32 * 0.5, height as f32 * 0.5, 0.0],
            [1.0; 4],
        );
        layer.content = Content::Image { asset: 1 };
        p.assets.push(motion_core::Asset {
            id: 1,
            path: format!("assets/{}", case["input"].as_str().unwrap()),
            width: pixels.width(),
            height: pixels.height(),
        });
        let mut effect = EffectInstance::new(
            1,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            def,
            source_size,
        );
        for (key, value) in case["properties"].as_object().unwrap() {
            if let Some(param) = def
                .params
                .iter()
                .find(|param| param.reference_match_name == *key)
            {
                let dst = &mut effect.params.get_mut(&param.id).unwrap().track.value;
                if let Some(v) = value.as_f64() {
                    dst[0] = v as f32;
                } else if let Some(v) = value.as_array() {
                    for (i, x) in v.iter().enumerate().take(4) {
                        dst[i] = x.as_f64().unwrap() as f32;
                    }
                }
            }
        }
        layer.effects.push(effect);
        p.layers.push(layer);
        p.rebuild_plugin_dependencies();
        p.validate()?;
        renderer.upload_image(1, pixels.width(), pixels.height(), pixels.as_raw())?;
        let mut scene = Scene::new(&p);
        scene.sample(&p, case["frame"].as_f64().unwrap_or(0.0), None)?;
        match renderer.capture(&scene, &target) {
            Ok((rgba, stats)) => {
                image::save_buffer(
                    root.join(format!("motion-{id}.png")),
                    &rgba,
                    width,
                    height,
                    image::ColorType::Rgba8,
                )?;
                records.push(serde_json::json!({"id":id,"draws":stats.draw_calls,"texture_bytes":stats.texture_bytes}));
            }
            Err(e) => {
                records.push(serde_json::json!({"id":id,"error":e.to_string()}));
            }
        }
    }
    let errors = records.iter().filter(|r| r.get("error").is_some()).count();
    fs::write(
        root.join("motion-renders.json"),
        serde_json::to_vec_pretty(&records)?,
    )?;
    println!(
        "{} cases, {errors} failures; {}",
        records.len(),
        renderer.adapter_info.name
    );
    if errors > 0 {
        return Err("effect GPU cases failed; see motion-renders.json".into());
    }
    Ok(())
}
