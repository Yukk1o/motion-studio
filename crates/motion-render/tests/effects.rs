use motion_core::{EffectInstance, Layer, Project};
use motion_render::Scene;
use motion_render::{effect_plan::PlanBuilder, Renderer};
fn instance(name: &str, id: u64) -> EffectInstance {
    let p = motion_effects::builtin::package().unwrap();
    let d = p.manifest.effects.iter().find(|d| d.id == name).unwrap();
    EffectInstance::new(
        id,
        &p.manifest.id,
        &p.manifest.version,
        &p.hash,
        d,
        [64.0; 2],
    )
}
fn fixture() -> Project {
    let mut p = Project::new(64, 64, 30, 60).unwrap();
    p.background = [0.0; 4];
    p.layers.push(Layer::solid(
        1,
        "test",
        [64.0; 2],
        [32.0, 32.0, 0.0],
        [1.0, 0.0, 0.0, 0.5],
    ));
    p
}
#[test]
fn gpu_chain_preserves_alpha_changes_order_and_bypasses_missing_plugins() {
    let mut p = fixture();
    p.layers[0].effects.push(instance("tint", 1));
    p.rebuild_plugin_dependencies();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(64, 64).unwrap();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let (pixels, stats) = renderer.capture(&scene, &target).unwrap();
    let pixel = &pixels[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4];
    assert!(pixel[0].abs_diff(76) <= 3 && pixel[0] == pixel[1] && pixel[1] == pixel[2]);
    assert!(pixel[3].abs_diff(128) <= 1);
    assert!(stats.draw_calls >= 5);
    let mut curves = instance("curves", 2);
    curves
        .params
        .get_mut("p0001")
        .unwrap()
        .curve
        .as_mut()
        .unwrap()
        .value
        .channels[1] = vec![[0.0, 0.0], [1.0, 0.0]];
    p.layers[0].effects.push(curves);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let (a, stats) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(stats.parameter_resource_upload_bytes, 1024);
    let (_, stable_stats) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(stable_stats.parameter_resource_upload_bytes, 0);
    p.layers[0].effects.swap(0, 1);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let (b, _) = renderer.capture(&scene, &target).unwrap();
    assert_ne!(a, b);
    p.layers[0].effects.truncate(1);
    p.layers[0].effects[0].hash = "a".repeat(64);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(renderer.capture(&scene, &target).is_err());
    renderer.draw(&scene, &target.view, 64, 64).unwrap();
    assert_eq!(renderer.effect_diagnostics.len(), 1);
    p.layers[0].effects[0].enabled = false;
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(renderer.capture(&scene, &target).is_ok());
}
#[test]
fn strict_plan_checks_hidden_layers_and_binary_buffer_bounds() {
    let mut p = fixture();
    p.layers[0].effects.push(instance("tint", 1));
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 64, 64, true).unwrap();
    let mut bytes = vec![0; plan.buffer_bytes(&scene)];
    assert_eq!(plan.write(&scene, &mut bytes).unwrap(), bytes.len());
    assert_eq!(
        u32::from_ne_bytes(bytes[..4].try_into().unwrap()),
        motion_render::effect_plan::PLAN_MAGIC
    );
    assert!(plan.write(&scene, &mut bytes[..4]).is_err());
    p.layers[0].visible = false;
    p.layers[0].effects[0].hash = "a".repeat(64);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(builder.build(&scene, &[0], 64, 64, true).is_err());
}

#[test]
fn effect_outputs_preserve_crossing_plane_batches_in_both_stack_orders() {
    let mut p = Project::new(256, 256, 30, 60).unwrap();
    p.background = [0.0, 0.0, 0.0, 1.0];
    for (id, angle, tint) in [
        (1, 45.0, [1.0, 0.0, 0.0, 1.0]),
        (2, -45.0, [0.0, 0.0, 1.0, 1.0]),
    ] {
        let mut layer = Layer::solid(id, "cross", [256.0; 2], [128.0, 128.0, 0.0], tint);
        layer.three_d = true;
        layer.transform.rotation.value[1] = angle;
        layer.effects.push(instance("brightness_contrast", id));
        p.layers.push(layer);
    }
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(256, 256).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    for _ in 0..2 {
        scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
        let plan = builder.build(&scene, &[0, 0], 256, 256, true).unwrap();
        assert_eq!(plan.batches.len(), 3);
        let mut bytes = vec![0; plan.buffer_bytes(&scene)];
        plan.write(&scene, &mut bytes).unwrap();
        assert_eq!(
            u32::from_ne_bytes(bytes[4..8].try_into().unwrap()),
            motion_render::effect_plan::PLAN_VERSION
        );
        assert_eq!(u32::from_ne_bytes(bytes[56..60].try_into().unwrap()), 3);
        let (pixels, _) = renderer.capture(&scene, &target).unwrap();
        let left = &pixels[(128 * 256 + 96) * 4..(128 * 256 + 96) * 4 + 4];
        let right = &pixels[(128 * 256 + 160) * 4..(128 * 256 + 160) * 4 + 4];
        assert!(left[2] > 245 && left[0] < 8, "{left:?}");
        assert!(right[0] > 245 && right[2] < 8, "{right:?}");
        p.layers.swap(0, 1);
    }
}

