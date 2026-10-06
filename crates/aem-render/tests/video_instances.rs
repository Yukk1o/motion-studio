use aem_core::{Content, EffectInstance, Layer, LayerTimeline, Project, Scene, VideoAsset, VideoClip};
use aem_render::Renderer;
fn fixture() -> Project {
    let mut p = Project::new(64, 32, 60, 120).unwrap();
    p.background = [0., 0., 0., 1.];
    p.video_assets.push(VideoAsset {
        id: 1,
        path: "assets/video.mp4".into(),
        bytes: 1,
        mime: "video/avc".into(),
        track: 0,
        width: 16,
        height: 16,
        rotation: 0,
        display_width: 16,
        display_height: 16,
        video_start_us: 0,
        video_end_us: 2_000_000,
        duration_us: 2_000_000,
        frame_count: 48,
        variable_frame_rate: false,
        nominal_frame_rate: 24.,
        color_standard: 1,
        color_range: 2,
        audio_asset: None,
    });
    for (id, x) in [(1, 16.), (2, 48.)] {
        let mut l = Layer::solid(id, "video", [32., 32.], [x, 16., 0.], [1.; 4]);
        l.content = Content::Video {
            video: VideoClip::new(1),
        };
        l.timeline = Some(LayerTimeline::full(120));
        p.layers.push(l);
    }
    p.validate().unwrap();
    p
}
#[test]
fn different_source_times_use_distinct_video_textures_and_reuse_allocations() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let p = fixture();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let red = [255, 0, 0, 255].repeat(256);
    let green = [0, 255, 0, 255].repeat(256);
    renderer.upload_video_frame(1, 1, 0, 16, 16, &red).unwrap();
    renderer
        .upload_video_frame(2, 1, 500000, 16, 16, &green)
        .unwrap();
    let target = renderer.capture_target(64, 32).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(
        &out[(16 * 64 + 16) * 4..(16 * 64 + 16) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(
        &out[(16 * 64 + 48) * 4..(16 * 64 + 48) * 4 + 4],
        &[0, 255, 0, 255]
    );
    let bytes = renderer.texture_bytes();
    let count = renderer.image_count();
    for _ in 0..100 {
        renderer.upload_video_frame(1, 1, 0, 16, 16, &red).unwrap();
        renderer
            .upload_video_frame(2, 1, 500000, 16, 16, &green)
            .unwrap();
    }
    assert_eq!(bytes, renderer.texture_bytes());
    assert_eq!(count, renderer.image_count());
    assert_eq!(count, 3);
    renderer
        .upload_video_frame(1, 1, 1000000, 16, 16, &green)
        .unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(
        &out[(16 * 64 + 16) * 4..(16 * 64 + 16) * 4 + 4],
        &[0, 255, 0, 255]
    );
    assert_eq!(bytes, renderer.texture_bytes());
}

fn effect(name: &str) -> EffectInstance {
    let package = aem_effects::builtin::package().unwrap();
    let definition = package.manifest.effects.iter().find(|e| e.id == name).unwrap();
    EffectInstance::new(1, &package.manifest.id, &package.manifest.version,
        &package.hash, definition, [32.; 2])
}
#[test]
fn video_effects_keep_live_instance_pixels_and_rebind_after_visibility_changes() {
    let mut p = fixture();
    for layer in &mut p.layers {
        layer.effects.push(effect("brightness_contrast"));
    }
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(64, 32).unwrap();
    let mut scene = Scene::new(&p);
    let red = [255, 0, 0, 255].repeat(256);
    let green = [0, 255, 0, 255].repeat(256);
    let blue = [0, 0, 255, 255].repeat(256);
    let pixel = |out: &[u8], x: usize| out[(16 * 64 + x) * 4..(16 * 64 + x) * 4 + 4].to_vec();
    renderer.upload_video_frame(1, 1, 0, 16, 16, &red).unwrap();
    renderer.upload_video_frame(2, 1, 500_000, 16, 16, &green).unwrap();
    scene.sample(&p, 0., None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(pixel(&out, 16), [255, 0, 0, 255]);
    assert_eq!(pixel(&out, 48), [0, 255, 0, 255]);
    renderer.upload_video_frame(1, 1, 1_000_000, 16, 16, &blue).unwrap();
    renderer.upload_video_frame(2, 1, 1_500_000, 16, 16, &red).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(pixel(&out, 16), [0, 0, 255, 255]);
    assert_eq!(pixel(&out, 48), [255, 0, 0, 255]);
    // The remaining instance now occupies the first pass's cached binding index.
    p.layers[0].visible = false;
    scene.sample(&p, 0., None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_eq!(pixel(&out, 16), [0, 0, 0, 255]);
    assert_eq!(pixel(&out, 48), [255, 0, 0, 255]);
    p.layers[0].visible = true;
    p.layers[0].effects[0] = effect("tint");
    p.rebuild_plugin_dependencies();
    scene.sample(&p, 0., None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    let tinted = pixel(&out, 16);
    assert!(tinted[0].abs_diff(29) <= 3 && tinted[0] == tinted[1] && tinted[1] == tinted[2]);
    assert_eq!(pixel(&out, 48), [255, 0, 0, 255]);
}
