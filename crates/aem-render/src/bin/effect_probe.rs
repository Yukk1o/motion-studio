use aem_core::{Content, EffectInstance, Layer, Project, Scene};
use aem_render::Renderer;
use serde_json::Value;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("artifacts/ae-reference/18.0.1".into()),
    );
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(root.join("cases.json"))?)?;
    let package = aem_effects::builtin::package()?;
    let mut renderer = pollster::block_on(Renderer::headless())?;
    let target = renderer.capture_target(256, 256)?;
    let mut records = vec![];
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let def = package
            .manifest
            .effects
            .iter()
            .find(|e| e.id == case["effect"])
            .unwrap();
        let mut p = Project::new(256, 256, 30, 180)?;
        p.background = [0.0; 4];
        let mut layer = Layer::solid(1, "Reference", [256.0; 2], [128.0, 128.0, 0.0], [1.0; 4]);
        layer.content = Content::Image { asset: 1 };
        p.assets.push(aem_core::Asset {
            id: 1,
            path: format!("assets/{}", case["input"].as_str().unwrap()),
            width: 256,
            height: 256,
        });
        let mut effect = EffectInstance::new(
            1,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            def,
            [256.0; 2],
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
        let pixels = image::open(root.join(case["input"].as_str().unwrap()))?.into_rgba8();
        renderer.upload_image(1, 256, 256, pixels.as_raw())?;
        let mut scene = Scene::new(&p);
        scene.sample(&p, case["frame"].as_f64().unwrap_or(0.0), None)?;
        match renderer.capture(&scene, &target) {
            Ok((rgba, stats)) => {
                image::save_buffer(
                    root.join(format!("motion-{id}.png")),
                    &rgba,
                    256,
                    256,
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
