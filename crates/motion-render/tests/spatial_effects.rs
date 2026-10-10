use motion_core::{EffectInstance, Layer, Project, Scene};
use motion_render::{effect_plan::PlanBuilder, Renderer};

fn fixture(effect: &str) -> Project {
    let package = motion_effects::builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == effect)
        .unwrap();
    let mut p = Project::new(192, 192, 30, 120).unwrap();
    p.background = [0.; 4];
    let mut layer = Layer::solid(1, "moving plane", [32., 20.], [96., 96., 0.], [1.; 4]);
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        layer.size,
    ));
    p.layers.push(layer);
    p.rebuild_plugin_dependencies();
    p
}
fn set(p: &mut Project, id: &str, value: [f32; 4]) {
    p.layers[0].effects[0]
        .params
        .get_mut(id)
        .unwrap()
        .track
        .value = value;
}
fn scene(p: &Project, f: f64) -> Scene {
    let mut s = Scene::new(p);
    s.sample(p, f, None).unwrap();
    s
}
fn render(r: &mut Renderer, p: &Project, f: f64, extent: u32) -> Vec<u8> {
    let target = r.capture_target(extent, extent).unwrap();
    r.capture(&scene(p, f), &target).unwrap().0
}
fn mass_and_center(pixels: &[u8], width: usize) -> (f64, [f64; 2]) {
    let mut sum = 0.;
    let mut center = [0.; 2];
    for (i, p) in pixels.chunks_exact(4).enumerate() {
        let a = f64::from(p[3]);
        sum += a;
        center[0] += (i % width) as f64 * a;
        center[1] += (i / width) as f64 * a;
    }
    (sum, [center[0] / sum, center[1] / sum])
}

#[test]
fn shake_moves_the_complete_plane_outside_the_original_bounds_without_editing_tracks() {
    let mut p = fixture("shake");
    set(&mut p, "translation", [32., 16., 0., 0.]);
    set(&mut p, "rotation", [0.; 4]);
    set(&mut p, "zoom", [0.; 4]);
    let transform = p.layers[0].transform.clone();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut plain = p.clone();
    plain.layers[0].effects.clear();
    plain.rebuild_plugin_dependencies();
    let original = render(&mut r, &plain, 17., 192);
    let (a, c) = mass_and_center(&original, 192);
    let shifted = render(&mut r, &p, 17., 192);
    let (b, d) = mass_and_center(&shifted, 192);
    let shifted_scene=scene(&p,17.);
    let registry=motion_effects::Registry::new_with_builtins().unwrap();
    let outline=motion_core::selection_geometry::polygon(&shifted_scene.layers[0],&shifted_scene,&registry).unwrap();
    let selection_center=[outline.iter().map(|v|v[0] as f64).sum::<f64>()/4.,outline.iter().map(|v|v[1] as f64).sum::<f64>()/4.];
    assert!((selection_center[0]-d[0]-0.5).abs()<1. && (selection_center[1]-d[1]-0.5).abs()<1., "selection {:?} differs from rendered pixel center {:?}",selection_center,d);
    assert!((a - b).abs() / a < 0.04, "source coverage lost: {a} -> {b}");
    assert!(
        (c[0] - d[0]).abs().max((c[1] - d[1]).abs()) > 2.,
        "plane did not move: {c:?} -> {d:?}"
    );
    assert!(shifted
        .chunks_exact(4)
        .zip(original.chunks_exact(4))
        .any(|(m, o)| m[3] > 200 && o[3] == 0));
    assert_eq!(p.layers[0].transform, transform);
    for f in [31., 0., 79., 17.] {
        let output = render(&mut r, &p, f, 192);
        if f == 17. {
            assert_eq!(output, shifted);
        }
    }
    let half = render(&mut r, &p, 17., 96);
    let (_, h) = mass_and_center(&half, 96);
    assert!((h[0] * 2. - d[0]).abs() < 2. && (h[1] * 2. - d[1]).abs() < 2.);
    // A following color effect must retain the displaced rectangle's origin.
    let package = motion_effects::builtin::package().unwrap();
    let tint = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "tint")
        .unwrap();
    let size = p.layers[0].size;
    p.layers[0].effects.push(EffectInstance::new(
        2,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        tint,
        size,
    ));
    p.rebuild_plugin_dependencies();
    let tinted = render(&mut r, &p, 17., 192);
    assert_eq!(
        tinted.chunks_exact(4).map(|p| p[3]).collect::<Vec<_>>(),
        shifted.chunks_exact(4).map(|p| p[3]).collect::<Vec<_>>()
    );
}

