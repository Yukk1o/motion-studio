use motion_core::{EffectInstance, Layer, Project, VideoSample};
use motion_render::Scene;
use motion_render::effect_plan::PlanBuilder;

fn builder() -> PlanBuilder {
    PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap()
}
fn project() -> Project {
    let package = motion_effects::builtin::package().unwrap();
    let d = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "shake")
        .unwrap();
    let mut p = Project::new(64, 64, 30, 120).unwrap();
    let mut l = Layer::solid(1, "video", [64.; 2], [32., 32., 0.], [1.; 4]);
    l.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        d,
        l.size,
    ));
    p.layers.push(l);
    p
}
fn bytes(builder: &mut PlanBuilder, scene: &Scene, width: u32) -> Vec<u8> {
    let plan = builder.build_preview(scene, &[0], width, width).unwrap();
    let mut out = vec![0; plan.buffer_bytes(scene)];
    plan.write(scene, &mut out).unwrap();
    out
}
#[test]
fn static_video_plans_reuse_geometry_but_keep_local_shader_time_on_random_seeks() {
    let p = project();
    let mut scene = Scene::new(&p);
    let mut cached = builder();
    for f in [0., 1., 31., 79., 2., 0.] {
        scene.sample(&p, f, None, &motion_core::ExpressionEvaluator).unwrap();
        scene.layers[0].video = Some(VideoSample {
            asset: 7,
            source_time_us: (f * 33_333.) as u64,
        });
        assert_eq!(
            bytes(&mut cached, &scene, 64),
            bytes(&mut builder(), &scene, 64),
            "frame {f}"
        );
    }
    assert_eq!(cached.preview_plan_builds, 1);
    assert_eq!(cached.preview_cache_hits, 5);
}
#[test]
fn signed_zero_parameters_do_not_reuse_a_different_gpu_uniform_payload() {
    let p = project();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let index = scene.effects[0]
        .param_ids
        .iter()
        .position(|p| p == "translation")
        .unwrap();
    scene.effects[0].values[index][0] = 0.;
    let mut cached = builder();
    let positive = bytes(&mut cached, &scene, 64);
    scene.effects[0].values[index][0] = -0.;
    let negative = bytes(&mut cached, &scene, 64);
    assert_ne!(positive, negative);
    assert_eq!(negative, bytes(&mut builder(), &scene, 64));
    assert_eq!(cached.preview_plan_builds, 2);
}
#[test]
fn parameters_geometry_quality_resources_and_strict_export_invalidate_cached_plan() {
    let mut p = project();
    let mut scene = Scene::new(&p);
    let mut cached = builder();
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    bytes(&mut cached, &scene, 64);
    p.layers[0].effects[0]
        .params
        .get_mut("translation")
        .unwrap()
        .track
        .value[0] = 24.;
    scene.sample(&p, 1., None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(
        bytes(&mut cached, &scene, 64),
        bytes(&mut builder(), &scene, 64)
    );
    p.layers[0].transform.rotation.value[2] = 20.;
    scene.sample(&p, 2., None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(
        bytes(&mut cached, &scene, 64),
        bytes(&mut builder(), &scene, 64)
    );
    assert_eq!(
        bytes(&mut cached, &scene, 32),
        bytes(&mut builder(), &scene, 32)
    );
    assert_eq!(cached.preview_plan_builds, 4);
    let pass = cached
        .frame
        .passes
        .iter()
        .find(|p| p.program >= 3)
        .unwrap()
        .program;
    cached
        .program_errors
        .insert(pass, "test compile failure".into());
    bytes(&mut cached, &scene, 32);
    assert!(!cached.frame.diagnostics.is_empty());
    assert!(cached.build(&scene, &[0], 64, 64, true).is_err());
    cached.program_errors.clear();
    bytes(&mut cached, &scene, 32);
    assert!(cached.frame.diagnostics.is_empty());
    let before = cached.preview_plan_builds;
    cached.build(&scene, &[0], 64, 64, true).unwrap();
    bytes(&mut cached, &scene, 32);
    assert_eq!(cached.preview_plan_builds, before + 1);
    let package = motion_effects::builtin::package().unwrap();
    cached.registry.disabled.insert((
        package.manifest.id.clone(),
        package.manifest.version.clone(),
        package.hash.clone(),
    ));
    bytes(&mut cached, &scene, 32);
    assert!(!cached.frame.diagnostics.is_empty());
}

#[test]
fn reused_preview_plan_uploads_changed_curve_lut_and_matches_fresh_gpu_output() {
    use motion_render::Renderer;
    let package = motion_effects::builtin::package().unwrap();
    let d = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "curves")
        .unwrap();
    let mut p = Project::new(64, 64, 30, 60).unwrap();
    let mut l = Layer::solid(1, "curves", [64.; 2], [32., 32., 0.], [1., 0.2, 0.1, 0.5]);
    l.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        d,
        l.size,
    ));
    p.layers.push(l);
    let mut scene = Scene::new(&p);
    let mut cached = pollster::block_on(Renderer::headless()).unwrap();
    let mut fresh = pollster::block_on(Renderer::headless()).unwrap();
    let target = cached.capture_target(64, 64).unwrap();
    let reference = fresh.capture_target(64, 64).unwrap();
    let mut outputs = Vec::new();
    for i in 0..2 {
        if i == 1 {
            p.layers[0].effects[0]
                .params
                .get_mut("p0001")
                .unwrap()
                .curve
                .as_mut()
                .unwrap()
                .value
                .channels[1] = vec![[0., 0.], [1., 0.]];
        }
        scene.sample(&p, i as f64, None, &motion_core::ExpressionEvaluator).unwrap();
        let mut encoder = cached.device.create_command_encoder(&Default::default());
        cached
            .encode_preview(&scene, &target.view, 64, 64, &mut encoder, None)
            .unwrap();
        cached.queue.submit(Some(encoder.finish()));
        let out = cached.read_target(&target).unwrap();
        assert_eq!(out, fresh.capture(&scene, &reference).unwrap().0);
        outputs.push(out);
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_eq!(cached.effect_plan_cache_hits(), 1);
}
