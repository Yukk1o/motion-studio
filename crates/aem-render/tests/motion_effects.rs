use aem_core::{
    Asset, Content, EffectImageInput, EffectImageStage, EffectInstance, Layer, Project, Scene,
};
use aem_render::{
    effect_plan::{image_input_order, PlanBuilder},
    CaptureTarget, Renderer,
};

const W: u32 = 96;
const H: u32 = 64;
fn instance(id: &str) -> EffectInstance {
    let p = aem_effects::builtin::package().unwrap();
    let d = p.manifest.effects.iter().find(|d| d.id == id).unwrap();
    EffectInstance::new(
        1,
        &p.manifest.id,
        &p.manifest.version,
        &p.hash,
        d,
        [W as f32, H as f32],
    )
}
fn value(e: &mut EffectInstance, id: &str, v: [f32; 4]) {
    e.params.get_mut(id).unwrap().track.value = v;
}
fn fixture() -> (Project, Renderer, CaptureTarget) {
    let mut p = Project::new(W, H, 30, 120).unwrap();
    p.background = [0.; 4];
    p.assets.push(Asset {
        id: 1,
        path: "assets/pattern.png".into(),
        width: W,
        height: H,
    });
    let mut l = Layer::solid(
        1,
        "source",
        [W as f32, H as f32],
        [W as f32 / 2., H as f32 / 2., 0.],
        [1.; 4],
    );
    l.content = Content::Image { asset: 1 };
    p.layers.push(l);
    let pixels = (0..H)
        .flat_map(|y| {
            (0..W).flat_map(move |x| {
                [
                    (x * 255 / (W - 1)) as u8,
                    (y * 255 / (H - 1)) as u8,
                    if (x / 6 + y / 6) % 2 == 0 { 240 } else { 16 },
                    if x < 4 || y < 4 {
                        0
                    } else if x < W / 2 {
                        128
                    } else {
                        255
                    },
                ]
            })
        })
        .collect::<Vec<_>>();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.upload_image(1, W, H, &pixels).unwrap();
    let target = r.capture_target(W, H).unwrap();
    (p, r, target)
}
fn render(p: &mut Project, r: &mut Renderer, t: &CaptureTarget, frame: f64) -> Vec<u8> {
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    let mut scene = Scene::new(p);
    scene.sample(p, frame, None).unwrap();
    r.capture(&scene, t).unwrap().0
}
fn close(a: &[u8], b: &[u8], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= tolerance)
}

