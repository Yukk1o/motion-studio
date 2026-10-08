use aem_core::{Asset, Composition, CompositionClip, Content, Layer, Project, Scene};
use aem_render::{image_resources::Resolution, Renderer};
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn image_layer(id: u64, asset: u64) -> Layer {
    let mut l = Layer::solid(id, "image", [64.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Image { asset };
    l
}

#[test]
fn particle_sprite_is_demanded_without_an_image_layer_and_released_when_disabled() {
    let package = aem_effects::builtin::particle_package().unwrap();
    let mut project = Project::new(1080, 1920, 30, 60).unwrap();
    project.assets.push(Asset {
        id: 7,
        path: "assets/sprite.png".into(),
        width: 16,
        height: 8,
    });
    let mut layer = Layer::solid(1, "Particles", [1080., 1920.], [540., 960., 0.], [1.; 4]);
    let mut effect = aem_core::EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        &package.manifest.effects[0],
        layer.size,
    );
    effect.scene.as_mut().unwrap().sprite_asset = Some(7);
    layer.effects.push(effect);
    project.layers.push(layer);
    project.rebuild_plugin_dependencies();
    project.validate().unwrap();
    let mut scene = Scene::new(&project);
    scene.sample(&project, 0., None).unwrap();
    assert!(scene.layers.iter().all(|layer| layer.asset != Some(7)));
    assert!(aem_render::image_resources::scene_assets(&scene).contains(&7));

    project.layers[0].effects[0].enabled = false;
    scene.sample(&project, 1., None).unwrap();
    assert!(!aem_render::image_resources::scene_assets(&scene).contains(&7));
}
#[test]
fn active_images_share_proxies_with_nested_instances_and_full_output_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    let file = fs::File::create(root.path().join("assets/one.png")).unwrap();
    let mut encoder = png::Encoder::new(file, 4096, 4096);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    {
        let mut stream = writer.stream_writer().unwrap();
        let row = [25, 117, 227, 255].repeat(4096);
        for _ in 0..4096 {
            stream.write_all(&row).unwrap();
        }
        stream.finish().unwrap();
    }
    writer.finish().unwrap();
    fs::copy(
        root.path().join("assets/one.png"),
        root.path().join("assets/two.png"),
    )
    .unwrap();
    fs::copy(
        root.path().join("assets/one.png"),
        root.path().join("assets/unused.png"),
    )
    .unwrap();
    let mut p = Project::new(64, 64, 30, 30).unwrap();
    p.background = [0., 0., 0., 1.];
    p.assets = vec![(99, "unused.png"), (7, "one.png"), (8, "two.png")]
        .into_iter()
        .map(|(id, name)| Asset {
            id,
            path: format!("assets/{name}"),
            width: 4096,
            height: 4096,
        })
        .collect();
    p.layers = vec![image_layer(1, 7), image_layer(2, 7)];
    let package = aem_effects::builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "tint")
        .unwrap();
    let mut nested_image = image_layer(1, 7);
    nested_image.effects.push(aem_core::EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        [64.; 2],
    ));
    p.compositions.push(Composition {
        id: "comp-child".into(),
        name: "comp-child".into(),
        width: 64,
        height: 64,
        fps: 30,
        frames: 30,
        background: [0.; 4],
        camera: p.camera.clone(),
        layers: vec![nested_image],
        expressions: vec![],
    });
    let mut child = Layer::solid(3, "nested", [64.; 2], [32., 32., 0.], [1.; 4]);
    child.content = Content::Composition {
        clip: CompositionClip::new("comp-child".into()),
    };
    p.layers.push(child);
    p.rebuild_plugin_dependencies();
    p.validate().unwrap();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0., None).unwrap();
    let mut r = pollster::block_on(Renderer::headless()).unwrap();
    r.configure_assets(&p, root.path()).unwrap();
    assert_eq!(r.image_count(), 1);
    assert_eq!(r.asset_ids(), [0, 99, 7, 8]);
    let start = Instant::now();
    while !r
        .prepare_scene_assets(&scene, Resolution::Preview(2048), true)
        .unwrap()
    {
        assert!(start.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(r.image_dimensions(7), Some((2048, 2048)));
    assert_eq!(r.image_dimensions(99), None);
    assert_eq!(r.image_count(), 2);
    assert_eq!(r.image_decodes, 1);
    let target = r.capture_target(64, 64).unwrap();
    let (proxy, _) = r.capture(&scene, &target).unwrap();
    let center = &proxy[(32 * 64 + 32) * 4..(32 * 64 + 33) * 4];
    assert_eq!(center[0], center[1]);
    assert_eq!(
        center[1], center[2],
        "nested Tint was not applied to the image"
    );
    for frame in [1., 12., 4., 29., 0.] {
        scene.sample(&p, frame, None).unwrap();
        assert!(r
            .prepare_scene_assets(&scene, Resolution::Preview(2048), true)
            .unwrap());
        let (pixels, _) = r.capture(&scene, &target).unwrap();
        assert_eq!(pixels, proxy);
    }
    assert_eq!(
        r.image_decodes, 1,
        "steady playback decoded the source again"
    );
    r.prepare_scene_assets(&scene, Resolution::Full, false)
        .unwrap();
    assert_eq!(r.image_dimensions(7), Some((4096, 4096)));
    assert_eq!(r.capture(&scene, &target).unwrap().0, proxy);
    // 2 x 64 MiB plus the solid texture does not fit: never downsample export.
    p.layers[1].content = Content::Image { asset: 8 };
    scene.sample(&p, 0., None).unwrap();
    assert!(r
        .prepare_scene_assets(&scene, Resolution::Full, false)
        .unwrap_err()
        .to_string()
        .contains("working set"));
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    assert_eq!(r.image_dimensions(7), Some((2048, 2048)));
    assert_eq!(r.image_dimensions(8), Some((2048, 2048)));
    assert_eq!(r.image_dimensions(99), None);
    assert_eq!(r.asset_ids(), [0, 99, 7, 8]);
    // Drop every reference: the proxy stays in the bounded idle GPU cache.
    p.layers.retain(|l| l.id == 2);
    scene.sample(&p, 0., None).unwrap();
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    assert_eq!(r.image_dimensions(7), Some((2048, 2048)));
    p.layers.push(image_layer(4, 7));
    scene.sample(&p, 0., None).unwrap();
    let hits = r.image_memory_cache_hits;
    let uploaded = r.image_upload_bytes;
    r.prepare_scene_assets(&scene, Resolution::Preview(2048), false)
        .unwrap();
    assert!(
        r.image_memory_cache_hits > hits,
        "reverse seek did not reuse the resident proxy"
    );
    assert_eq!(r.image_upload_bytes, uploaded);
    // Project/catalog replacement must not reuse a video stamped with the same
    // object/source/PTS from a different project directory.
    r.upload_video_frame(100, 200, 0, 1, 1, &[255, 0, 0, 255])
        .unwrap();
    let uploads = r.video_uploads;
    r.configure_assets(&p, root.path()).unwrap();
    r.upload_video_frame(100, 200, 0, 1, 1, &[0, 0, 255, 255])
        .unwrap();
    assert_eq!(r.video_uploads, uploads + 1);
}
