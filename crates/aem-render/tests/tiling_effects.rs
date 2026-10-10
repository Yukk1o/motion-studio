use aem_core::{Content, EffectInstance, Layer, Project, Scene};
use aem_render::{
    effect_plan::{scratch_capacity_bytes, PlanBuilder},
    Renderer,
};

fn fixture(effect: &str) -> Project {
    let package = aem_effects::builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == effect)
        .unwrap();
    let mut p = Project::new(128, 128, 30, 60).unwrap();
    p.background = [0.; 4];
    let mut layer = Layer::solid(1, "source", [32., 16.], [64., 64., 0.], [1.; 4]);
    layer.content = Content::Image { asset: 1 };
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        layer.size,
    ));
    p.assets.push(aem_core::Asset {
        id: 1,
        path: "test.png".into(),
        width: 32,
        height: 16,
    });
    p.layers.push(layer);
    p.rebuild_plugin_dependencies();
    p
}
fn set(p: &mut Project, id: &str, value: f32) {
    p.layers[0].effects[0]
        .params
        .get_mut(id)
        .unwrap()
        .track
        .value[0] = value;
}
fn sample(p: &Project, frame: f64) -> Scene {
    let mut scene = Scene::new(p);
    scene.sample(p, frame, None).unwrap();
    scene
}
fn pixels() -> Vec<u8> {
    (0..16)
        .flat_map(|y| (0..32).flat_map(move |x| [x * 8, y * 16, 100, 255]))
        .collect()
}

#[test]
fn asymmetric_expansion_cropping_preview_scale_and_budget_keep_layer_transform() {
    let mut p = fixture("motion_tile");
    let position = p.layers[0].transform.position.clone();
    set(&mut p, "output_width", 400.);
    set(&mut p, "output_height", 100.);
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let scene = sample(&p, 0.);
    let plan = builder.build(&scene, &[0, 1], 128, 128, true).unwrap();
    assert_eq!(plan.scratch_sizes[0], [128, 16]);
    assert_eq!(plan.scratch_sizes[1], [32, 16]);
    assert_eq!(plan.scratch_sizes[2], [0, 0]);
    assert_eq!(
        scratch_capacity_bytes(&plan.scratch_sizes),
        (128 * 16 + 32 * 16) * 4
    );
    assert_eq!(
        plan.passes.last().unwrap().uniform.region,
        [-48., 0., 128., 16.]
    );
    let plan = builder.build(&scene, &[0, 1], 64, 64, true).unwrap();
    assert_eq!(plan.scratch_sizes[0], [64, 8]);
    assert_eq!(p.layers[0].transform.position, position);
    set(&mut p, "output_width", 50.);
    let plan = builder
        .build(&sample(&p, 0.), &[0, 1], 128, 128, true)
        .unwrap();
    assert_eq!(
        plan.passes.last().unwrap().uniform.region,
        [8., 0., 16., 16.]
    );
    set(&mut p, "output_width", 30000.);
    set(&mut p, "output_height", 30000.);
    let error = builder
        .build(&sample(&p, 0.), &[0, 1], 128, 128, true)
        .err()
        .unwrap();
    assert!(
        error.contains("budget") || error.contains("dimension"),
        "{error}"
    );
    set(&mut p, "effect_opacity", 0.);
    assert!(builder
        .build(&sample(&p, 0.), &[0, 1], 128, 128, true)
        .unwrap()
        .passes
        .is_empty());
}

#[test]
fn tile_pixels_repeat_mirror_phase_alpha_and_zero_width_have_distinct_behavior() {
    let mut p = fixture("motion_tile");
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, 32, 16, &pixels()).unwrap();
    let target = renderer.capture_target(128, 128).unwrap();
    let render =
        |renderer: &mut Renderer, p: &Project| renderer.capture(&sample(p, 0.), &target).unwrap().0;
    let original = render(&mut renderer, &p);
    let pixel = |data: &[u8], x: usize, y: usize| -> [u8; 4] {
        data[(y * 128 + x) * 4..(y * 128 + x) * 4 + 4]
            .try_into()
            .unwrap()
    };
    assert_eq!(pixel(&original, 48, 56), [0, 0, 100, 255]);
    set(&mut p, "output_width", 400.);
    set(&mut p, "output_height", 400.);
    let repeat = render(&mut renderer, &p);
    assert_eq!(pixel(&repeat, 16, 56), pixel(&repeat, 48, 56));
    assert_eq!(pixel(&repeat, 49, 40), pixel(&repeat, 49, 56));
    assert_eq!(pixel(&repeat, 1, 1)[3], 0);
    set(&mut p, "mirror", 1.);
    let mirror = render(&mut renderer, &p);
    assert_eq!(pixel(&mirror, 47, 56), pixel(&mirror, 48, 56));
    assert_eq!(pixel(&mirror, 16, 56), pixel(&mirror, 79, 56));
    set(&mut p, "mirror", 0.);
    set(&mut p, "phase", 90.);
    let phase = render(&mut renderer, &p);
    assert_eq!(pixel(&phase, 49, 56), pixel(&repeat, 49, 56));
    assert_ne!(pixel(&phase, 17, 56), pixel(&repeat, 17, 56));
    set(&mut p, "tile_width", 0.);
    let collapsed = render(&mut renderer, &p);
    assert_eq!(pixel(&collapsed, 17, 56)[3], 255);
    assert_eq!(pixel(&collapsed, 17, 56), pixel(&collapsed, 49, 56));
}

