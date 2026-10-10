use motion_core::{EffectInstance, Layer, Project};
use motion_render::Scene;
use motion_render::{
    effect_plan::{scratch_capacity_bytes, PlanBuilder},
    PreviewMode, PreviewPolicy, PreviewTier, Renderer,
};

fn effect(name: &str, size: [f32; 2]) -> EffectInstance {
    let package = motion_effects::builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == name)
        .unwrap();
    let mut e = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        size,
    );
    if name == "directional_blur" {
        e.params.get_mut("p0002").unwrap().track.value[0] = 32.;
    }
    e
}
fn project(name: &str) -> Project {
    let mut p = Project::new(512, 256, 30, 120).unwrap();
    p.background = [0.; 4];
    let mut l = Layer::solid(
        1,
        "large source",
        [2048., 1024.],
        [256., 128., 0.],
        [0.8, 0.5, 0.2, 0.7],
    );
    l.transform.scale.value = [25., 25., 100.];
    l.effects.push(effect(name, l.size));
    p.layers.push(l);
    p.rebuild_plugin_dependencies();
    p
}
#[test]
fn automatic_preview_fits_the_physical_canvas_and_high_is_explicit() {
    let mut policy = PreviewPolicy::default();
    assert_eq!(policy.render_dimensions(3840, 2160, 1080, 608), (1080, 608));
    assert_eq!(policy.render_dimensions(320, 180, 1080, 608), (320, 180));
    policy.set_mode(PreviewMode::Balanced);
    assert_eq!(policy.render_dimensions(3840, 2160, 1080, 608), (720, 405));
    policy.set_mode(PreviewMode::High);
    assert_eq!(
        policy.render_dimensions(3840, 2160, 1080, 608),
        (3840, 2160)
    );
    policy.set_mode(PreviewMode::Auto);
    for _ in 0..60 {
        policy.observe_render(500, Some(20_000.), 0);
    }
    assert_eq!(policy.tier(), PreviewTier::Balanced);
    policy.set_mode(PreviewMode::Auto);
    for _ in 0..60 {
        policy.observe_render(500, None, 50_000);
    }
    assert_eq!(policy.tier(), PreviewTier::Balanced);
    policy.set_mode(PreviewMode::Auto);
    for _ in 0..600 {
        policy.observe_render(500, None, 16_667);
    }
    assert_eq!(policy.tier(), PreviewTier::High);
}
#[test]
fn preview_density_preserves_coordinates_mixed_pool_uvs_and_formal_output() {
    let registry = motion_effects::Registry::new_with_builtins().unwrap();
    let mut builder = PlanBuilder::new(registry).unwrap();
    for name in ["rays", "directional_blur", "glow", "glow_edges", "glint"] {
        let mut p = project(name);
        let mut small = Layer::solid(
            2,
            "unscaled source",
            [1024., 512.],
            [256., 128., 0.],
            [0.1, 0.2, 0.3, 1.],
        );
        small.effects.push(effect(name, small.size));
        p.layers.push(small);
        p.rebuild_plugin_dependencies();
        let mut scene = Scene::new(&p);
        scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
        let full = builder.build(&scene, &[0], 512, 256, true).unwrap();
        let full_scratch = scratch_capacity_bytes(&full.scratch_sizes);
        let full_uniform = full.passes[full.draws[0].pass_end - 1].uniform;
        let bytes = full.buffer_bytes(&scene);
        let mut original = vec![0; bytes];
        full.write(&scene, &mut original).unwrap();
        let preview = builder.build_preview(&scene, &[0], 512, 256).unwrap();
        assert!(
            preview.diagnostics.is_empty(),
            "{name}: {:?}",
            preview.diagnostics
        );
        assert!(scratch_capacity_bytes(&preview.scratch_sizes) < full_scratch);
        let first = &preview.draws[0];
        let out = &preview.passes[first.pass_end - 1];
        assert_eq!(out.uniform.region, full_uniform.region);
        assert_eq!(out.uniform.params, full_uniform.params);
        assert!((out.uniform.output_mode[3] - 0.25).abs() < 1e-6);
        assert_eq!(
            first.words[25],
            out.width as f32 / preview.scratch_sizes[0][0] as f32
        );
        assert!(
            first.words[25] < 1.,
            "the smaller source must use only its part of the pooled texture"
        );
        let after = builder.build(&scene, &[0], 512, 256, true).unwrap();
        let mut restored = vec![0; bytes];
        after.write(&scene, &mut restored).unwrap();
        assert_eq!(
            original, restored,
            "{name}: preview must not change frozen/formal plans"
        );
        p.layers[0].three_d = true;
        let mut spatial = Scene::new(&p);
        spatial.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
        let plan = builder.build_preview(&spatial, &[0], 512, 256).unwrap();
        let draw = plan.draws.iter().find(|d| d.layer == 1).unwrap();
        assert_eq!(plan.passes[draw.pass_end - 1].uniform.output_mode[3], 1.);
    }
}
#[test]
fn projected_density_handles_rotations_reflections_and_anisotropic_transforms() {
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let mut p = project("rays");
    let mut scene = Scene::new(&p);
    for degrees in (0..360).step_by(15) {
        p.layers[0].transform.rotation.value = [0., 0., degrees as f32];
        for scales in [
            [25., 25., 100.],
            [-25., 25., 100.],
            [25., -25., 100.],
            [10., 40., 100.],
            [25., 100., 100.],
        ] {
            p.layers[0].transform.scale.value = scales;
            scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
            let plan = builder.build_preview(&scene, &[0], 512, 256).unwrap();
            let draw = &plan.draws[0];
            let scale = plan.passes[draw.pass_end - 1].uniform.output_mode[3];
            let expected = if scales[1] == 100. {
                1.
            } else if scales[1] == 40. {
                0.5
            } else {
                0.25
            };
            assert_eq!(scale, expected, "rotation {degrees}, scale {scales:?}");
        }
    }
    // Perspective/scene generators cannot use a 2D footprint safely.
    let package = motion_effects::builtin::scene_package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "lens_flare")
        .unwrap();
    p.layers[0].effects = vec![EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        p.layers[0].size,
    )];
    p.layers[0].transform.scale.value = [25., 25., 100.];
    p.layers[0].transform.rotation.value = [0.; 3];
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let plan = builder.build_preview(&scene, &[0], 512, 256).unwrap();
    assert!(!plan.passes.is_empty());
    assert!(plan
        .passes
        .iter()
        .all(|pass| pass.uniform.output_mode[3] == 1.));
}
#[test]
fn gpu_preview_retains_rays_blur_alpha_placement_and_releases_density_changes() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(512, 256).unwrap();
    for name in ["rays", "directional_blur", "glow_edges"] {
        let mut p = project(name);
        let mut scene = Scene::new(&p);
        scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
        let reference = renderer.capture(&scene, &target).unwrap().0;
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        let stats = renderer
            .encode_preview(&scene, &target.view, 512, 256, &mut encoder, None)
            .unwrap();
        renderer.queue.submit(Some(encoder.finish()));
        let out = renderer.read_target(&target).unwrap();
        let mae = out
            .iter()
            .zip(&reference)
            .map(|(a, b)| f64::from(a.abs_diff(*b)))
            .sum::<f64>()
            / out.len() as f64;
        assert!(mae <= 3., "{name}: RGBA MAE {mae}");
        assert!(out.chunks_exact(4).any(|c| c[3] > 128));
        let resident = stats.texture_bytes;
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        assert_eq!(
            resident,
            renderer
                .encode_preview(&scene, &target.view, 512, 256, &mut encoder, None)
                .unwrap()
                .texture_bytes
        );
        renderer.queue.submit(Some(encoder.finish()));
        p.layers[0].transform.scale.value = [50., 50., 100.];
        scene.sample(&p, 30., None, &motion_core::ExpressionEvaluator).unwrap();
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        assert!(
            renderer
                .encode_preview(&scene, &target.view, 512, 256, &mut encoder, None)
                .unwrap()
                .texture_bytes
                > resident
        );
        renderer.queue.submit(Some(encoder.finish()));
        p.layers[0].effects.clear();
        scene.sample(&p, 17., None, &motion_core::ExpressionEvaluator).unwrap();
        assert_eq!(
            renderer.capture(&scene, &target).unwrap().1.texture_bytes,
            renderer.texture_bytes()
        );
        assert!(renderer.gpu_error().is_none());
    }
}
