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
#[test]
fn four_k_adjustment_uses_composite_budget_separate_from_effect_scratch() {
    // Plan validation needs no 4K GPU allocation.
    let mut p = Project::new(3840, 2160, 30, 60).unwrap();
    let mut a = Layer::solid(1, "4K adjustment", [3840., 2160.], [1920., 1080., 0.], [1.; 4]);
    a.content = Content::Adjustment;
    a.effects.push(effect("motion_tile", 1));
    p.layers.push(a);
    p.rebuild_plugin_dependencies();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let mut b = PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let frame = b.build(&scene, &[0], 3840, 2160, true).unwrap();
    assert!(!frame.passes.is_empty());
    assert!(frame.diagnostics.is_empty());

    // A multi-pass effect still exceeds its own 64 MiB pool at full 4K;
    // raising the accumulator allowance must not waive that check.
    p.layers[0].effects = vec![effect("gaussian_blur", 1)];
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0., None).unwrap();
    let error = b.build(&scene, &[0], 3840, 2160, true).err().unwrap();
    assert!(error.contains("effect scratch textures"), "{error}");
    assert!(error.contains("64 MiB"), "{error}");
    assert!(!error.contains("accumulators"), "{error}");
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
    assert_eq!(word(1), aem_render::effect_plan::PLAN_VERSION);
    assert_eq!(word(22), 1);
    assert_eq!(word(27), 28);
}

#[test]
fn color_effect_keeps_expanded_vector_source_and_original_anchor_in_place() {
    let mut p = base();
    let mut l = Layer::solid(1, "expanded line", [20.; 2], [32., 32., 0.], [1.; 4]);
    let mut vector = VectorContent::shape(ShapeKind::Line);
    vector.stroke.as_mut().unwrap().width.value = 12.;
    l.content = Content::Vector { vector };
    l.transform.anchor = [0.25, 0.75];
    l.transform.rotation.value[2] = 15.;
    p.layers.push(l);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let before = capture(&mut r, &p);
    p.layers[0].effects.push(effect("tint", 1));
    p.rebuild_plugin_dependencies();
    let after = capture(&mut r, &p);
    let alpha_error: f64 = before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .map(|(a, b)| a[3].abs_diff(b[3]) as f64)
        .sum::<f64>()
        / 4096.;
    assert!(
        alpha_error <= 0.1,
        "identity color effect displaced expanded vector: {alpha_error}"
    );
}

#[test]
fn stacked_adjustments_preserve_alpha_parented_mask_and_crossing_three_d_layers() {
    let mut p = base();
    for (id, color, angle) in [(1, [1., 0., 0., 0.5], 25.), (2, [0., 1., 0., 0.4], -25.)] {
        let mut l = Layer::solid(id, "3d lower", [56.; 2], [32., 32., 0.], color);
        l.three_d = true;
        l.transform.rotation.value[1] = angle;
        p.layers.push(l);
    }
    let mut parent = Layer::solid(3, "parent", [1.; 2], [32., 32., 0.], [1.; 4]);
    parent.content = Content::Null;
    parent.transform.rotation.value[2] = 20.;
    p.layers.push(parent);
    let mut first = Layer::solid(4, "masked red", [24., 18.], [32., 32., 0.], [1.; 4]);
    first.content = Content::Adjustment;
    first.parent = Some(aem_core::ParentLink { object: Some(3), bind: glam::Mat4::IDENTITY.to_cols_array_2d() });
    first.transform.rotation.value[2] = 15.;
    first.transform.opacity.value = 0.5;
    first.effects.push(effect("tint", 1));
    first.effects[0]
        .params
        .get_mut("p0002")
        .unwrap()
        .track
        .value = [1., 0., 0., 1.];
    let mut second = Layer::solid(5, "full blue", [64.; 2], [32., 32., 0.], [1.; 4]);
    second.content = Content::Adjustment;
    second.effects.push(effect("tint", 1));
    second.effects[0]
        .params
        .get_mut("p0002")
        .unwrap()
        .track
        .value = [0., 0., 1., 1.];
    p.layers.push(first);
    p.layers.push(second);
    p.rebuild_plugin_dependencies();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let both = capture(&mut r, &p);
    let mut only = p.clone();
    only.layers.remove(3);
    only.rebuild_plugin_dependencies();
    let plain = capture(&mut r, &only);
    let mut s = Scene::new(&p);
    s.sample(&p, 0., None).unwrap();
    let inverse = s.layers.iter().find(|l| l.id == 4).unwrap().model.inverse();
    let mut inside = 0;
    for y in 0..64 {
        for x in 0..64 {
            let a = pixel(&both, x, y);
            let b = pixel(&plain, x, y);
            assert!(
                a[3].abs_diff(b[3]) <= 1,
                "adjustment duplicated alpha at {x},{y}"
            );
            let q = inverse.transform_point3(glam::Vec3::new(
                x as f32 + 0.5 - 32.,
                32. - y as f32 - 0.5,
                0.,
            ));
            if q.x.abs() > 14. || q.y.abs() > 11. {
                assert!(
                    a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 1),
                    "masked adjustment escaped its rotated parent at {x},{y}"
                );
            } else if q.x.abs() < 10. && q.y.abs() < 7. && a[2].abs_diff(b[2]) > 3 {
                inside += 1;
            }
        }
    }
    assert!(inside > 30, "parented mask had no visible effect");
    p.layers.swap(3, 4);
    assert_ne!(capture(&mut r, &p), both, "adjustment order was ignored");
    p.layers.push(Layer::solid(
        6,
        "upper",
        [6.; 2],
        [32., 32., 0.],
        [0., 1., 0., 1.],
    ));
    assert_eq!(pixel(&capture(&mut r, &p), 32, 32), [0, 255, 0, 255]);
}

