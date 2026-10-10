use motion_core::{Content, Layer, Project, Scene};
use motion_render::Renderer;

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * 256 + x) * 4..(y * 256 + x + 1) * 4]
        .try_into()
        .unwrap()
}
fn encode(v: f32) -> u8 {
    ((if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }) * 255.0)
        .round() as u8
}
fn color(actual: [u8; 4], expected: [u8; 4]) {
    for i in 0..4 {
        assert!(
            actual[i].abs_diff(expected[i]) <= 3,
            "{actual:?} != {expected:?}"
        );
    }
}
fn project() -> Project {
    let mut p = Project::new(256, 256, 30, 180).unwrap();
    p.background = [0.0, 0.0, 0.0, 1.0];
    for (id, angle, tint) in [
        (1, 45.0, [1.0, 0.0, 0.0, 1.0]),
        (2, -45.0, [0.0, 0.0, 1.0, 1.0]),
    ] {
        let mut l = Layer::solid(id, "cross", [256.0; 2], [128.0, 128.0, 0.0], tint);
        l.three_d = true;
        l.transform.rotation.value[1] = angle;
        p.layers.push(l);
    }
    p
}
#[test]
fn opaque_and_translucent_crossing_planes_occlude_per_pixel_in_both_stack_orders() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(256, 256).unwrap();
    let mut p = project();
    let mut scene = Scene::new(&p);
    for _ in 0..2 {
        scene.sample(&p, 0.0, None).unwrap();
        let (pixels, stats) = renderer.capture(&scene, &target).unwrap();
        assert_eq!(stats.draw_calls, 3);
        color(pixel(&pixels, 96, 128), [0, 0, 255, 255]);
        color(pixel(&pixels, 160, 128), [255, 0, 0, 255]);
        p.layers.swap(0, 1);
    }
    p.layers[0].transform.opacity.value = 0.65;
    p.layers[1].transform.opacity.value = 0.35;
    scene.sample(&p, 0.0, None).unwrap();
    let (pixels, _) = renderer.capture(&scene, &target).unwrap();
    color(
        pixel(&pixels, 96, 128),
        [encode(0.65 * 0.65), 0, encode(0.35), 255],
    );
    color(
        pixel(&pixels, 160, 128),
        [encode(0.65), 0, encode(0.35 * 0.35), 255],
    );
    // A transparent cutout must reveal the farther plane, not reserve depth.
    p.layers[0].transform.opacity.value = 1.0;
    p.layers[1].transform.opacity.value = 1.0;
    p.layers[0].content = Content::Image { asset: 7 };
    renderer
        .upload_image(7, 2, 1, &[255, 255, 255, 0, 255, 0, 0, 255])
        .unwrap();
    scene.sample(&p, 0.0, None).unwrap();
    let (pixels, _) = renderer.capture(&scene, &target).unwrap();
    color(pixel(&pixels, 96, 128), [0, 0, 255, 255]);
    color(pixel(&pixels, 190, 128), [255, 0, 0, 255]);
    // A flat UI overlay follows stack order and remains fixed under camera motion.
    p.layers.push(Layer::solid(
        3,
        "overlay",
        [256.0; 2],
        [128.0, 128.0, -900.0],
        [0.0, 1.0, 0.0, 1.0],
    ));
    p.camera.created = true;
    p.camera.position.value = [500.0, 300.0, -300.0];
    scene.sample(&p, 0.0, None).unwrap();
    let (pixels, _) = renderer.capture(&scene, &target).unwrap();
    color(pixel(&pixels, 96, 128), [0, 255, 0, 255]);
    color(pixel(&pixels, 160, 128), [0, 255, 0, 255]);
}
