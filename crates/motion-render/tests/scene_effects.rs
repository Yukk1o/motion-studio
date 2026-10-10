use motion_core::{Command, EffectAction, EffectInstance, Engine, Layer, Project};
use motion_render::Scene;
use motion_effects::{builtin, Registry};
use motion_render::{
    effect_plan::{PlanBuilder, PLAN_VERSION},
    Renderer,
};

fn project(id: &str) -> Project {
    let package = builtin::scene_package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == id)
        .unwrap();
    let mut p = Project::new(128, 128, 30, 120).unwrap();
    p.background = [0., 0., 0., 0.];
    p.layers.push(Layer::solid(
        1,
        "生成器",
        [128., 128.],
        [64., 64., 0.],
        [0., 0., 0., 0.],
    ));
    let mut engine = Engine::new(p).unwrap();
    engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::Insert {
                instance: EffectInstance::new(
                    1,
                    &package.manifest.id,
                    &package.manifest.version,
                    &package.hash,
                    def,
                    [128., 128.],
                ),
            },
        })
        .unwrap();
    engine.snapshot()
}
fn plan(p: &Project, frame: f64) -> (Scene, PlanBuilder) {
    let mut scene = Scene::new(p);
    scene.sample(p, frame, None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder = PlanBuilder::new(Registry::new_with_builtins().unwrap()).unwrap();
    builder.build(&scene, &[0], 128, 128, true).unwrap();
    (scene, builder)
}
#[test]
fn particles_cull_bounds_keep_offscreen_state_and_are_seek_order_independent() {
    let mut p = project("starfield");
    let e = &mut p.layers[0].effects[0];
    e.params.get_mut("extent").unwrap().track.value = [1024., 1024., 0., 0.];
    e.params.get_mut("fade").unwrap().track.value = [0.; 4];
    let (mut scene, mut builder) = plan(&p, 17.);
    let expected = builder.frame.sprites.clone();
    assert!(builder.frame.generator_stats.visible > 0);
    assert!(builder.frame.generator_stats.culled > 0);
    for f in [119., 0., 63., 4., 17.] {
        scene.sample(&p, f, None, &motion_core::ExpressionEvaluator).unwrap();
        builder.build(&scene, &[0], 128, 128, true).unwrap();
    }
    assert_eq!(builder.frame.sprites, expected);
    let alive = builder.frame.generator_stats.alive;
    p.layers[0].transform.position.value = [100000., 64., 0.];
    scene.sample(&p, 17., None, &motion_core::ExpressionEvaluator).unwrap();
    builder.build(&scene, &[0], 128, 128, true).unwrap();
    assert_eq!(builder.frame.generator_stats.alive, alive);
    assert!(builder.frame.sprites.is_empty());
    p.layers[0].transform.position.value = [64., 64., 0.];
    scene.sample(&p, 17., None, &motion_core::ExpressionEvaluator).unwrap();
    builder.build(&scene, &[0], 128, 128, true).unwrap();
    assert_eq!(builder.frame.sprites, expected);
}
#[test]
fn partial_sprite_edges_remain_visible_and_camera_moves_project_real_3d() {
    let mut p = project("starfield");
    p.layers[0].three_d = true;
    let e = &mut p.layers[0].effects[0];
    e.params.get_mut("extent").unwrap().track.value = [0.; 4];
    e.params.get_mut("fade").unwrap().track.value = [0.; 4];
    e.params.get_mut("size").unwrap().track.value = [80., 0., 0., 0.];
    e.params.get_mut("end_size").unwrap().track.value = [80., 0., 0., 0.];
    p.layers[0].transform.position.value = [145., 64., 0.];
    let (_, before) = plan(&p, 30.);
    assert!(!before.frame.sprites.is_empty());
    assert!(before.frame.sprites[0].rect[0] > 1.);
    p.layers[0].transform.position.value = [64., 64., 100.];
    let (_, far) = plan(&p, 30.);
    p.layers[0].transform.position.value = [64., 64., 0.];
    let (_, near) = plan(&p, 30.);
    assert!(far.frame.sprites[0].rect[2] < near.frame.sprites[0].rect[2]);
    p.camera.created = true;
    p.camera.position.value[0] += 30.;
    p.camera.target.value[0] += 30.;
    let (_, moved) = plan(&p, 30.);
    assert_ne!(moved.frame.sprites[0].rect, near.frame.sprites[0].rect);
}
#[test]
fn lens_follows_null_pivot_and_alpha_occlusion_obeys_depth_and_transparency() {
    let mut p = project("lens_flare");
    p.layers[0].three_d = true;
    let mut light = Layer::solid(2, "光源", [1., 1.], [40., 64., 100.], [0.; 4]);
    light.content = motion_core::Content::Null;
    light.three_d = true;
    p.layers.push(light);
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = Some(2);
    let (_, unoccluded) = plan(&p, 0.);
    assert!(!unoccluded.frame.sprites.is_empty());
    p.layers[0].effects[0].scene.as_mut().unwrap().occlusion = true;
    p.layers.push(Layer::solid(
        3,
        "遮挡",
        [128., 128.],
        [64., 64., 0.],
        [1.; 4],
    ));
    p.layers[2].three_d = true;
    let (_, hidden) = plan(&p, 0.);
    assert!(hidden.frame.sprites.is_empty());
    p.layers[2].transform.opacity.value = 0.5;
    let (_, half) = plan(&p, 0.);
    assert!(
        (half.frame.sprites[0].color[0] / unoccluded.frame.sprites[0].color[0] - 0.5).abs() < 1e-6
    );
    p.layers[2].transform.position.value[2] = 200.;
    let (_, behind) = plan(&p, 0.);
    assert_eq!(behind.frame.sprites, unoccluded.frame.sprites);
    p.layers[1].transform.position.value[0] = 90.;
    let (_, moved) = plan(&p, 0.);
    assert_ne!(moved.frame.sprites[0].rect, behind.frame.sprites[0].rect);
}
#[test]
fn particle_budget_failure_is_explicit_and_binary_plan_has_bounded_instances() {
    let mut p = project("sparks");
    let (scene, builder) = plan(&p, 30.);
    let mut bytes = vec![0; builder.frame.buffer_bytes(&scene)];
    builder.frame.write(&scene, &mut bytes).unwrap();
    assert_eq!(
        u32::from_ne_bytes(bytes[4..8].try_into().unwrap()),
        PLAN_VERSION
    );
    let offset = u32::from_ne_bytes(bytes[64..68].try_into().unwrap()) as usize;
    let end = offset
        + builder.frame.sprites.len() * std::mem::size_of::<motion_render::scene_generator::Sprite>();
    assert_eq!(
        &bytes[offset..end],
        bytemuck::cast_slice::<_, u8>(&builder.frame.sprites)
    );
    assert!(builder
        .frame
        .write(&scene, &mut bytes[..bytes.len() - 1].to_vec())
        .is_err());
    p.layers[0].effects[0]
        .params
        .get_mut("rate")
        .unwrap()
        .track
        .value = [10000., 0., 0., 0.];
    let mut scene = Scene::new(&p);
    scene.sample(&p, 30., None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder = PlanBuilder::new(Registry::new_with_builtins().unwrap()).unwrap();
    assert!(builder
        .build(&scene, &[0], 128, 128, true)
        .unwrap_err()
        .contains("capacity"));
    let fallback = builder.build(&scene, &[0], 128, 128, false).unwrap();
    assert_eq!(fallback.diagnostics.len(), 1);
    assert!(fallback.passes.is_empty());
}
#[test]
fn two_dimensional_generators_ignore_camera_motion_and_spatial_depth() {
    for id in ["starfield", "lens_flare"] {
        let mut p = project(id);
        p.camera.created = true;
        if id == "starfield" {
            p.layers[0].effects[0]
                .params
                .get_mut("extent")
                .unwrap()
                .track
                .value = [80., 80., 0., 0.];
        }
        let (_, before) = plan(&p, 30.);
        assert!(!before.frame.sprites.is_empty());
        p.camera.position.value[0] += 100.;
        p.camera.target.value[0] += 100.;
        p.layers[0].transform.position.value[2] = 200.;
        let (_, after) = plan(&p, 30.);
        assert_eq!(before.frame.sprites, after.frame.sprites);
    }
}
#[test]
fn all_generators_render_on_gpu_and_following_image_effects_execute() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(128, 128).unwrap();
    for id in [
        "lens_flare",
        "starfield",
        "sparks",
        "dust",
        "snow",
        "energy",
    ] {
        let mut p = project(id);
        if id != "lens_flare" {
            p.layers[0].effects[0]
                .params
                .get_mut("extent")
                .unwrap()
                .track
                .value = [80., 80., 40., 0.];
        }
        let (scene, _) = plan(&p, 30.);
        let (pixels, _) = renderer.capture(&scene, &target).unwrap();
        assert!(
            pixels.chunks_exact(4).any(|p| p[3] > 0),
            "{id} drew no particles or flares"
        );
        let package = builtin::package().unwrap();
        let def = package
            .manifest
            .effects
            .iter()
            .find(|d| d.id == "tint")
            .unwrap();
        p.layers[0].effects.push(EffectInstance::new(
            2,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            def,
            [128., 128.],
        ));
        p.rebuild_plugin_dependencies();
        let mut scene = Scene::new(&p);
        scene.sample(&p, 30., None, &motion_core::ExpressionEvaluator).unwrap();
        renderer.capture(&scene, &target).unwrap();
    }
}

#[test]
fn hidden_generator_capacity_missing_sources_and_chain_order_block_export() {
    let mut p = project("sparks");
    p.layers[0].visible = false;
    p.layers[0].effects[0]
        .params
        .get_mut("rate")
        .unwrap()
        .track
        .value = [10000., 0., 0., 0.];
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder = PlanBuilder::new(Registry::new_with_builtins().unwrap()).unwrap();
    assert!(builder
        .build(&scene, &[0], 128, 128, true)
        .unwrap_err()
        .contains("capacity"));
    let mut p = project("lens_flare");
    p.layers[0].visible = false;
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = Some(99);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(builder
        .build(&scene, &[0], 128, 128, true)
        .unwrap_err()
        .contains("source layer"));
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = None;
    let package = builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "tint")
        .unwrap();
    p.layers[0].effects.insert(
        0,
        EffectInstance::new(
            2,
            &package.manifest.id,
            &package.manifest.version,
            &package.hash,
            def,
            [128., 128.],
        ),
    );
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(builder
        .build(&scene, &[0], 128, 128, true)
        .unwrap_err()
        .contains("first enabled"));
}
#[test]
fn source_image_alpha_uses_filtered_pixels_and_cache_budget_is_explicit() {
    let mut p = project("lens_flare");
    p.layers[0].three_d = true;
    p.layers[0].effects[0]
        .params
        .get_mut("position")
        .unwrap()
        .track
        .value = [64., 64., 100., 0.];
    p.layers[0].effects[0].scene.as_mut().unwrap().occlusion = true;
    p.assets.push(motion_core::Asset {
        id: 1,
        path: "assets/mask.png".into(),
        width: 2,
        height: 1,
    });
    let mut layer = Layer::solid(2, "遮挡图片", [128., 128.], [64., 64., 0.], [1.; 4]);
    layer.three_d = true;
    layer.content = motion_core::Content::Image { asset: 1 };
    p.layers.push(layer);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let mut builder = PlanBuilder::new(Registry::new_with_builtins().unwrap()).unwrap();
    assert!(builder
        .build(&scene, &[0, 1], 128, 128, true)
        .unwrap_err()
        .contains("alpha"));
    builder
        .set_alpha(1, 2, 1, &[255, 255, 255, 0, 255, 255, 255, 255])
        .unwrap();
    builder.build(&scene, &[0, 1], 128, 128, true).unwrap();
    let half = builder.frame.sprites[0].color[0];
    p.layers[0].effects[0].scene.as_mut().unwrap().occlusion = false;
    scene.sample(&p, 0., None, &motion_core::ExpressionEvaluator).unwrap();
    builder.build(&scene, &[0, 1], 128, 128, true).unwrap();
    assert!((half / builder.frame.sprites[0].color[0] - 0.5).abs() < 0.02);
    assert!(builder.set_alpha(9, 16384, 16384, &[]).is_err());
}