#[test]
fn every_new_effect_renders_finite_and_unsupported_controls_fail_explicitly() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, 32, 16, &pixels()).unwrap();
    let target = renderer.capture_target(128, 128).unwrap();
    for name in [
        "motion_tile",
        "optics_compensation",
        "spherize",
        "cc_lens",
        "cc_radial_fast_blur",
        "simple_choker",
        "solid_composite",
    ] {
        let mut p = fixture(name);
        if name == "optics_compensation" {
            set(&mut p, "fov", 90.);
        }
        if name == "spherize" {
            set(&mut p, "radius", 12.);
        }
        if name == "simple_choker" {
            set(&mut p, "choke", -3.);
        }
        let output = renderer.capture(&sample(&p, 0.), &target).unwrap().0;
        assert_eq!(output.len(), 128 * 128 * 4);
        assert!(output.chunks_exact(4).any(|p| p[3] > 0), "{name}");
    }
    let mut p = fixture("solid_composite");
    set(&mut p, "blend_mode", 2.);
    assert!(renderer
        .capture(&sample(&p, 0.), &target)
        .unwrap_err()
        .to_string()
        .contains("blend_mode is invalid or unsupported"));
}

#[test]
fn animated_tiles_are_identical_after_restore_and_random_seeking() {
    let mut p = fixture("motion_tile");
    set(&mut p, "output_width", 400.);
    set(&mut p, "output_height", 400.);
    p.layers[0].effects[0]
        .params
        .get_mut("phase")
        .unwrap()
        .track
        .keys = vec![
        aem_core::Keyframe {
            frame: 0,
            value: [0.; 4],
            ease: aem_core::Ease::Linear,
            curve: None,
            spatial: None,
        },
        aem_core::Keyframe {
            frame: 30,
            value: [360., 0., 0., 0.],
            ease: aem_core::Ease::Linear,
            curve: None,
            spatial: None,
        },
    ];
    let restored: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, 32, 16, &pixels()).unwrap();
    let target = renderer.capture_target(128, 128).unwrap();
    let mut forward = vec![];
    for frame in [0., 7.5, 15., 30.] {
        forward.push(renderer.capture(&sample(&p, frame), &target).unwrap().0);
    }
    assert_ne!(forward[0], forward[1]);
    for i in [3, 0, 2, 1, 1, 3] {
        let frame = [0., 7.5, 15., 30.][i];
        assert_eq!(
            renderer
                .capture(&sample(&restored, frame), &target)
                .unwrap()
                .0,
            forward[i]
        );
    }
}

#[test]
fn full_hd_double_output_fits_budget_without_an_expanded_conversion_copy() {
    let mut p = fixture("motion_tile");
    p.width = 1080;
    p.height = 1920;
    p.layers[0].size = [1080., 1920.];
    p.assets[0].width = 1080;
    p.assets[0].height = 1920;
    set(&mut p, "output_width", 200.);
    set(&mut p, "output_height", 200.);
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder
        .build(&sample(&p, 0.), &[0, 1], 1080, 1920, true)
        .unwrap();
    assert_eq!(plan.scratch_sizes[0], [2160, 3840]);
    assert_eq!(plan.scratch_sizes[1], [1080, 1920]);
    assert_eq!(plan.slots, 3);
    assert_eq!(plan.passes.len(), 3); // materialize, convert input, render tile directly
    assert_eq!(scratch_capacity_bytes(&plan.scratch_sizes), 41_472_000);
}