#[test]
fn all_official_effects_render_bypass_and_survive_fractional_random_seeks() {
    let (mut p, mut r, t) = fixture();
    let reference = render(&mut p, &mut r, &t, 0.);
    for d in aem_effects::builtin::creative_effects() {
        let mut e = instance(&d.id);
        p.layers[0].effects = vec![e.clone()];
        let output = render(&mut p, &mut r, &t, 11.25);
        if d.id != "corner_pin" {
            assert_ne!(output, reference, "{} has no pixel effect", d.id);
        }
        render(&mut p, &mut r, &t, 81.5);
        let again = render(&mut p, &mut r, &t, 11.25);
        assert_eq!(again, output, "{} depends on seek history", d.id);
        if let Some(path) = std::env::var_os("MOTION_EXTENSION_GALLERY") {
            std::fs::create_dir_all(&path).unwrap();
            image::save_buffer(
                std::path::Path::new(&path).join(format!("{}.png", d.id)),
                &output,
                W,
                H,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        value(&mut e, "effect_opacity", [0.; 4]);
        p.layers[0].effects = vec![e];
        assert!(
            close(&render(&mut p, &mut r, &t, 11.25), &reference, 1),
            "{} bypass",
            d.id
        );
    }
}

#[test]
fn gradient_modes_points_grid_and_alpha_have_real_pixel_semantics() {
    let (mut p, mut r, t) = fixture();
    let mut outputs = Vec::new();
    for mode in 0..6 {
        let mut e = instance("gradient");
        value(&mut e, "shape", [mode as f32, 0., 0., 0.]);
        p.layers[0].effects = vec![e];
        outputs.push(render(&mut p, &mut r, &t, 0.));
    }
    for i in 0..6 {
        for j in i + 1..6 {
            assert_ne!(outputs[i], outputs[j], "gradient modes {i}/{j}");
        }
    }
    let mut e = instance("multi_point_gradient");
    for i in 0..5 {
        value(&mut e, &format!("point{i}"), [48., 32., 0., 0.]);
        value(&mut e, &format!("color{i}"), [1., 0., 0., 1.]);
    }
    p.layers[0].effects = vec![e];
    let output = render(&mut p, &mut r, &t, 0.);
    assert!(output
        .chunks_exact(4)
        .all(|c| c[0] > 250 && c[1] < 3 && c[2] < 3 && c[3] == 255));
    let mut e = instance("grid_gradient");
    for i in 0..16 {
        value(&mut e, &format!("color{i}"), [0., 1., 0., 0.5]);
    }
    p.layers[0].effects = vec![e];
    let output = render(&mut p, &mut r, &t, 0.);
    assert!(output
        .chunks_exact(4)
        .all(|c| c[0] < 3 && c[2] < 3 && c[3].abs_diff(128) < 2));
    value(
        &mut p.layers[0].effects[0],
        "effect_opacity",
        [50., 0., 0., 0.],
    );
    let half = render(&mut p, &mut r, &t, 0.);
    assert!(half[3].abs_diff(64) < 2);
    assert!(
        half[1] > 245,
        "straight-color opacity darkened transparent generator: {:?}",
        &half[..4]
    );
}

#[test]
fn channel_radii_are_independent_and_focus_band_is_exact() {
    let (mut p, mut r, t) = fixture();
    let reference = render(&mut p, &mut r, &t, 0.);
    let mut e = instance("channel_blur");
    value(&mut e, "red", [0.; 4]);
    value(&mut e, "green", [0.; 4]);
    value(&mut e, "blue", [18., 0., 0., 0.]);
    p.layers[0].effects = vec![e];
    let blurred = render(&mut p, &mut r, &t, 0.);
    assert!(blurred
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .all(|(a, b)| a[0].abs_diff(b[0]) < 3 && a[1].abs_diff(b[1]) < 3 && a[3] == b[3]));
    assert!(blurred
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .any(|(a, b)| a[2].abs_diff(b[2]) > 20));
    let mut e = instance("region_blur");
    value(&mut e, "mode", [1., 0., 0., 0.]);
    value(&mut e, "width", [12., 0., 0., 0.]);
    value(&mut e, "transition", [8., 0., 0., 0.]);
    p.layers[0].effects = vec![e];
    let output = render(&mut p, &mut r, &t, 0.);
    for y in 22..42 {
        let offset = (y * W * 4) as usize;
        assert!(close(
            &output[offset..offset + (W * 4) as usize],
            &reference[offset..offset + (W * 4) as usize],
            2
        ));
    }
    assert_ne!(output, reference);
}

#[test]
fn displacement_uses_hidden_effected_source_and_is_stack_order_independent() {
    let (mut p, mut r, t) = fixture();
    let mut src = Layer::solid(
        2,
        "material",
        [W as f32, H as f32],
        [48., 32., 0.],
        [0.5, 0.5, 0.5, 1.],
    );
    src.visible = false;
    let mut noise = instance("noise_generator");
    value(&mut noise, "basis", [2., 0., 0., 0.]);
    src.effects = vec![noise];
    p.layers.push(src);
    let mut e = instance("displacement_map");
    e.image_input = Some(EffectImageInput::Layer {
        layer: 2,
        stage: EffectImageStage::Effects,
    });
    p.layers[0].effects = vec![e.clone()];
    let effected = render(&mut p, &mut r, &t, 11.25);
    render(&mut p, &mut r, &t, 49.75);
    assert_eq!(effected, render(&mut p, &mut r, &t, 11.25));
    p.layers.reverse();
    assert_eq!(effected, render(&mut p, &mut r, &t, 11.25));
    p.layers.reverse();
    e.image_input = Some(EffectImageInput::Layer {
        layer: 2,
        stage: EffectImageStage::Source,
    });
    p.layers[0].effects = vec![e];
    let raw = render(&mut p, &mut r, &t, 11.25);
    assert_ne!(raw, effected);
    let parsed: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(
        parsed.layers[0].effects[0].image_input,
        p.layers[0].effects[0].image_input
    );
    let mut scene = Scene::new(&p);
    scene.sample(&p, 11.25, None).unwrap();
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    builder.build(&scene, &[0, 1], W, H, true).unwrap();
    assert!(image_input_order(&builder.frame).unwrap().is_empty());
    p.layers[1].timeline = Some(aem_core::LayerTimeline {
        in_frame: 0,
        out_frame: 20,
        offset_frame: 0,
    });
    let neutral = render(&mut p, &mut r, &t, 27.5);
    p.layers[0].effects.clear();
    let original = render(&mut p, &mut r, &t, 27.5);
    assert!(
        close(&neutral, &original, 2),
        "inactive material did not become neutral"
    );
}

#[test]
fn shared_font_atlas_is_a_persisted_effect_resource_and_dissolve_has_exact_endpoints() {
    let (mut p, mut r, t) = fixture();
    let root = tempfile::tempdir().unwrap();
    let mut fonts = aem_media::fonts::FontStore::new(root.path()).unwrap();
    let atlas = fonts
        .ascii_atlas(&aem_media::fonts::FontStore::builtin_id(), " .#@", 32.)
        .unwrap();
    p.assets.push(Asset {
        id: 2,
        path: "assets/font.png".into(),
        width: atlas.raster.width,
        height: atlas.raster.height,
    });
    r.upload_image(
        2,
        atlas.raster.width,
        atlas.raster.height,
        &atlas.raster.rgba,
    )
    .unwrap();
    let mut e = instance("ascii");
    e.image_input = Some(EffectImageInput::Asset { asset: 2 });
    value(&mut e, "glyph_count", [4., 0., 0., 0.]);
    value(
        &mut e,
        "glyph_aspect",
        [atlas.cell[0] as f32 / atlas.cell[1] as f32, 0., 0., 0.],
    );
    p.layers[0].effects = vec![e];
    let custom = render(&mut p, &mut r, &t, 0.);
    p.layers[0].effects = vec![instance("ascii")];
    assert_ne!(custom, render(&mut p, &mut r, &t, 0.));
    p.layers[0].effects.clear();
    let reference = render(&mut p, &mut r, &t, 0.);
    let mut e = instance("noise_dissolve");
    value(&mut e, "progress", [0.; 4]);
    p.layers[0].effects = vec![e.clone()];
    assert!(close(&render(&mut p, &mut r, &t, 0.), &reference, 2));
    value(&mut e, "progress", [1., 0., 0., 0.]);
    p.layers[0].effects = vec![e];
    assert!(render(&mut p, &mut r, &t, 0.).iter().all(|v| *v == 0));
}
