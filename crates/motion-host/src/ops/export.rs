//! Frozen/export render plans, captures and packed geometry output.
use crate::session::Result;
use motion_render::Scene;
use motion_render::Renderer;
use serde_json::json;

pub fn render_plan_info(id: i64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        s.observing = false;
        s.sample()?;
        let p = s.engine.project();
        for id in p.reachable_compositions().map_err(|e| e.to_string())? {
            s.effects
                .preflight_project(&p.composition(&id).map_err(|e| e.to_string())?)?;
        }
        s.effects
            .synchronize_scene_alpha(&s.scene, p, &s.root)?;
        let assets = std::iter::once(0)
            .chain(p.assets.iter().map(|a| a.id))
            .collect::<Vec<_>>();
        for id in p.reachable_compositions().map_err(|e| e.to_string())? {
            let mut node = p.composition(&id).map_err(|e| e.to_string())?;
            for e in &mut node.expressions {
                e.enabled = false;
            }
            for c in &mut node.compositions {
                for e in &mut c.expressions {
                    e.enabled = false;
                }
            }
            let mut scene = Scene::new(&node);
            scene.sample(&node, 0., None, &motion_core::ExpressionEvaluator).map_err(|e| e.to_string())?;
            s.effects.synchronize(&scene)?;
        }
        s.effects
            .build(&s.scene, &assets, p.width, p.height, true)?;
        let programs = s
            .effects
            .programs
            .iter()
            .map(|program| {
                json!({"key":program.key,"wgsl":program.shader.wgsl,"glsl":program.shader.glsl,
                    "sprite":program.shader.sprite,"additive":program.shader.additive,
                    "resources":program.resources.iter().map(|path|{
                        let bytes=&program.package.as_ref().unwrap().files[path];
                        let dimensions=image::load_from_memory(bytes).map(|v|(v.width(),v.height())).unwrap_or((0,0));
                        json!({"path":path,"width":dimensions.0,"height":dimensions.1})
                    }).collect::<Vec<_>>()})
            })
            .collect::<Vec<_>>();
        let mask_programs = motion_render::mask_plan::shaders()?
            .iter()
            .enumerate()
            .map(|(index, shader)| {
                json!({"key":if index == 0 {"sdk-mask-gaussian"} else {"sdk-mask-combine"},
                    "wgsl":shader.wgsl,"glsl":shader.glsl})
            })
            .collect::<Vec<_>>();
        let count = p
            .layers
            .iter()
            .map(|l| l.effects.iter().filter(|e| e.enabled).count())
            .sum::<usize>();
        let passes = count * 10 + p.layers.len() * 2;
        let buffer_bytes = motion_render::effect_plan::HEADER_BYTES
            + p.layers.len() * (128 + 20)
            + passes * (40 + motion_effects::shader::UNIFORM_BYTES)
            + count * 1024
            + motion_effects::MAX_SPRITES * 48
            + 8192 * 12
            + 65536 * 20
            + motion_core::MAX_LAYERS * 28
            + 262144 * 24
            + motion_core::MAX_LAYERS * motion_core::masks::MAX_MASKS * motion_render::mask_plan::RECORD_BYTES
            + 262144 * 24;
        let mut info=json!({"version":motion_render::effect_plan::PLAN_VERSION,
            "scratchBudgetBytes":s.effects.scratch_budget(),
            "headerBytes":motion_render::effect_plan::HEADER_BYTES,
            "composition_bundle_version":1,"composition_bundle_buffer_hint":131072,
            "has_video":!p.video_assets.is_empty(),
            "has_audio":p.audio_voices().is_ok_and(|v|!v.is_empty()),
            "programs":programs,"maskPrograms":mask_programs,
            "maskBytes":motion_render::mask_plan::RECORD_BYTES,"maskVertexBytes":24,
            "maskSourceToken":motion_render::mask_plan::SOURCE_TOKEN,
            "bufferBytes":buffer_bytes,"uniformBytes":motion_effects::shader::UNIFORM_BYTES,
            "passBytes":40,"spriteBytes":48,"assetBytes":4,
            "declaredAssetBytes":4+p.assets.iter().map(|a|u64::from(a.width)*u64::from(a.height)*4).sum::<u64>(),
            "imageResources":{"version":1,"demandLoading":true,
                "previewMaxEdge":motion_render::image_resources::MAX_PREVIEW_EDGE,
                "fullResolutionExport":true,"directBuffer":true}});
        let needed=p.layers.iter().chain(p.compositions.iter().flat_map(|c|&c.layers))
            .any(|l|l.track_matte.is_some()||l.blend!=motion_core::compositing::LayerBlend::default());
        if needed {info["compositing"]=json!({"version":1,"planes":motion_render::compositing_plan::plane_programs()?,"blend":motion_render::compositing_plan::shader()?.glsl});}
        Ok(info)
    })
}

