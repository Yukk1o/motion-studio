use aem_core::{Asset, Content, Layer, Project, Scene};
use aem_render::image_resources::{self as images, Resolution, Source};
use std::{fs, io::Write, time::Instant};

fn png(root: &std::path::Path, name: &str, w: u32, h: u32, pixels: &[u8]) -> Source {
    fs::create_dir_all(root.join("assets")).unwrap();
    image::save_buffer(
        root.join(format!("assets/{name}")),
        pixels,
        w,
        h,
        image::ColorType::Rgba8,
    )
    .unwrap();
    Source::new(
        root,
        &Asset {
            id: 1,
            path: format!("assets/{name}"),
            width: w,
            height: h,
        },
    )
    .unwrap()
}

#[test]
fn upcoming_clips_map_nested_time_without_sampling_or_loading_media() {
    use aem_core::{Composition, CompositionClip, Content, Layer, LayerTimeline, Project};
    let mut p = Project::new(64, 64, 30, 90).unwrap();
    let clip = |id, asset, start, end| {
        let mut layer = Layer::solid(id, "image", [64.; 2], [0.; 3], [1.; 4]);
        layer.content = Content::Image { asset };
        layer.timeline = Some(LayerTimeline {
            in_frame: start,
            out_frame: end,
            offset_frame: 0,
        });
        layer
    };
    p.layers = vec![clip(1, 1, 0, 10), clip(2, 2, 10, 20), clip(5, 5, 31, 40)];
    let mut hidden = clip(6, 6, 1, 3);
    hidden.visible = false;
    p.layers.push(hidden);
    p.compositions.push(Composition {
        id: "child".into(),
        name: "child".into(),
        width: 64,
        height: 64,
        fps: 60,
        frames: 90,
        background: [0.; 4],
        camera: p.camera.clone(),
        layers: vec![clip(1, 3, 24, 40), clip(2, 4, 35, 45)],
        expressions: vec![],
    });
    let mut nested = clip(9, 0, 5, 30);
    nested.timeline.as_mut().unwrap().offset_frame = 5;
    let mut source = CompositionClip::new("child".into());
    source.source_start_frame = 15;
    nested.content = Content::Composition { clip: source };
    p.layers.push(nested);
    assert_eq!(images::upcoming_assets(&p, 0.), [1, 3, 2]);
    assert!(images::upcoming_assets(&p, f64::NAN).is_empty());
    assert!(images::upcoming_assets(&p, 89.).is_empty());
    // A negative child offset does not prefetch its frame zero before it enters
    // the lookahead interval; a two-level reference preserves the same rule.
    if let Content::Composition { clip } = &mut p.layers.last_mut().unwrap().content {
        clip.source_start_frame = -60;
    }
    assert_eq!(images::upcoming_assets(&p, 0.), [1, 2]);
}

