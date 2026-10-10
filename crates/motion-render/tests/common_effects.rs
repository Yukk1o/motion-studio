use motion_core::{Asset, Content, EffectInstance, Layer, Project, Scene};
use motion_render::{effect_plan::PlanBuilder, Renderer};
const SIZE: u32 = 64;

fn fixture() -> (Project, Vec<u8>) {
    let mut p = Project::new(SIZE, SIZE, 30, 60).unwrap();
    p.background = [0.; 4];
    let mut layer = Layer::solid(1, "pattern", [SIZE as f32; 2], [32., 32., 0.], [1.; 4]);
    layer.content = Content::Image { asset: 1 };
    p.layers.push(layer);
    p.assets.push(Asset {
        id: 1,
        path: "assets/pattern.png".into(),
        width: SIZE,
        height: SIZE,
    });
    let mut pixels = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            pixels.extend([
                ((x * 255) / (SIZE - 1)) as u8,
                ((y * 255) / (SIZE - 1)) as u8,
                if (x / 8 + y / 8) % 2 == 0 { 210 } else { 30 },
                if x < 4 || y < 4 {
                    0
                } else if x < 32 {
                    128
                } else {
                    255
                },
            ]);
        }
    }
    (p, pixels)
}
fn instance(name: &str) -> EffectInstance {
    let package = motion_effects::builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == name)
        .unwrap();
    EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        [SIZE as f32; 2],
    )
}
fn set(e: &mut EffectInstance, name: &str, value: f32) {
    e.params.get_mut(name).unwrap().track.value[0] = value;
}
fn render(p: &mut Project, r: &mut Renderer, t: &motion_render::CaptureTarget) -> Vec<u8> {
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    let mut scene = Scene::new(p);
    scene.sample(p, 0., None).unwrap();
    r.capture(&scene, t).unwrap().0
}

#[test]
fn common_effects_produce_results_preserve_alpha_and_bypass_at_zero_opacity() {
    let (mut p, pixels) = fixture();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.upload_image(1, SIZE, SIZE, &pixels).unwrap();
    let t = r.capture_target(SIZE, SIZE).unwrap();
    let reference = render(&mut p, &mut r, &t);
    for def in motion_effects::builtin::manifest()
        .effects
        .iter()
        .filter(|e| e.compatibility_profile == "ae18-common-srgb8-v1")
    {
        eprintln!("rendering {}", def.id);
        let mut e = instance(&def.id);
        match def.id.as_str() {
            "linear_wipe" | "radial_wipe" | "venetian_blinds" => set(&mut e, "completion", 40.),
            "channel_mixer" => {
                set(&mut e, "red_red", 0.);
                set(&mut e, "red_green", 100.);
            }
            _ => {}
        }
        p.layers[0].effects = vec![e.clone()];
        let output = render(&mut p, &mut r, &t);
        assert_ne!(output, reference, "{}", def.id);
        if !matches!(
            def.id.as_str(),
            "linear_wipe" | "radial_wipe" | "venetian_blinds" | "turbulent_displace"
        ) {
            for (a, b) in output.chunks_exact(4).zip(reference.chunks_exact(4)) {
                assert_eq!(a[3], b[3], "{} Alpha", def.id);
            }
        }
        if let Ok(path) = std::env::var("MOTION_COMMON_GALLERY") {
            std::fs::create_dir_all(&path).unwrap();
            image::save_buffer(
                std::path::Path::new(&path).join(format!("{}.png", def.id)),
                &output,
                SIZE,
                SIZE,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        set(&mut e, "effect_opacity", 0.);
        p.layers[0].effects = vec![e];
        let bypass = render(&mut p, &mut r, &t);
        assert!(
            bypass
                .iter()
                .zip(&reference)
                .all(|(a, b)| a.abs_diff(*b) <= 2),
            "{} bypass",
            def.id
        );
    }
}

#[test]
fn wipes_have_exact_endpoints_and_quantizers_have_meaningful_limits() {
    let (mut p, pixels) = fixture();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.upload_image(1, SIZE, SIZE, &pixels).unwrap();
    let t = r.capture_target(SIZE, SIZE).unwrap();
    let reference = render(&mut p, &mut r, &t);
    for name in ["linear_wipe", "radial_wipe", "venetian_blinds"] {
        let mut e = instance(name);
        set(&mut e, "feather", 32000.);
        p.layers[0].effects = vec![e.clone()];
        assert!(render(&mut p, &mut r, &t)
            .iter()
            .zip(&reference)
            .all(|(a, b)| a.abs_diff(*b) <= 2));
        set(&mut e, "completion", 100.);
        p.layers[0].effects = vec![e];
        assert!(
            render(&mut p, &mut r, &t).iter().all(|&v| v == 0),
            "{name}: complete wipe"
        );
    }
    let mut e = instance("posterize");
    set(&mut e, "levels", 2.);
    p.layers[0].effects = vec![e];
    let output = render(&mut p, &mut r, &t);
    for pixel in output.chunks_exact(4).filter(|p| p[3] == 255) {
        assert!(pixel[..3].iter().all(|&v| v == 0 || v == 255));
    }
    for (level, expected) in [(-30000., 255), (30000., 0)] {
        let mut e = instance("threshold");
        set(&mut e, "level", level);
        p.layers[0].effects = vec![e];
        let output = render(&mut p, &mut r, &t);
        for pixel in output.chunks_exact(4).filter(|p| p[3] == 255) {
            assert_eq!(&pixel[..3], &[expected; 3]);
        }
    }
}

#[test]
fn sampled_fractional_enums_cannot_bypass_project_validation() {
    let (mut p, _) = fixture();
    p.layers[0].effects = vec![instance("gradient_ramp")];
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let index = scene.effects[0]
        .param_ids
        .iter()
        .position(|id| id == "shape")
        .unwrap();
    scene.effects[0].values[index][0] = 1.5;
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let error = builder
        .build(&scene, &[0, 1], SIZE, SIZE, true)
        .unwrap_err();
    assert!(error.contains("parameter shape"));
    let preview = builder.build(&scene, &[0, 1], SIZE, SIZE, false).unwrap();
    assert!(preview.passes.is_empty());
    assert_eq!(preview.diagnostics.len(), 1);
}