#[test]
fn over_budget_effect_is_reported_and_preview_keeps_input() {
    let mut p = fixture();
    p.layers[0].size = [4096.0; 2];
    p.layers[0].effects.push(instance("gaussian_blur", 1));
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let error = builder.build(&scene, &[0], 64, 64, true).err().unwrap();
    assert!(error.contains("layer 1, effect 1") && error.contains("64 MiB"));
    let preview = builder.build(&scene, &[0], 64, 64, false).unwrap();
    assert_eq!(preview.diagnostics.len(), 1);
    assert!(preview.passes.is_empty());
    assert_eq!(preview.draws[0].words[27], -1.0);
}

#[test]
fn full_hd_polar_then_edge_glow_fits_and_every_capacity_matches_written_passes() {
    let mut p = Project::new(1080, 1920, 30, 60).unwrap();
    p.layers.push(Layer::solid(
        1,
        "rectangle",
        [1080., 1920.],
        [540., 960., 0.],
        [1.; 4],
    ));
    // The screenshot uses core 1.1.0; the host fix must also apply to pinned packages.
    let package = motion_effects::builtin::packages()
        .unwrap()
        .into_iter()
        .find(|package| {
            package.manifest.id == motion_effects::builtin::PLUGIN_ID
                && package.manifest.version == "1.1.0"
        })
        .unwrap();
    for (i, name) in ["polar_coordinates", "glow_edges"].iter().enumerate() {
        let def = package
            .manifest
            .effects
            .iter()
            .find(|def| &def.id == name)
            .unwrap();
        p.layers[0].effects.push(EffectInstance::new(
            i as u64 + 1,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            def,
            [1080., 1920.],
        ));
    }
    // A common 48 px glow fails under the old seven equally sized, 128-aligned targets.
    p.layers[0].effects[1]
        .params
        .get_mut("radius")
        .unwrap()
        .track
        .value[0] = 48.;
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 1080, 1920, true).unwrap();
    assert_eq!(plan.slots, 0b01110111); // no unused sRGB ping-pong slot 3
    assert_eq!(plan.scratch_sizes[1], [1080, 1920]);
    assert_eq!(plan.scratch_sizes[5], [1176, 2016]);
    let mut derived = [[0; 2]; 8];
    for pass in &plan.passes {
        let size = &mut derived[pass.output as usize];
        size[0] = size[0].max(pass.width);
        size[1] = size[1].max(pass.height);
    }
    assert_eq!(
        plan.scratch_sizes, derived,
        "GLES derives capacities from this table"
    );
    let bytes = motion_render::effect_plan::scratch_capacity_bytes(&derived);
    assert!(bytes < motion_effects::SCRATCH_BUDGET);
    let old_bytes = motion_render::effect_plan::scratch_bytes(1280, 2048, 127);
    assert!(old_bytes > motion_effects::SCRATCH_BUDGET);
    assert!(bytes < old_bytes);

    // A true device limit remains an error and must retain its cause and effect context.
    builder.device_dimension = 1024;
    let error = builder.build(&scene, &[0], 1080, 1920, true).unwrap_err();
    assert!(
        error.contains("device dimension limit is 1024") && error.contains("layer 1, effect 1")
    );
}

#[test]
fn shared_execution_plan_uses_layer_local_effect_clock() {
    let mut p = fixture();
    p.layers[0].timeline = Some(motion_core::LayerTimeline {
        in_frame: 10,
        out_frame: 50,
        offset_frame: 10,
    });
    p.layers[0].effects.push(instance("wave_warp", 1));
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 25.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 64, 64, true).unwrap();
    assert!(!plan.passes.is_empty());
    for pass in &plan.passes {
        assert_eq!(pass.uniform.clock[0], 0.5);
        assert_eq!(pass.uniform.clock[1], 15.0);
    }
}

#[test]
fn zero_opacity_avoids_all_scratch_but_keeps_dependency_checks() {
    let mut p = fixture();
    p.layers[0].size = [4096.; 2];
    let mut effect = instance("glow_edges", 1);
    effect.params.get_mut("effect_opacity").unwrap().track.value[0] = 0.;
    p.layers[0].effects = vec![effect];
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 64, 64, true).unwrap();
    assert!(plan.passes.is_empty());
    assert_eq!(plan.slots, 0);
    assert_eq!(
        motion_render::effect_plan::scratch_capacity_bytes(&plan.scratch_sizes),
        0
    );
    p.layers[0].effects[0].hash = "a".repeat(64);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(builder.build(&scene, &[0], 64, 64, true).is_err());
}
