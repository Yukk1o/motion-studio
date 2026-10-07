use aem_core::{
    Content, Ease, EffectInstance, Keyframe, Layer, LayerTimeline, ParentLink, Project, Scene,
};
use aem_effects::{builtin, Registry};
use aem_render::{effect_plan::PlanBuilder, Renderer};
fn key<T>(frame: i32, value: T) -> Keyframe<T> {
    Keyframe {
        frame,
        value,
        ease: Ease::Linear,
        curve: None,
    }
}
fn project() -> Project {
    let package = builtin::particle_package().unwrap();
    let mut p = Project::new(128, 128, 30, 120).unwrap();
    p.background = [0.; 4];
    let mut l = Layer::solid(1, "Emitter", [128., 128.], [32., 64., 0.], [0.; 4]);
    l.transform.position.keys = vec![key(0, [32., 64., 0.]), key(30, [96., 64., 0.])];
    let mut e = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        &package.manifest.effects[0],
        [128., 128.],
    );
    for (id, value) in [
        ("rate", 2.),
        ("lifetime", 2.),
        ("speed", 0.),
        ("spread", 0.),
        ("inherit_velocity", 0.),
        ("drag", 0.),
        ("fade", 0.),
        ("size", 8.),
        ("end_size", 8.),
    ] {
        e.params.get_mut(id).unwrap().track.value = [value, 0., 0., 0.];
    }
    e.params.get_mut("gravity").unwrap().track.value = [0.; 4];
    e.params.get_mut("color").unwrap().track.value = [1.; 4];
    e.params.get_mut("end_color").unwrap().track.value = [1.; 4];
    l.effects.push(e);
    p.layers.push(l);
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    p
}
fn builder() -> PlanBuilder {
    PlanBuilder::new(Registry::new_with_builtins().unwrap()).unwrap()
}
fn sample(p: &Project, s: &mut Scene, b: &mut PlanBuilder, f: f64) {
    s.sample(p, f, None).unwrap();
    b.build(s, &[0], 128, 128, true).unwrap();
}
fn xs(b: &PlanBuilder) -> Vec<f32> {
    b.frame
        .sprites
        .iter()
        .map(|s| (s.rect[0] + 1.) * 64.)
        .collect()
}
#[test]
fn moving_emitter_leaves_old_births_and_reuses_their_poses() {
    let p = project();
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    assert_eq!(xs(&b), vec![32., 64., 96.]);
    assert_eq!(b.frame.generator_stats.births_sampled, 3);
    let original = s.effects[0].particle_history.clone().unwrap();
    sample(&p, &mut s, &mut b, 31.);
    assert_eq!(xs(&b), vec![32., 64., 96.]);
    assert_eq!(b.frame.generator_stats.births_sampled, 0);
    assert!(std::sync::Arc::ptr_eq(
        &original,
        s.effects[0].particle_history.as_ref().unwrap()
    ));
    sample(&p, &mut s, &mut b, 45.);
    assert_eq!(b.frame.generator_stats.births_sampled, 1);
}
#[test]
fn seek_reverse_save_restore_and_seed_edits_are_deterministic() {
    let mut p = project();
    p.layers[0].effects[0]
        .params
        .get_mut("spread")
        .unwrap()
        .track
        .value[0] = 25.;
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    let expected = b.frame.sprites.clone();
    for f in [110., 0., 55., 11., 30.] {
        sample(&p, &mut s, &mut b, f);
    }
    assert_eq!(b.frame.sprites, expected);
    let restored: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    restored.validate().unwrap();
    sample(&restored, &mut s, &mut b, 30.);
    assert_eq!(b.frame.sprites, expected);
    p.layers[0].effects[0].seed += 1;
    sample(&p, &mut s, &mut b, 30.);
    assert_ne!(b.frame.sprites, expected);
    assert_eq!(b.frame.generator_stats.births_sampled, 3);
}
#[test]
fn linked_null_parent_and_clip_offsets_sample_birth_composition_time() {
    let mut p = project();
    p.layers[0].timeline = Some(LayerTimeline {
        in_frame: 30,
        out_frame: 120,
        offset_frame: 30,
    });
    let mut source = Layer::solid(2, "Source", [1., 1.], [0., 0., 0.], [0.; 4]);
    source.content = Content::Null;
    source.transform.position.keys = vec![key(0, [16., 64., 0.]), key(60, [80., 64., 0.])];
    let mut parent = Layer::solid(3, "Parent", [1., 1.], [80., 64., 0.], [0.; 4]);
    parent.content = Content::Null;
    source.parent = Some(ParentLink {
        object: Some(3),
        bind: glam::Mat4::IDENTITY.to_cols_array_2d(),
    });
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = Some(2);
    p.layers.extend([source, parent]);
    p.validate().unwrap();
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 60.);
    assert_eq!(xs(&b), vec![64., 80., 96.]);
}
#[test]
fn animated_velocity_and_appearance_are_frozen_at_birth() {
    let mut p = project();
    let e = &mut p.layers[0].effects[0];
    e.params.get_mut("direction").unwrap().track.value = [1., 0., 0., 0.];
    e.params.get_mut("speed").unwrap().track.keys =
        vec![key(0, [0.; 4]), key(30, [32., 0., 0., 0.])];
    e.params.get_mut("size").unwrap().track.keys =
        vec![key(0, [8., 0., 0., 0.]), key(30, [24., 0., 0., 0.])];
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    assert_eq!(xs(&b), vec![32., 72., 96.]);
    assert!((b.frame.sprites[0].rect[2] * 64. - 8.).abs() < 1e-5);
    assert!((b.frame.sprites[1].rect[2] * 64. - 14.).abs() < 1e-5);
    assert!((b.frame.sprites[2].rect[2] * 64. - 24.).abs() < 1e-5);
}
#[test]
fn inherited_velocity_gravity_and_drag_follow_analytic_motion() {
    let mut p = project();
    let e = &mut p.layers[0].effects[0];
    e.params.get_mut("inherit_velocity").unwrap().track.value[0] = 1.;
    e.params.get_mut("gravity").unwrap().track.value = [0., 20., 0., 0.];
    e.params.get_mut("drag").unwrap().track.value[0] = 2.;
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    // Birth 1 at t=0.5 has dx/dt=64 px/s. At age=0.5, drag reduces its travel.
    let expected = 64. + 64. * (1. - (-1_f32).exp()) / 2.;
    assert!((xs(&b)[1] - expected).abs() < 0.02);
    let y = (1. - b.frame.sprites[1].rect[1]) * 64.;
    let expected_y = 64. + 20. * (0.5 - (1. - (-1_f32).exp()) / 2.) / 2.;
    assert!((y - expected_y).abs() < 0.02);
}
#[test]
fn camera_culling_keeps_births_and_reprojects_them() {
    let mut p = project();
    p.layers[0].three_d = true;
    p.camera.created = true;
    p.layers[0].transform.position.keys.clear();
    p.layers[0].transform.position.value = [64., 64., 0.];
    p.camera.position.value[0] += 10000.;
    p.camera.target.value[0] += 10000.;
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    assert_eq!(b.frame.generator_stats.alive, 3);
    assert!(b.frame.sprites.is_empty());
    p.camera.position.value[0] -= 10000.;
    p.camera.target.value[0] -= 10000.;
    sample(&p, &mut s, &mut b, 30.);
    assert_eq!(b.frame.generator_stats.visible, 3);
    assert_eq!(b.frame.generator_stats.births_sampled, 0);
}
#[test]
fn missing_source_and_history_expression_block_export_with_preview_fallback() {
    let mut p = project();
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = Some(99);
    let mut s = Scene::new(&p);
    let mut b = builder();
    s.sample(&p, 30., None).unwrap();
    assert!(b.build(&s, &[0], 128, 128, true).is_err());
    assert_eq!(
        b.build(&s, &[0], 128, 128, false)
            .unwrap()
            .diagnostics
            .len(),
        1
    );
    p.layers[0].effects[0].scene.as_mut().unwrap().source_layer = None;
    p.expressions.push(aem_core::PropertyExpression {
        target: aem_core::ExpressionTarget::Property {
            object: 1,
            property: aem_core::Property::Position,
            axis: None,
        },
        source: "[32 + time * 32, 64, 0]".into(),
        enabled: true,
        seed: 0,
        profile: aem_core::EXPRESSION_PROFILE.into(),
    });
    s.sample(&p, 30., None).unwrap();
    assert!(b
        .build(&s, &[0], 128, 128, true)
        .unwrap_err()
        .contains("expressions"));
    assert_eq!(
        b.build(&s, &[0], 128, 128, false)
            .unwrap()
            .diagnostics
            .len(),
        1
    );
}
#[test]
fn world_birth_particles_render_on_gpu_and_survive_undo_redo() {
    let p = project();
    let mut engine = aem_core::Engine::new(p.clone()).unwrap();
    let mut s = Scene::new(&p);
    let mut b = builder();
    sample(&p, &mut s, &mut b, 30.);
    let expected = b.frame.sprites.clone();
    engine
        .apply(aem_core::Command::Effect {
            object: 1,
            action: aem_core::EffectAction::Seed {
                effect: 1,
                seed: 123,
            },
        })
        .unwrap();
    engine.undo().unwrap();
    sample(engine.project(), &mut s, &mut b, 30.);
    assert_eq!(b.frame.sprites, expected);
    engine.redo().unwrap();
    assert_eq!(engine.project().layers[0].effects[0].seed, 123);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let target = r.capture_target(128, 128).unwrap();
    let (pixels, _) = r.capture(&s, &target).unwrap();
    for x in [32, 64, 96] {
        assert!(pixels[(64 * 128 + x) * 4 + 3] > 0, "missing birth at x={x}");
    }
}
#[test]
fn custom_png_sprite_keeps_aspect_alpha_and_shared_texture() {
    let mut p = project();
    p.assets.push(aem_core::Asset {
        id: 1,
        path: "assets/sprite.png".into(),
        width: 16,
        height: 8,
    });
    let e = &mut p.layers[0].effects[0];
    e.scene.as_mut().unwrap().sprite_asset = Some(1);
    e.params.get_mut("size").unwrap().track.value[0] = 16.;
    e.params.get_mut("end_size").unwrap().track.value[0] = 16.;
    let mut s = Scene::new(&p);
    s.sample(&p, 30., None).unwrap();
    let mut b = builder();
    b.build(&s, &[0, 1], 128, 128, true).unwrap();
    assert_eq!(b.frame.sprites[0].style[0], 6.);
    assert_eq!(b.frame.sprites[0].rect[2] / b.frame.sprites[0].rect[3], 2.);
    assert_eq!(b.frame.passes[0].input, -2);
    let mut rgba = vec![0; 16 * 8 * 4];
    for y in 0..4 {
        for x in 0..8 {
            rgba[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.upload_image(1, 16, 8, &rgba).unwrap();
    let target = r.capture_target(128, 128).unwrap();
    let (pixels, first) = r.capture(&s, &target).unwrap();
    assert!(pixels[(62 * 128 + 90) * 4 + 3] > 200);
    assert_eq!(pixels[(62 * 128 + 102) * 4 + 3], 0);
    let mut second = p.layers[0].clone();
    second.id = 2;
    p.layers.push(second);
    p.rebuild_plugin_dependencies();
    s.sample(&p, 30., None).unwrap();
    let (_, shared) = r.capture(&s, &target).unwrap();
    assert_eq!(first.texture_bytes, shared.texture_bytes);
    p.layers[0].effects[0].scene.as_mut().unwrap().sprite_asset = Some(99);
    s.sample(&p, 30., None).unwrap();
    assert!(b
        .build(&s, &[0, 1], 128, 128, true)
        .unwrap_err()
        .contains("sprite"));
}
#[test]
fn nested_png_emitter_preserves_births_pixels_and_random_seek() {
    let mut p = project();
    p.assets.push(aem_core::Asset {
        id: 1,
        path: "assets/sprite.png".into(),
        width: 8,
        height: 8,
    });
    p.layers[0].effects[0].scene.as_mut().unwrap().sprite_asset = Some(1);
    // Scene-generator precompose remains explicitly unsupported. Exercise a
    // generator authored inside a child, referenced through the supported API.
    let mut nested_project = p.clone();
    nested_project.compositions.push(aem_core::Composition {
        id: "comp-1".into(),
        name: "particles".into(),
        width: p.width,
        height: p.height,
        fps: p.fps,
        frames: p.frames,
        background: [0.; 4],
        camera: p.camera.clone(),
        layers: std::mem::take(&mut nested_project.layers),
        expressions: Vec::new(),
    });
    let mut engine = aem_core::Engine::new(nested_project).unwrap();
    engine.apply_batch(aem_core::parse_commands(r#"{"op":"composition","composition":"comp-main","action":{"kind":"reference","target":"comp-1"}}"#).unwrap()).unwrap();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    renderer.upload_image(1, 8, 8, &[255; 8 * 8 * 4]).unwrap();
    let target = renderer.capture_target(128, 128).unwrap();
    let mut plain = Scene::new(&p);
    let mut nested = Scene::new(engine.project());
    for frame in [30., 15., 35., 5., 30.] {
        plain.sample(&p, frame, None).unwrap();
        let expected = renderer.capture(&plain, &target).unwrap().0;
        nested.sample(engine.project(), frame, None).unwrap();
        assert_eq!(nested.nested.len(), 1);
        assert!(nested.nested[0].scene.effects[0].particle_history.is_some());
        assert_eq!(nested.nested[0].scene.sprite_assets[&1], [8, 8]);
        let actual = renderer.capture(&nested, &target).unwrap().0;
        let mae = expected
            .iter()
            .zip(&actual)
            .map(|(a, b)| a.abs_diff(*b) as f64)
            .sum::<f64>()
            / expected.len() as f64;
        assert!(
            mae <= 3.,
            "composition reference changed frame {frame}: {mae}"
        );
    }
    engine.undo().unwrap();
    assert!(engine.project().layers.is_empty());
    engine.redo().unwrap();
    let restored: Project =
        serde_json::from_slice(&serde_json::to_vec(engine.project()).unwrap()).unwrap();
    restored.validate().unwrap();
    nested.sample(&restored, 30., None).unwrap();
    assert_eq!(
        nested.nested[0].scene.effects[0]
            .scene
            .as_ref()
            .unwrap()
            .sprite_asset,
        Some(1)
    );
}