/// Serialize the versioned effect frame plan into a caller-owned buffer.
///
/// Returns the number of bytes written. A nested composition must use
/// [`crate::ops::composition::frame_bundle`] instead.
pub fn sample_render_plan_into(id: i64, frame: i32, out: &mut [u8]) -> Result<usize> {
    crate::session::with_session(id, |s| sample_render_plan_into_inner(s, frame, out))
}

pub fn sample_render_plan_into_inner(
    s: &mut crate::session::Session,
    frame: i32,
    out: &mut [u8],
) -> Result<usize> {
    {
        s.frame = f64::from(frame);
        s.sample()?;
        let p = s.engine.project();
        if !s.scene.nested.is_empty() {
            return Err("nested compositions require CompositionBridge.sampleFrameBundleInto".into());
        }
        let assets = std::iter::once(0)
            .chain(p.assets.iter().map(|a| a.id))
            .collect::<Vec<_>>();
        s.effects
            .synchronize_scene_alpha(&s.scene, p, &s.root)?;
        let result = (|| {
            let plan = s
                .effects
                .build(&s.scene, &assets, p.width, p.height, true)?;
            plan.write(&s.scene, out).map_err(|e| e.to_string())
        })();
        if let Err(error) = &result {
            s.last_error = Some(error.clone());
        }
        result
    }
}

/// Write the current frame to `exports/frame-NNNNNN.png`.
pub fn capture(id: i64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        let p = s.engine.project();
        for id in p.reachable_compositions().map_err(|e| e.to_string())? {
            s.effects
                .preflight_project(&p.composition(&id).map_err(|e| e.to_string())?)?;
        }
        s.effects
            .synchronize_scene_alpha(&s.scene, p, &s.root)?;
        let mut scene = Scene::new(p);
        scene.sample(p, s.frame, None, &motion_core::ExpressionEvaluator).map_err(|e| e.to_string())?;
        let mut temporary = None;
        if s.graphics.is_none() {
            temporary = Some(pollster::block_on(Renderer::headless()).map_err(|e| e.to_string())?);
        }
        let renderer = if let Some(g) = &mut s.graphics {
            &mut g.renderer
        } else {
            temporary.as_mut().unwrap()
        };
        renderer.set_effect_registry(s.effects.registry.clone());
        renderer
            .set_scratch_budget(s.effects.scratch_budget())
            .map_err(|e| e.to_string())?;
        renderer
            .configure_assets(p, &s.root)
            .map_err(|e| e.to_string())?;
        renderer.retain_video_instances(&scene);
        renderer
            .prepare_scene_assets(&scene, motion_render::image_resources::Resolution::Full, false)
            .map_err(|e| e.to_string())?;
        let target = renderer
            .capture_target(p.width, p.height)
            .map_err(|e| e.to_string())?;
        let frames = s
            .video_frames
            .prepare_scene_exact(p, &s.root, &scene)?
            .ok_or("video capture pending; request frames and retry")?;
        renderer.retain_video_instances(&scene);
        for (object, image) in frames {
            let source = scene
                .video_layers()
                .into_iter()
                .find(|l| l.id == object)
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .asset;
            image.upload(renderer, object, source)?;
        }
        let (pixels, _) = renderer
            .capture(&scene, &target)
            .map_err(|e| e.to_string())?;
        let dir = s.root.join("exports");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("frame-{:06}.png", s.frame.floor() as u32));
        image::save_buffer(&file, &pixels, p.width, p.height, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        Ok(json!({"path":file.to_string_lossy(),"width":p.width,"height":p.height,"frame":s.frame.floor()}))
    })
}

