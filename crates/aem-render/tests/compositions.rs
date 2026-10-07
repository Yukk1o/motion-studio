use aem_core::*;
use aem_render::{composition_plan, effect_plan::PlanBuilder, Renderer};
use serde_json::json;
fn apply(e: &mut Engine, c: &str, a: serde_json::Value) {
    e.apply_batch(
        parse_commands(&json!({"op":"composition","composition":c,"action":a}).to_string())
            .unwrap(),
    )
    .unwrap();
}
fn fixture() -> Project {
    let mut p = Project::new(64, 64, 30, 120).unwrap();
    p.background = [0.03, 0.04, 0.06, 1.];
    p.layers = vec![
        Layer::solid(1, "red top", [40., 20.], [26., 12., 0.], [1., 0., 0., 0.6]),
        Layer::solid(
            2,
            "blue bottom",
            [30., 30.],
            [44., 48., 0.],
            [0., 0., 1., 0.7],
        ),
    ];
    p.layers[0]
        .transform
        .position
        .upsert(0, [26., 12., 0.], Ease::Linear)
        .unwrap();
    p.layers[0]
        .transform
        .position
        .upsert(80, [38., 20., 0.], Ease::InOut)
        .unwrap();
    p.layers[1].timeline = Some(LayerTimeline {
        in_frame: 12,
        out_frame: 115,
        offset_frame: 5,
    });
    p
}
fn capture(r: &mut Renderer, p: &Project, f: f64) -> Vec<u8> {
    let target = r.capture_target(p.width, p.height).unwrap();
    let mut s = Scene::new(p);
    s.sample(p, f, None).unwrap();
    r.retain_video_instances(&s);
    r.capture(&s, &target).unwrap().0
}
#[test]
fn nested_gpu_preserves_pixels_at_keys_and_random_seek_and_reuses_outputs() {
    let mut e = Engine::new(fixture()).unwrap();
    let before = e.snapshot();
    apply(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[1,2],"name":"inside"}),
    );
    apply(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[3],"name":"outside"}),
    );
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    for frame in [0., 12., 80., 99., 4., 80., 119., 40.5] {
        let a = capture(&mut r, &before, frame);
        let b = capture(&mut r, e.project(), frame);
        let mae = a
            .iter()
            .zip(&b)
            .map(|(a, b)| a.abs_diff(*b) as f64)
            .sum::<f64>()
            / a.len() as f64;
        assert!(mae <= 3., "frame {frame}: mean pixel error {mae}");
        let worst = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 4, "frame {frame}: worst {worst}");
    }
    let bytes = r.texture_bytes();
    capture(&mut r, e.project(), 40.5);
    assert_eq!(r.texture_bytes(), bytes);
    let mut p = e.snapshot();
    p.layers[0].three_d = true;
    p.layers[0].transform.rotation.value[1] = 35.;
    assert_ne!(capture(&mut r, &p, 50.), capture(&mut r, e.project(), 50.));
    capture(&mut r, &before, 40.5);
    assert!(r.texture_bytes() < bytes);
}
#[test]
fn nested_effects_and_postorder_bundle_keep_program_and_parent_texture_bindings() {
    let mut p = fixture();
    let package = aem_effects::builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "tint")
        .unwrap();
    p.layers[0].effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        [64.; 2],
    ));
    p.rebuild_plugin_dependencies();
    let before = p.clone();
    let mut e = Engine::new(p).unwrap();
    apply(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[1,2],"name":"child"}),
    );
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let a = capture(&mut r, &before, 40.);
    let b = capture(&mut r, e.project(), 40.);
    assert!(
        a.iter()
            .zip(&b)
            .map(|(a, b)| a.abs_diff(*b) as f64)
            .sum::<f64>()
            / a.len() as f64
            <= 3.
    );
    let mut scene = Scene::new(e.project());
    scene.sample(e.project(), 40., None).unwrap();
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let mut bytes = Vec::new();
    composition_plan::build(&mut builder, &scene, e.project(), &[0], &mut bytes).unwrap();
    let word = |n: usize| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap());
    assert_eq!(word(0), composition_plan::MAGIC);
    assert_eq!(word(8), 2);
    assert_eq!(word(16), 1);
    assert_eq!(word(32 + 8), 1);
    let root = 32 + 80;
    let plan = word(root + 40) as usize;
    let draws = word(plan + 16) as usize;
    assert_eq!(
        f32::from_le_bytes(
            bytes[plan + draws + 30 * 4..plan + draws + 31 * 4]
                .try_into()
                .unwrap()
        ),
        2.
    );
}

#[test]
fn switching_contexts_can_reuse_an_id_for_video_and_composition_without_reusing_texture_usage() {
    let mut e = Engine::new(fixture()).unwrap();
    apply(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[1,2],"name":"child"}),
    );
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    let expected = capture(&mut r, e.project(), 20.);
    let mut scene = Scene::new(e.project());
    scene.sample(e.project(), 20., None).unwrap();
    let id = scene.layers[0].id;
    scene.nested.clear();
    scene.layers[0].composition = false;
    scene.layers[0].video = Some(VideoSample {
        asset: 7,
        source_time_us: 0,
    });
    r.retain_video_instances(&scene);
    let pixels = [0, 255, 0, 255].repeat(64 * 64);
    r.upload_video_frame(id, 7, 0, 64, 64, &pixels).unwrap();
    let target = r.capture_target(64, 64).unwrap();
    let (pixels, _) = r.capture(&scene, &target).unwrap();
    assert_eq!(
        &pixels[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4],
        &[0, 255, 0, 255]
    );
    assert_eq!(capture(&mut r, e.project(), 20.), expected);
}
