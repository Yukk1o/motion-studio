use aem_core::{EffectInstance, Layer, Project, Scene};
use aem_render::{effect_plan::PlanBuilder, Renderer};
fn instance(name: &str, id: u64) -> EffectInstance {
    let p = aem_effects::builtin::package().unwrap();
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
    scene.sample(&p, 0.0, None).unwrap();
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
    scene.sample(&p, 0.0, None).unwrap();
    let (a, stats) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(stats.parameter_resource_upload_bytes, 1024);
    let (_, stable_stats) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(stable_stats.parameter_resource_upload_bytes, 0);
    p.layers[0].effects.swap(0, 1);
    scene.sample(&p, 0.0, None).unwrap();
    let (b, _) = renderer.capture(&scene, &target).unwrap();
    assert_ne!(a, b);
    p.layers[0].effects.truncate(1);
    p.layers[0].effects[0].hash = "a".repeat(64);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0.0, None).unwrap();
    assert!(renderer.capture(&scene, &target).is_err());
    renderer.draw(&scene, &target.view, 64, 64).unwrap();
    assert_eq!(renderer.effect_diagnostics.len(), 1);
    p.layers[0].effects[0].enabled = false;
    scene.sample(&p, 0.0, None).unwrap();
    assert!(renderer.capture(&scene, &target).is_ok());
}
#[test]
fn strict_plan_checks_hidden_layers_and_binary_buffer_bounds() {
    let mut p = fixture();
    p.layers[0].effects.push(instance("tint", 1));
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None).unwrap();
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 64, 64, true).unwrap();
    let mut bytes = vec![0; plan.buffer_bytes(&scene)];
    assert_eq!(plan.write(&scene, &mut bytes).unwrap(), bytes.len());
    assert_eq!(
        u32::from_ne_bytes(bytes[..4].try_into().unwrap()),
        aem_render::effect_plan::PLAN_MAGIC
    );
    assert!(plan.write(&scene, &mut bytes[..4]).is_err());
    p.layers[0].visible = false;
    p.layers[0].effects[0].hash = "a".repeat(64);
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0.0, None).unwrap();
    assert!(builder.build(&scene, &[0], 64, 64, true).is_err());
}

#[test]
fn over_budget_effect_is_reported_and_preview_keeps_input() {
    let mut p = fixture();
    p.layers[0].size = [4096.0; 2];
    p.layers[0].effects.push(instance("gaussian_blur", 1));
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None).unwrap();
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let error = builder.build(&scene, &[0], 64, 64, true).err().unwrap();
    assert!(error.contains("layer 1, effect 1") && error.contains("64 MiB"));
    let preview = builder.build(&scene, &[0], 64, 64, false).unwrap();
    assert_eq!(preview.diagnostics.len(), 1);
    assert!(preview.passes.is_empty());
    assert_eq!(preview.draws[0].words[27], -1.0);
}
