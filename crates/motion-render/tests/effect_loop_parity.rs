use motion_core::{Content, EffectInstance, Layer, Project, Scene};
use motion_render::Renderer;

#[test]
fn narrowed_sample_loops_match_published_pixels_including_sparse_and_fractional_taps() {
    let latest = motion_effects::builtin::package().unwrap();
    let previous = motion_effects::builtin::packages()
        .unwrap()
        .into_iter()
        .find(|p| p.manifest.id == latest.manifest.id && p.manifest.version == "1.4.0")
        .unwrap();
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(64, 64).unwrap();
    // Colored transparent edges, impulses and fine spatial variation expose
    // changed tap sets, accumulation order and choker alpha ties.
    let rgba: Vec<u8> = (0..64 * 64)
        .flat_map(|i| {
            let x = i % 64;
            let y = i / 64;
            [
                (x * 37 % 256) as u8,
                (y * 51 % 256) as u8,
                ((x + y) * 13 % 256) as u8,
                if x < 8 || y < 8 {
                    0
                } else {
                    ((x / 4 + y / 4) * 29 % 256) as u8
                },
            ]
        })
        .collect();
    renderer.upload_image(7, 64, 64, &rgba).unwrap();
    for (name, param, values) in [
        (
            "gaussian_blur",
            "p0001",
            &[0., 0.25, 4., 4.25, 128., 128.25, 151.75][..],
        ),
        (
            "fast_box_blur",
            "p0001",
            &[0., 0.25, 4., 4.25, 75., 75.25][..],
        ),
        (
            "unsharp_mask",
            "p0002",
            &[0.25, 4., 4.25, 128., 128.25, 151.75][..],
        ),
        (
            "simple_choker",
            "choke",
            &[-100., -4.25, -4., 0., 4., 4.25, 100.][..],
        ),
    ] {
        for &value in values {
            let mut p = Project::new(64, 64, 30, 60).unwrap();
            p.background = [0.; 4];
            let mut layer = Layer::solid(1, "tap fixture", [64.; 2], [32., 32., 0.], [1.; 4]);
            layer.content = Content::Image { asset: 7 };
            let d = latest
                .manifest
                .effects
                .iter()
                .find(|d| d.id == name)
                .unwrap();
            let mut fx = EffectInstance::new(
                1,
                &latest.manifest.id,
                &latest.manifest.version,
                &latest.hash,
                d,
                layer.size,
            );
            fx.params.get_mut(param).unwrap().track.value[0] = value;
            layer.effects.push(fx);
            p.layers.push(layer);
            let mut outputs = Vec::new();
            for package in [&previous, &latest] {
                p.layers[0].effects[0].version = package.manifest.version.clone();
                p.layers[0].effects[0].hash = package.hash.clone();
                p.rebuild_plugin_dependencies();
                let mut scene = Scene::new(&p);
                scene.sample(&p, 0., None).unwrap();
                outputs.push(renderer.capture(&scene, &target).unwrap().0);
            }
            assert_eq!(outputs[0], outputs[1], "{name}, {param}={value}");
        }
    }
}