pub fn pack(id: i64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        let output = s.root.join("exports/project.msproj");
        std::fs::create_dir_all(output.parent().unwrap())
            .map_err(|e| e.to_string())?;
        motion_core::storage::export_package(&s.root, s.engine.project(), &output)
            .map_err(|e| e.to_string())?;
        Ok(json!({"path":output.to_string_lossy()}))
    })
}

/// Packed plane parameters for hosts that render without the wgpu pipeline.
///
/// Effects, vector/adjustment sources, video layers and intersecting planes are
/// rejected: those require the versioned render plan or the geometry bundle.
pub fn sample_into(id: i64, frame: i32, out: &mut [u8]) -> Result<usize> {
    crate::session::with_session(id, |s| sample_into_inner(s, frame, out))
}

pub fn sample_into_inner(
    s: &mut crate::session::Session,
    frame: i32,
    out: &mut [u8],
) -> Result<usize> {
    {
        if s.engine
            .project()
            .layers
            .iter()
            .any(|l| l.effects.iter().any(|e| e.enabled))
        {
            return Err("effects require the versioned render plan".into());
        }
        s.frame = f64::from(frame);
        s.scene
            .sample(s.engine.project(), s.frame, None, &motion_core::ExpressionEvaluator)
            .map_err(|e| e.to_string())?;
        if s.scene
            .layers
            .iter()
            .any(|l| l.adjustment || l.vector.is_some())
        {
            return Err("vector and adjustment sources require render plan version 4".into());
        }
        s.geometry.prepare(&s.scene).map_err(|e| e.to_string())?;
        if s.scene.layers.iter().any(|l| l.video.is_some()) {
            return Err("video export requires dynamic frame reads and GeometryBridge".into());
        }
        if s.geometry.batches.iter().any(|b| b.vertices.len() != 6)
            || s.geometry.vertices.len() > s.scene.layers.len() * 6
        {
            return Err("intersecting layers require GeometryBridge.sampleGeometryInto".into());
        }
        let words = s.geometry.batches.len() * 32;
        if out.len() < words * 4 {
            return Err("draw parameter buffer is too small".into());
        }
        for (i, batch) in s.geometry.batches.iter().enumerate() {
            let layer = &s.scene.layers[batch.layer];
            let mut data = [0.0f32; 32];
            data[..16].copy_from_slice(&(layer.view_projection * layer.model).to_cols_array());
            data[16..20].copy_from_slice(&layer.color);
            for c in &mut data[16..19] {
                *c = if *c <= 0.04045 {
                    *c / 12.92
                } else {
                    ((*c + 0.055) / 1.055).powf(2.4)
                };
            }
            data[20] = layer.size[0];
            data[21] = layer.size[1];
            data[22] = layer.opacity;
            data[24] = layer.asset.map_or(0.0, |id| {
                s.engine
                    .project()
                    .assets
                    .iter()
                    .position(|a| a.id == id)
                    .map_or(0.0, |index| index as f32 + 1.0)
            });
            out[i * 128..(i + 1) * 128].copy_from_slice(bytemuck::cast_slice(&data));
        }
        Ok(s.geometry.batches.len())
    }
}

/// Full-resolution RGBA bytes for one project asset, premultiplied.
pub fn asset_pixels(id: i64, asset: i64) -> Result<Vec<u8>> {
    crate::session::with_session(id, |s| {
        let a = s
            .engine
            .project()
            .assets
            .iter()
            .find(|a| a.id == asset as u64)
            .ok_or("asset not found")?;
        let reader = image::ImageReader::open(s.root.join(&a.path)).map_err(|e| e.to_string())?;
        let dimensions = reader.into_dimensions().map_err(|e| e.to_string())?;
        if dimensions != (a.width, a.height)
            || u64::from(a.width) * u64::from(a.height) * 4 > 128 * 1024 * 1024
        {
            return Err("invalid asset dimensions or memory budget".into());
        }
        let mut pixels = image::ImageReader::open(s.root.join(&a.path))
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?
            .into_rgba8()
            .into_raw();
        motion_render::premultiply_pixels(&mut pixels);
        Ok(pixels)
    })
}