#[test]
fn cancelling_a_decode_releases_its_only_job_without_delivering_stale_pixels() {
    let root = tempfile::tempdir().unwrap();
    let source = png(
        root.path(),
        "cancel.png",
        64,
        64,
        &[60, 20, 100, 255].repeat(64 * 64),
    );
    let mut worker = images::DecodeTask::default();
    worker
        .start(source.clone(), Resolution::Full, true)
        .unwrap();
    assert!(worker
        .start(source.clone(), Resolution::Full, false)
        .is_err());
    worker.cancel();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some((_, _, speculative, result)) = worker.poll() {
            assert!(speculative);
            assert_eq!(result.err().unwrap(), "image decode superseded");
            assert!(!worker.busy());
            break;
        }
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    worker.start(source, Resolution::Full, false).unwrap();
    loop {
        if let Some((_, _, speculative, result)) = worker.poll() {
            assert!(!speculative);
            assert_eq!(result.unwrap().rgba.len(), 64 * 64 * 4);
            break;
        }
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn box_proxy_averages_linear_premultiplied_color_and_preserves_original() {
    let root = tempfile::tempdir().unwrap();
    let pixels = [
        255, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 255,
    ];
    let source = png(root.path(), "edge.png", 4, 1, &pixels);
    let before = fs::read(&source.path).unwrap();
    let proxy = images::decode(&source, Resolution::Preview(2)).unwrap();
    assert_eq!((proxy.width, proxy.height), (2, 1));
    assert_eq!(proxy.rgba, [188, 0, 0, 128, 0, 0, 255, 255]);
    let full = images::decode(&source, Resolution::Full).unwrap();
    assert_eq!(
        full.rgba,
        [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255]
    );
    assert_eq!(fs::read(&source.path).unwrap(), before);
    let cached = images::decode(&source, Resolution::Preview(2)).unwrap();
    assert!(cached.cached);
    assert_eq!(cached.rgba, proxy.rgba);
    assert!(!images::decode(&source, Resolution::Full).unwrap().cached);
}

#[test]
fn odd_sizes_conserve_every_source_pixel_and_proxies_do_not_upscale() {
    let root = tempfile::tempdir().unwrap();
    let mut data = vec![0; 7 * 5 * 4];
    for p in data.chunks_exact_mut(4) {
        p.copy_from_slice(&[37, 113, 241, 255]);
    }
    let source = png(root.path(), "odd.png", 7, 5, &data);
    for edge in [1, 2, 3, 4, 5, 6, 7, 8, 2048] {
        let proxy = images::decode(&source, Resolution::Preview(edge)).unwrap();
        assert!(proxy.width <= 7 && proxy.height <= 5);
        assert!(proxy.rgba.chunks_exact(4).all(|p| p == [37, 113, 241, 255]));
    }
}

#[test]
fn direct_decode_checks_lengths_metadata_and_truncation_before_trusting_data() {
    let root = tempfile::tempdir().unwrap();
    let source = png(root.path(), "source.png", 2, 1, &[255; 8]);
    assert!(images::decode_into(&source, Resolution::Full, &mut [0; 7]).is_err());
    let mut mismatch = source.clone();
    mismatch.width = 3;
    assert!(images::decode(&mismatch, Resolution::Full).is_err());
    let original = fs::read(&source.path).unwrap();
    fs::write(&source.path, &original[..original.len() - 8]).unwrap();
    assert!(images::decode(&source, Resolution::Full).is_err());
    assert!(Source::new(
        root.path(),
        &Asset {
            id: 2,
            path: "assets/../escape.png".into(),
            width: 1,
            height: 1
        }
    )
    .is_err());
}

#[test]
fn cache_detects_corruption_and_source_changes_and_keeps_a_disk_budget() {
    let root = tempfile::tempdir().unwrap();
    let source = png(root.path(), "source.png", 4, 1, &[255; 16]);
    let first = images::decode(&source, Resolution::Preview(2)).unwrap();
    assert!(!first.cached);
    let dir = root.path().join(".cache/image-proxies-v1");
    let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let mut bytes = fs::read(&file).unwrap();
    bytes[12] = 0;
    fs::write(&file, &bytes).unwrap();
    let repaired = images::decode(&source, Resolution::Preview(2)).unwrap();
    assert!(!repaired.cached);
    assert_eq!(repaired.rgba, first.rgba);
    assert!(
        images::decode(&source, Resolution::Preview(2))
            .unwrap()
            .cached
    );
    png(root.path(), "source.png", 4, 1, &[0, 0, 255, 255].repeat(4));
    let changed = images::decode(&source, Resolution::Preview(2)).unwrap();
    assert!(!changed.cached);
    assert_eq!(changed.rgba, [0, 0, 255, 255].repeat(2));
    for i in 1..=4 {
        let path = dir.join(format!("{i:064x}.rgba"));
        fs::File::create(path)
            .unwrap()
            .set_len(17 * 1024 * 1024)
            .unwrap();
    }
    // A new resolution triggers cache maintenance; cache stays disposable.
    images::decode(&source, Resolution::Preview(1)).unwrap();
    let total: u64 = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(total <= images::PROXY_CACHE_BYTES);
    assert!(fs::read_dir(dir)
        .unwrap()
        .all(|e| e.unwrap().path().extension().unwrap() == "rgba"));
}

#[test]
fn streamed_large_png_import_can_prepare_a_proxy_without_a_full_rgba_allocation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    let w = 7952;
    let h = 3273;
    let file = fs::File::create(root.path().join("assets/large.png")).unwrap();
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    {
        let mut stream = writer.stream_writer().unwrap();
        let row = [31, 129, 221, 255].repeat(w as usize);
        for _ in 0..h {
            stream.write_all(&row).unwrap();
        }
        stream.finish().unwrap();
    }
    writer.finish().unwrap();
    let source = Source::new(
        root.path(),
        &Asset {
            id: 1,
            path: "assets/large.png".into(),
            width: w,
            height: h,
        },
    )
    .unwrap();
    let started = Instant::now();
    let proxy = images::decode(&source, Resolution::Preview(2048)).unwrap();
    println!(
        "7952x3273 proxy: {}x{}, {} bytes, {} ms",
        proxy.width,
        proxy.height,
        proxy.rgba.len(),
        started.elapsed().as_millis()
    );
    assert_eq!((proxy.width, proxy.height), (2048, 843));
    assert!(proxy.rgba.chunks_exact(4).all(|p| p == [31, 129, 221, 255]));
    assert_eq!(
        fs::metadata(source.path).unwrap().len(),
        images::inspect(&root.path().join("assets/large.png"))
            .unwrap()
            .3
    );
}

#[test]
fn scene_demand_excludes_inactive_clips_and_unreferenced_library_assets() {
    let mut p = Project::new(64, 64, 30, 20).unwrap();
    for id in 1..=3 {
        p.assets.push(Asset {
            id,
            path: format!("assets/{id}.png"),
            width: 2,
            height: 2,
        });
    }
    let mut l = Layer::solid(1, "image", [64.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Image { asset: 2 };
    let mut timeline = l.clip(20);
    timeline.in_frame = 5;
    timeline.out_frame = 10;
    l.timeline = Some(timeline);
    p.layers.push(l);
    let mut scene = Scene::new(&p);
    for (frame, count) in [(0., 0), (5., 1), (9., 1), (10., 0)] {
        scene.sample(&p, frame, None).unwrap();
        let demand = images::scene_assets(&scene);
        assert_eq!(demand.len(), count);
        if count > 0 {
            assert!(demand.contains(&2));
        }
    }
}

#[test]
#[ignore = "requires MOTION_IMAGE_SOURCE outside the repository"]
fn optional_local_original_image_probe() {
    let Some(path) = std::env::var_os("MOTION_IMAGE_SOURCE") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let root = tempfile::tempdir().unwrap();
    let (width, height, _, bytes) = images::inspect(&path).unwrap();
    let source = Source {
        id: 1,
        path,
        width,
        height,
        root: root.path().canonicalize().unwrap(),
    };
    let started = Instant::now();
    let proxy = images::decode(&source, Resolution::Preview(2048)).unwrap();
    let cold = started.elapsed().as_millis();
    let started = Instant::now();
    let warm = images::decode(&source, Resolution::Preview(2048)).unwrap();
    let warm_ms = started.elapsed().as_millis();
    assert!(warm.cached);
    assert_eq!(warm.rgba, proxy.rgba);
    println!("local original {width}x{height}, encoded={bytes}, proxy={}x{}, rgba={}, cold_ms={cold}, warm_ms={warm_ms}",proxy.width,proxy.height,proxy.rgba.len());
}

#[test]
fn adam7_full_and_proxy_match_noninterlaced_sampling() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    fs::write(
        root.path().join("assets/adam7.png"),
        include_bytes!("fixtures/image-resources-adam7.png"),
    )
    .unwrap();
    let source = Source::new(
        root.path(),
        &Asset {
            id: 1,
            path: "assets/adam7.png".into(),
            width: 7,
            height: 5,
        },
    )
    .unwrap();
    let original = image::open(&source.path).unwrap().into_rgba8();
    let plain = png(root.path(), "plain.png", 7, 5, original.as_raw());
    for mode in [
        Resolution::Full,
        Resolution::Preview(1),
        Resolution::Preview(3),
        Resolution::Preview(5),
    ] {
        let interlaced = images::decode(&source, mode).unwrap();
        let ordinary = images::decode(&plain, mode).unwrap();
        assert_eq!(interlaced.rgba, ordinary.rgba, "Adam7 differs for {mode:?}");
    }
}

#[test]
fn png_color_types_palette_transparency_and_16_bit_conversion_match_existing_contract() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    for (i, color, depth, bytes) in [
        (
            1,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            vec![200, 30, 80, 0, 250, 180],
        ),
        (
            2,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            vec![37, 244],
        ),
        (
            3,
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            vec![37, 128, 244, 255],
        ),
        (4, png::ColorType::Indexed, png::BitDepth::Eight, vec![0, 1]),
        (
            5,
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            vec![
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xff, 0xff, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
                0x80, 0x80,
            ],
        ),
    ] {
        let path = format!("assets/{i}.png");
        let file = fs::File::create(root.path().join(&path)).unwrap();
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![200, 30, 80, 0, 250, 180]);
            encoder.set_trns(vec![128, 0]);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&bytes).unwrap();
        writer.finish().unwrap();
        let mut expected = image::open(root.path().join(&path))
            .unwrap()
            .into_rgba8()
            .into_raw();
        aem_render::premultiply_pixels(&mut expected);
        let source = Source::new(
            root.path(),
            &Asset {
                id: i,
                path,
                width: 2,
                height: 1,
            },
        )
        .unwrap();
        assert_eq!(
            images::decode(&source, Resolution::Full).unwrap().rgba,
            expected,
            "PNG color {color:?} depth {depth:?}"
        );
    }
}
