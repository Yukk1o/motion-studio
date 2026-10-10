use motion_core::{Asset, Content, Layer, LayerTimeline, Project, Scene};
use motion_render::{
    image_resources::{self, Resolution},
    ChromaLayout, Renderer, Yuv420Frame,
};
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn layer(id: u64, asset: u64, start: u32, end: u32) -> Layer {
    let mut l = Layer::solid(id, "image", [64.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Image { asset };
    l.timeline = Some(LayerTimeline {
        in_frame: start,
        out_frame: end,
        offset_frame: 0,
    });
    l
}
fn fixture(root: &std::path::Path, id: u64, size: u32) -> Asset {
    let path = format!("assets/{id}.png");
    let mut encoder = png::Encoder::new(fs::File::create(root.join(&path)).unwrap(), size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    {
        let mut stream = writer.stream_writer().unwrap();
        let row = [id as u8 * 20, 50, 160, 255].repeat(size as usize);
        for _ in 0..size {
            stream.write_all(&row).unwrap();
        }
        stream.finish().unwrap();
    }
    writer.finish().unwrap();
    Asset {
        id,
        path,
        width: size,
        height: size,
    }
}

#[test]
fn recent_textures_are_lru_bounded_and_yuv_allocations_reclaim_idle_images() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    let mut p = Project::new(64, 64, 30, 30).unwrap();
    p.assets = (1..=4).map(|id| fixture(root.path(), id, 2048)).collect();
    p.layers = vec![layer(1, 1, 0, 30)];
    let mut scene = Scene::new(&p);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.configure_assets(&p, root.path()).unwrap();
    let target = r.capture_target(64, 64).unwrap();
    let mut colors = vec![];
    for asset in [1, 2, 1, 3, 4] {
        p.layers[0].content = Content::Image { asset };
        scene.sample(&p, 0., None).unwrap();
        assert!(r
            .prepare_scene_assets(&scene, Resolution::Preview(2048), false)
            .unwrap());
        assert!(r.image_idle_bytes() <= image_resources::IDLE_TEXTURE_BYTES);
        colors.push(r.capture(&scene, &target).unwrap().0[(32 * 64 + 32) * 4]);
    }
    assert_eq!(
        colors[0], colors[2],
        "cached pixels changed after reverse seek"
    );
    assert_ne!(colors[0], colors[1]);
    assert_eq!(r.image_decodes, 4);
    assert_eq!(r.image_memory_cache_hits, 1);
    assert_eq!(r.image_dimensions(2), None, "LRU evicted a newer image");
    assert_eq!(r.image_dimensions(1), Some((2048, 2048)));
    assert_eq!(r.image_idle_bytes(), 32 * 1024 * 1024);
    // A public in-place upload can replace an idle texture. Reclaim another
    // idle texture, preserving correct replacement accounting for the old one.
    r.upload_image(1, 6144, 4096, &[40, 50, 160, 255].repeat(6144 * 4096))
        .unwrap();
    assert_eq!(r.image_dimensions(3), None);
    assert_eq!(r.texture_bytes(), 112 * 1024 * 1024 + 4);
    // Return to the registered dimensions and restore the recent working set.
    p.layers[0].content = Content::Image { asset: 3 };
    scene.sample(&p, 0., None).unwrap();
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    p.layers[0].content = Content::Image { asset: 1 };
    scene.sample(&p, 0., None).unwrap();
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    p.layers[0].content = Content::Image { asset: 4 };
    scene.sample(&p, 0., None).unwrap();
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    // 4K output + decoder planes = 88 MiB. With three images it would exceed
    // 128 MiB: reclaim an idle image, never reject a valid active video instead.
    let video = Yuv420Frame {
        width: 4096,
        height: 4096,
        rotation: 0,
        standard: 1,
        range: 2,
        phase: [0, 0],
        chroma_layout: ChromaLayout::Uv,
        y: vec![128; 4096 * 4096],
        uv: vec![128; 4096 * 4096 / 2],
    };
    r.upload_video_yuv(100, 200, 0, &video).unwrap();
    assert_eq!(r.image_dimensions(3), None);
    assert_eq!(r.image_dimensions(1), Some((2048, 2048)));
    assert_eq!(
        r.image_dimensions(4),
        Some((2048, 2048)),
        "evicted the active image"
    );
    assert!(r.texture_bytes() <= 128 * 1024 * 1024);
    r.prepare_scene_assets(&scene, Resolution::Full, false)
        .unwrap();
    assert_eq!(
        r.image_idle_bytes(),
        0,
        "formal output kept a preview working set"
    );
    assert_eq!(r.image_dimensions(1), None);
    r.clear_assets();
    assert_eq!(r.image_count(), 1);
}

#[test]
fn prefetched_clip_renders_immediately_and_invalid_future_source_only_fails_when_active() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    let mut p = Project::new(64, 64, 30, 30).unwrap();
    p.assets = (1..=2).map(|id| fixture(root.path(), id, 64)).collect();
    fs::write(root.path().join("assets/bad.png"), b"invalid PNG").unwrap();
    p.assets.push(Asset {
        id: 9,
        path: "assets/bad.png".into(),
        width: 64,
        height: 64,
    });
    p.layers = vec![layer(1, 1, 0, 15), layer(2, 2, 15, 30), layer(9, 9, 18, 30)];
    let mut scene = Scene::new(&p);
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.configure_assets(&p, root.path()).unwrap();
    scene.sample(&p, 1., None).unwrap();
    r.set_image_prefetch(image_resources::upcoming_assets(&p, 1.));
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    r.prefetch_scene_assets(Resolution::Preview(2048)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while r.image_dimensions(2).is_none() {
        assert!(r
            .prepare_scene_assets(&scene, Resolution::Preview(2048), true)
            .unwrap());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let uploaded = r.image_upload_bytes;
    scene.sample(&p, 16., None).unwrap();
    r.set_image_prefetch(image_resources::upcoming_assets(&p, 16.));
    assert!(r
        .prepare_scene_assets(&scene, Resolution::Preview(2048), true)
        .unwrap());
    assert_eq!(r.image_upload_bytes, uploaded);
    assert_eq!(r.image_memory_cache_hits, 1);
    assert_eq!(r.image_prefetches, 1);
    for _ in 0..100 {
        r.prefetch_scene_assets(Resolution::Preview(2048)).unwrap();
        assert!(r
            .prepare_scene_assets(&scene, Resolution::Preview(2048), true)
            .unwrap());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        r.image_prefetches, 2,
        "failed speculation retried every render"
    );
    scene.sample(&p, 18., None).unwrap();
    loop {
        match r.prepare_scene_assets(&scene, Resolution::Preview(2048), true) {
            Err(error) => {
                assert!(error.to_string().contains("image 9"));
                break;
            }
            Ok(ready) => assert!(!ready, "ignored an invalid active image"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