#[test]
fn transformed_motion_blur_moves_the_plane_and_retains_the_unmodified_opacity_source() {
    let mut p = fixture("transform_blur");
    set(&mut p, "shift", [38., 24., 0., 0.]);
    set(&mut p, "translation_blur", [0.; 4]);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut plain = p.clone();
    plain.layers[0].effects.clear();
    plain.rebuild_plugin_dependencies();
    let original = render(&mut r, &plain, 0., 192);
    let (a, c) = mass_and_center(&original, 192);
    let shifted = render(&mut r, &p, 0., 192);
    let (b, d) = mass_and_center(&shifted, 192);
    assert!((a - b).abs() / a < 0.04);
    assert!((d[0] - c[0] - 38.).abs() < 1. && (d[1] - c[1] - 24.).abs() < 1.);
    assert_eq!(
        shifted[(96 * 192 + 96) * 4 + 3],
        0,
        "the original plane was filled by edge repetition"
    );
    set(&mut p, "effect_opacity", [50., 0., 0., 0.]);
    let mixed = render(&mut r, &p, 0., 192);
    assert!((120..=135).contains(&mixed[(96 * 192 + 96) * 4 + 3]));
    assert!((120..=135).contains(&mixed[(120 * 192 + 134) * 4 + 3]));
}

#[test]
fn polar_unwrap_keeps_corner_pixels_below_the_original_layer_and_zero_amount_is_identity() {
    let mut p = fixture("polar_coordinates");
    set(&mut p, "p0001", [1., 0., 0., 0.]);
    let package = motion_effects::builtin::package().unwrap();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut plain = p.clone();
    plain.layers[0].effects.clear();
    plain.rebuild_plugin_dependencies();
    let original = render(&mut r, &plain, 0., 192);
    let unwrapped = render(&mut r, &p, 0., 192);
    assert!(
        unwrapped
            .chunks_exact(4)
            .enumerate()
            .any(|(i, p)| i / 192 > 107 && p[3] > 200),
        "polar tail clipped at the original bottom edge"
    );
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene(&p, 0.), &[0], 192, 192, true).unwrap();
    let region = plan.passes.last().unwrap().uniform.region;
    assert_eq!(region[0..2], [0., 0.]);
    assert!(region[3] > 36.);
    // Top geometry stays at the original top, rather than recentering its taller output.
    let top = plan
        .vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!((top - 10.).abs() < 0.01, "top moved: {top}");
    set(&mut p, "p0001", [0.; 4]);
    assert_eq!(render(&mut r, &p, 0., 192), original);
    // Package upgrade does not mutate parameter ranges or the saved legacy package.
    assert_eq!(package.manifest.version, "2.0.0");
}

#[test]
fn asymmetric_rectangle_origin_retains_three_d_transform_and_resource_errors_are_explicit() {
    let mut p = fixture("motion_tile");
    set(&mut p, "output_width", [50., 0., 0., 0.]);
    set(&mut p, "output_height", [100., 0., 0., 0.]);
    let mut builder =
        PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene(&p, 0.), &[0], 192, 192, true).unwrap();
    let xs: Vec<_> = plan.vertices.iter().map(|v| v.position[0]).collect();
    assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), -8.);
    // A noncentered custom output rectangle is transformed around the original
    // anchor, including the 3D rotation, rather than the output texture's center.
    p.layers[0].three_d = true;
    p.layers[0].transform.anchor = [0.2, 0.8];
    p.layers[0].transform.rotation.value = [15., 35., 12.];
    p.layers[0].transform.scale.value = [130., 80., 100.];
    let sampled = scene(&p, 0.);
    let mut compositor = motion_core::PlaneCompositor::new();
    compositor
        .prepare_with_bounds_and_overlays(&sampled, &[[40., 36.]], &[[9., -7.]], &[])
        .unwrap();
    let model = sampled.layers[0].model;
    let corners = [
        [9. - 16., 10. + 7.],
        [49. - 16., 10. + 7.],
        [9. - 16., 10. - 29.],
        [49. - 16., 10. - 29.],
    ];
    for [x, y] in corners {
        let expected = model.transform_point3(glam::Vec3::new(x, y, 0.));
        assert!(
            compositor
                .vertices
                .iter()
                .any(|v| glam::Vec3::from_array(v.position).distance(expected) < 1e-4),
            "missing transformed output corner {expected:?}"
        );
    }
    p = fixture("transform_blur");
    set(&mut p, "shift", [8192., 8192., 0., 0.]);
    let error = builder
        .build(&scene(&p, 0.), &[0], 192, 192, true)
        .err()
        .unwrap();
    assert!(
        error.contains("budget") || error.contains("dimension"),
        "{error}"
    );
}