#[test]
fn animated_geometry_fill_and_stroke_are_seek_independent_and_release_sources() {
    let mut p = base();
    let mut l = Layer::solid(1, "animated vector", [24.; 2], [32., 32., 0.], [1.; 4]);
    let mut vector = VectorContent::shape(ShapeKind::Heart);
    vector
        .fill
        .as_mut()
        .unwrap()
        .upsert(0, [1., 0., 0., 0.4], aem_core::Ease::Linear)
        .unwrap();
    vector
        .fill
        .as_mut()
        .unwrap()
        .upsert(40, [0., 1., 1., 0.8], aem_core::Ease::InOut)
        .unwrap();
    if let VectorSource::Shape { parameters, .. } = &mut vector.source {
        parameters
            .get_mut("angle")
            .unwrap()
            .upsert(0, 0., aem_core::Ease::Linear)
            .unwrap();
        parameters
            .get_mut("angle")
            .unwrap()
            .upsert(40, 120., aem_core::Ease::InOut)
            .unwrap();
    }
    vector.stroke = Some(Stroke {
        color: aem_core::Track::constant([1.; 4]),
        width: aem_core::Track::constant(6.),
        cap: LineCap::Round,
        join: LineJoin::Round,
        miter_limit: 4.,
    });
    vector
        .stroke
        .as_mut()
        .unwrap()
        .width
        .upsert(0, 6., aem_core::Ease::Linear)
        .unwrap();
    vector
        .stroke
        .as_mut()
        .unwrap()
        .width
        .upsert(40, 14., aem_core::Ease::InOut)
        .unwrap();
    l.content = Content::Vector { vector };
    p.layers.push(l);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let target = r.capture_target(64, 64).unwrap();
    let mut s = Scene::new(&p);
    s.sample(&p, 17., None).unwrap();
    let expected = r.capture(&s, &target).unwrap().0;
    for f in [39., 0., 17., 40., 8., 17.] {
        s.sample(&p, f, None).unwrap();
        let pixels = r.capture(&s, &target).unwrap().0;
        if f == 17. {
            assert_eq!(pixels, expected);
        } else {
            assert_ne!(pixels, expected);
        }
    }
    let active = r.texture_bytes();
    r.capture(&s, &target).unwrap();
    assert_eq!(r.texture_bytes(), active);
    p.layers.clear();
    s.sample(&p, 17., None).unwrap();
    r.capture(&s, &target).unwrap();
    assert!(
        r.texture_bytes() < active,
        "deleted vector source remained resident"
    );
}
