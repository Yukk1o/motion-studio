use aem_core::{vector::*, Content, EffectInstance, Layer, Project, Scene};
use aem_render::{effect_plan::PlanBuilder, Renderer};
fn effect(name: &str, id: u64) -> EffectInstance {
    let p = aem_effects::builtin::package().unwrap();
    let d = p.manifest.effects.iter().find(|d| d.id == name).unwrap();
    EffectInstance::new(
        id,
        &p.manifest.id,
        &p.manifest.version,
        &p.hash,
        d,
        [64.; 2],
    )
}
fn base() -> Project {
    let mut p = Project::new(64, 64, 30, 60).unwrap();
    p.background = [0.; 4];
    p
}
fn capture(r: &mut Renderer, p: &Project) -> Vec<u8> {
    let mut s = Scene::new(p);
    s.sample(p, 0., None).unwrap();
    let target = r.capture_target(64, 64).unwrap();
    r.capture(&s, &target).unwrap().0
}
fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}
#[test]
fn vector_holes_open_paths_stroke_and_cache_render() {
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut p = base();
    let mut l = Layer::solid(1, "ring", [40.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Vector {
        vector: VectorContent::shape(ShapeKind::Ring),
    };
    p.layers.push(l);
    let first = capture(&mut r, &p);
    assert_eq!(pixel(&first, 32, 32)[3], 0);
    assert!(pixel(&first, 48, 32)[3] > 240);
    let bytes = r.texture_bytes();
    let second = capture(&mut r, &p);
    assert_eq!(first, second);
    assert_eq!(r.texture_bytes(), bytes);
    p.layers[0].content = Content::Vector {
        vector: VectorContent::shape(ShapeKind::Line),
    };
    let line = capture(&mut r, &p);
    assert!(pixel(&line, 32, 32)[3] > 240);
    assert_eq!(pixel(&line, 32, 24)[3], 0);
    for (kind, _, _, _) in SHAPES {
        p.layers[0].content = Content::Vector {
            vector: VectorContent::shape(kind),
        };
        let pixels = capture(&mut r, &p);
        assert!(pixels.chunks_exact(4).any(|c| c[3] > 0), "{kind:?}");
    }
}
#[test]
fn adjustment_changes_lower_composite_and_keeps_upper_and_background() {
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut p = base();
    p.background = [0., 0., 1., 1.];
    p.layers.push(Layer::solid(
        1,
        "lower",
        [32.; 2],
        [20., 32., 0.],
        [1., 0., 0., 0.5],
    ));
    let mut a = Layer::solid(2, "adjust", [64.; 2], [32., 32., 0.], [1.; 4]);
    a.content = Content::Adjustment;
    a.effects.push(effect("tint", 1));
    p.layers.push(a);
    p.layers.push(Layer::solid(
        3,
        "upper",
        [8.; 2],
        [48., 32., 0.],
        [0., 1., 0., 1.],
    ));
    p.rebuild_plugin_dependencies();
    let pixels = capture(&mut r, &p);
    let lower = pixel(&pixels, 20, 32);
    assert!(lower[0].abs_diff(lower[1]) <= 1 && lower[2] > lower[0]);
    assert_eq!(pixel(&pixels, 48, 32), [0, 255, 0, 255]);
    assert_eq!(pixel(&pixels, 60, 60), [0, 0, 255, 255]);
    p.layers[1].transform.opacity.value = 0.;
    let off = capture(&mut r, &p);
    assert!(pixel(&off, 20, 32)[0] > pixel(&pixels, 20, 32)[0]);
    p.layers[1].transform.opacity.value = 0.5;
    let half = capture(&mut r, &p);
    assert!(pixel(&half, 20, 32)[0] > lower[0] && pixel(&half, 20, 32)[0] < pixel(&off, 20, 32)[0]);
}
#[test]
fn adjustment_blur_spreads_beyond_each_lower_layer_and_obeys_mask() {
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let mut p = base();
    p.layers.push(Layer::solid(
        1,
        "red",
        [8., 32.],
        [25., 32., 0.],
        [1., 0., 0., 1.],
    ));
    p.layers.push(Layer::solid(
        2,
        "green",
        [8., 32.],
        [39., 32., 0.],
        [0., 1., 0., 1.],
    ));
    let mut a = Layer::solid(3, "blur", [64.; 2], [32., 32., 0.], [1.; 4]);
    a.content = Content::Adjustment;
    let mut blur = effect("gaussian_blur", 1);
    blur.params.get_mut("p0001").unwrap().track.value[0] = 8.;
    a.effects.push(blur);
    p.layers.push(a);
    p.rebuild_plugin_dependencies();
    let pixels = capture(&mut r, &p);
    assert!(pixel(&pixels, 32, 32)[3] > 0);
    assert!(pixel(&pixels, 32, 32)[0] > 0 && pixel(&pixels, 32, 32)[1] > 0);
    p.layers[2].size = [16., 64.];
    p.layers[2].transform.position.value = [10., 32., 0.];
    let masked = capture(&mut r, &p);
    assert_eq!(pixel(&masked, 32, 32)[3], 0);
    assert_eq!(pixel(&masked, 25, 32), [255, 0, 0, 255]);
}
#[test]
fn versioned_plan_contains_vector_sources_and_adjustment_boundary() {
    let mut p = base();
    let mut l = Layer::solid(1, "star", [32.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Vector {
        vector: VectorContent::shape(ShapeKind::Star),
    };
    p.layers.push(l);
    let mut a = Layer::solid(2, "adjust", [64.; 2], [32., 32., 0.], [1.; 4]);
    a.content = Content::Adjustment;
    p.layers.push(a);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let mut b = PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let frame = b.build(&scene, &[0], 64, 64, true).unwrap();
    assert_eq!(frame.vectors.len(), 1);
    assert_eq!(frame.draws[0].words[31], 1.);
    assert_eq!(frame.draws[1].words[31], 2.);
    let mut bytes = vec![0; frame.buffer_bytes(&scene)];
    frame.write(&scene, &mut bytes).unwrap();
    let word = |i: usize| u32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!(word(1), 4);
    assert_eq!(word(22), 1);
    assert_eq!(word(27), 28);
}
