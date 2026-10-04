use aem_core::{Asset, Content, Layer, Project, Scene};
use aem_render::Renderer;

fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
    rgba[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn assert_color(actual: [u8; 4], expected: [u8; 4]) {
    for i in 0..4 {
        assert!(
            actual[i].abs_diff(expected[i]) <= 3,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn gpu_sorting_transparency_png_orientation_and_resource_reuse() {
    // A missing GPU is a test failure, never a silent skip.
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    println!(
        "GPU acceptance adapter: {} / {:?}",
        renderer.adapter_info.name, renderer.adapter_info.backend
    );
    let mut p = Project::new(64, 64, 30, 180).unwrap();
    p.background = [0.0, 0.0, 0.0, 1.0];
    p.layers = vec![
        Layer::solid(1, "red", [64.0; 2], [32.0, 32.0, 0.0], [1.0, 0.0, 0.0, 1.0]),
        Layer::solid(
            2,
            "blue",
            [64.0; 2],
            [32.0, 32.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
        ),
    ];
    let target = renderer.capture_target(64, 64).unwrap();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 32, 32), [0, 0, 255, 255]);
    p.layers.swap(0, 1);
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 32, 32), [255, 0, 0, 255]);
    p.layers[0].transform.position.value[2] = -10.0; // blue nearer, despite stack order
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 32, 32), [0, 0, 255, 255]);
    p.layers[0].transform.opacity.value = 0.5;
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 32, 32), [188, 0, 188, 255]); // linear-space alpha composition

    // A 2x2 RGBA source verifies UV orientation and transparent source pixels.
    p.layers.truncate(1);
    p.layers[0].transform.position.value[2] = 0.0;
    p.layers[0].transform.opacity.value = 1.0;
    p.layers[0].content = Content::Image { asset: 7 };
    let data = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
    ];
    renderer.upload_image(7, 2, 2, &data).unwrap();
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 1, 1), [255, 0, 0, 255]);
    assert_color(pixel(&out, 62, 1), [0, 255, 0, 255]);
    assert_color(pixel(&out, 1, 62), [0, 0, 255, 255]);
    assert_color(pixel(&out, 62, 62), [0, 0, 0, 255]);
    // A transparent white texel must not tint blue when linear filtering crosses its edge.
    let edge = pixel(&out, 32, 62);
    assert!(
        edge[0] <= 2 && edge[1] <= 2 && edge[2] > 0,
        "transparent RGB fringe: {edge:?}"
    );
    let bytes = renderer.texture_bytes();
    let count = renderer.image_count();
    for frame in 0..20 {
        scene.sample(&p, frame as f64, None).unwrap();
        renderer.draw(&scene, &target.view, 64, 64).unwrap();
    }
    assert_eq!(renderer.texture_bytes(), bytes);
    assert_eq!(renderer.image_count(), count);
    renderer.clear_assets();
    assert_eq!(renderer.texture_bytes(), 4);
    assert_eq!(renderer.image_count(), 1);

    // Real PNG decode/upload, with declared dimensions verified before upload.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("assets")).unwrap();
    image::save_buffer(
        tmp.path().join("assets/a.png"),
        &data,
        2,
        2,
        image::ColorType::Rgba8,
    )
    .unwrap();
    p.assets = vec![Asset {
        id: 7,
        path: "assets/a.png".into(),
        width: 2,
        height: 2,
    }];
    renderer.synchronize_assets(&p, tmp.path()).unwrap();
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 1, 1), [255, 0, 0, 255]);

    p.background = [0.0; 4];
    p.layers[0].content = Content::Solid {
        color: [1.0, 0.0, 0.0, 0.5],
    };
    scene.sample(&p, 0.0, None).unwrap();
    let (out, _) = renderer.capture(&scene, &target).unwrap();
    assert_color(pixel(&out, 32, 32), [255, 0, 0, 128]); // straight-alpha PNG output
}
