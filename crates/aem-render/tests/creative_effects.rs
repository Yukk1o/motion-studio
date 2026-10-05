use aem_core::{Asset, Content, EffectInstance, Layer, Project, Scene};
use aem_render::Renderer;

const SIZE: u32 = 96;
fn fixture() -> (Project, Vec<u8>) {
    let mut project = Project::new(SIZE, SIZE, 30, 120).unwrap();
    project.background = [0.0; 4];
    let mut layer = Layer::solid(1, "pattern", [SIZE as f32; 2], [48.0, 48.0, 0.0], [1.0; 4]);
    layer.content = Content::Image { asset: 1 };
    project.layers.push(layer);
    project.assets.push(Asset {
        id: 1,
        path: "assets/pattern.png".into(),
        width: SIZE,
        height: SIZE,
    });
    let mut pixels = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            let outside = x < 12 || x >= 84 || y < 12 || y >= 84;
            let pulse = (x as i32 - 31).abs() < 4 && (y as i32 - 42).abs() < 4;
            pixels.extend(if outside {
                [0, 0, 0, 0]
            } else if pulse {
                [255, 255, 255, 255]
            } else {
                [
                    (x * 255 / SIZE) as u8,
                    (y * 255 / SIZE) as u8,
                    if (x / 9 + y / 9) % 2 == 0 { 210 } else { 40 },
                    if x < 48 { 128 } else { 255 },
                ]
            });
        }
    }
    (project, pixels)
}
fn instance(name: &str) -> EffectInstance {
    let package = aem_effects::builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == name)
        .unwrap();
    EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        [SIZE as f32; 2],
    )
}
fn set(effect: &mut EffectInstance, id: &str, value: [f32; 4]) {
    effect.params.get_mut(id).unwrap().track.value = value;
}

#[test]
fn all_creative_effects_render_and_neutral_controls_preserve_the_input() {
    let (mut project, pixels) = fixture();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, SIZE, SIZE, &pixels).unwrap();
    let target = renderer.capture_target(SIZE, SIZE).unwrap();
    let mut scene = Scene::new(&project);
    scene.sample(&project, 0.0, None).unwrap();
    let reference = renderer.capture(&scene, &target).unwrap().0;
    let package = aem_effects::builtin::package().unwrap();
    for definition in package
        .manifest
        .effects
        .iter()
        .filter(|e| e.compatibility_profile == "motion-creative-sapphire-inspired-v1")
    {
        let mut effect = instance(&definition.id);
        project.layers[0].effects = vec![effect.clone()];
        project.rebuild_plugin_dependencies();
        project.validate().unwrap();
        scene.sample(&project, 17.0, None).unwrap();
        let output = renderer.capture(&scene, &target).unwrap().0;
        assert_ne!(
            reference, output,
            "{} must produce a visible result",
            definition.id
        );
        if let Ok(path) = std::env::var("MOTION_EFFECT_GALLERY") {
            std::fs::create_dir_all(&path).unwrap();
            image::save_buffer(
                std::path::Path::new(&path).join(format!("{}.png", definition.id)),
                &output,
                SIZE,
                SIZE,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        match definition.id.as_str() {
            "glow" | "glow_edges" | "rays" | "edge_rays" | "streaks" | "glint" => {
                set(&mut effect, "strength", [0.0; 4])
            }
            "kaleido" | "kaleido_polar" => set(&mut effect, "mix", [0.0; 4]),
            "transform_blur" => set(&mut effect, "translation_blur", [0.0; 4]),
            _ => set(&mut effect, "amount", [0.0; 4]),
        }
        project.layers[0].effects = vec![effect];
        scene.sample(&project, 17.0, None).unwrap();
        let neutral = renderer.capture(&scene, &target).unwrap().0;
        let error = reference
            .iter()
            .zip(&neutral)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            error <= 2,
            "{} neutral maximum error={error}",
            definition.id
        );
    }
}

#[test]
fn seeded_time_effects_are_independent_of_seek_order_and_follow_layer_local_time() {
    let (mut project, pixels) = fixture();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, SIZE, SIZE, &pixels).unwrap();
    let target = renderer.capture_target(SIZE, SIZE).unwrap();
    let mut scene = Scene::new(&project);
    for name in [
        "shake",
        "grain",
        "light_leak",
        "film_damage",
        "digital_damage",
    ] {
        let mut effect = instance(name);
        effect.seed = 12345;
        project.layers[0].effects = vec![effect];
        project.rebuild_plugin_dependencies();
        scene.sample(&project, 25.0, None).unwrap();
        let a = renderer.capture(&scene, &target).unwrap().0;
        for frame in [80.0, 0.0, 35.0, 5.0, 25.0] {
            scene.sample(&project, frame, None).unwrap();
            let output = renderer.capture(&scene, &target).unwrap().0;
            if frame == 25.0 {
                assert_eq!(a, output, "{name}: seek must be reproducible");
            }
        }
        project.layers[0].timeline = Some(aem_core::LayerTimeline {
            in_frame: 10,
            out_frame: 110,
            offset_frame: 10,
        });
        scene.sample(&project, 35.0, None).unwrap();
        assert_eq!(
            a,
            renderer.capture(&scene, &target).unwrap().0,
            "{name}: shifted clip must retain its effect clock"
        );
        project.layers[0].timeline = None;
        project.layers[0].effects[0].seed = 54321;
        scene.sample(&project, 25.0, None).unwrap();
        assert_ne!(
            a,
            renderer.capture(&scene, &target).unwrap().0,
            "{name}: seed must affect the result"
        );
    }
}

#[test]
fn glow_extends_alpha_without_moving_the_layer_and_chroma_preserves_empty_pixels() {
    let mut project = Project::new(SIZE, SIZE, 30, 120).unwrap();
    project.background = [0.0; 4];
    project.layers.push(Layer::solid(
        1,
        "light",
        [16.0; 2],
        [48.0, 48.0, 0.0],
        [1.0; 4],
    ));
    let mut glow = instance("glow");
    set(&mut glow, "radius", [8.0, 0.0, 0.0, 0.0]);
    project.layers[0].effects = vec![glow];
    project.rebuild_plugin_dependencies();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(SIZE, SIZE).unwrap();
    let mut scene = Scene::new(&project);
    scene.sample(&project, 0.0, None).unwrap();
    let output = renderer.capture(&scene, &target).unwrap().0;
    assert!(
        output[((48 * SIZE + 37) * 4 + 3) as usize] > 0,
        "glow must extend beyond the original boundary"
    );
    assert_eq!(output[((48 * SIZE + 48) * 4 + 3) as usize], 255);
    assert_eq!(
        project.layers[0].transform.position.value,
        [48.0, 48.0, 0.0]
    );
    project.layers[0].effects = vec![instance("warp_chroma")];
    project.rebuild_plugin_dependencies();
    scene.sample(&project, 0.0, None).unwrap();
    let output = renderer.capture(&scene, &target).unwrap().0;
    assert_eq!(&output[..4], &[0; 4]);
}
