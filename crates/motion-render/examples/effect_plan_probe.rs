//! CPU planning only, independent of GPU, decoding, frame pacing and output FPS.
use motion_core::{EffectInstance, Layer, Project};
use motion_render::Scene;
use motion_render::effect_plan::PlanBuilder;
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package = motion_effects::builtin::package()?;
    let mut p = Project::new(1920, 1080, 30, 120)?;
    let mut l = Layer::solid(
        1,
        "static video geometry",
        [1920., 1080.],
        [960., 540., 0.],
        [1.; 4],
    );
    for (i, name) in ["tint", "glow", "rays", "directional_blur", "curves"]
        .iter()
        .enumerate()
    {
        let d = package
            .manifest
            .effects
            .iter()
            .find(|e| &e.id == name)
            .unwrap();
        l.effects.push(EffectInstance::new(
            i as u64 + 1,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            d,
            l.size,
        ));
    }
    p.layers.push(l);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator)?;
    let mut b = PlanBuilder::new(motion_effects::Registry::new_with_builtins()?)?;
    let mut times = Vec::new();
    for memo in [false, true] {
        b.invalidate_preview_plan();
        b.build_preview(&scene, &[0], 960, 540)?;
        let started = Instant::now();
        for i in 0..2000 {
            scene.frame = i as f64;
            for effect in &mut scene.effects {
                effect.local_frame = i as f64;
            }
            if !memo {
                b.invalidate_preview_plan();
            }
            let plan = b.build_preview(&scene, &[0], 960, 540)?;
            if !plan.diagnostics.is_empty() {
                return Err(plan.diagnostics.join("; ").into());
            }
            std::hint::black_box(plan);
        }
        times.push(started.elapsed().as_secs_f64() * 1_000_000. / 2000.);
    }
    println!(
        "{}",
        serde_json::json!({"cpuOnly":true,"iterations":2000,"withoutMemoUs":times[0],"withMemoUs":times[1],"speedup":times[0]/times[1],"planBuilds":b.preview_plan_builds,"cacheHits":b.preview_cache_hits})
    );
    Ok(())
}
