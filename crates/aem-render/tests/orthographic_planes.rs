use aem_core::{Layer, ObservationView, Observer, Project, Scene};
use aem_render::Renderer;

fn project(view: ObservationView) -> Project {
    let mut project = Project::new(256, 256, 30, 180).unwrap();
    project.background = [0.0, 0.0, 0.0, 1.0];
    for (id, offset, angle, color) in [
        (1, 400.0, 10.0, [1.0, 0.0, 0.0, 1.0]),
        (2, 500.0, 20.0, [0.0, 0.0, 1.0, 1.0]),
    ] {
        // Centres lie beyond the finite observer eye. The visible portions
        // extend back into its frustum and must be ordered along parallel rays.
        let (size, position, axis) = match view {
            ObservationView::Top => ([128.0, 2048.0], [128.0, 128.0 - offset, 0.0], 0),
            ObservationView::Side => ([2048.0, 128.0], [128.0 + offset, 128.0, 0.0], 1),
            ObservationView::Free => unreachable!(),
        };
        let mut layer = Layer::solid(id, "large tilted plane", size, position, color);
        layer.three_d = true;
        layer.transform.rotation.value[axis] = angle;
        project.layers.push(layer);
    }
    project.validate().unwrap();
    project
}

fn check(view: ObservationView, point: [usize; 2]) {
    let mut project = project(view);
    let mut observer = Observer::new(256, 256);
    observer.view = view;
    let mut scene = Scene::new(&project);
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(256, 256).unwrap();
    for swapped in [false, true] {
        if swapped {
            project.layers.swap(0, 1);
        }
        for mirrored in [false, true] {
            for layer in &mut project.layers {
                layer.transform.scale.value[0] = if mirrored { -100.0 } else { 100.0 };
            }
            for opacity in [1.0, 0.5] {
                project
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == 2)
                    .unwrap()
                    .transform
                    .opacity
                    .value = opacity;
                scene.sample(&project, 0.0, Some(&observer)).unwrap();
                let hits = scene.hit_candidates(point.map(|v| v as f32)).unwrap();
                assert_eq!(hits.len(), 2);
                assert_eq!(hits[0].id, 2, "blue is nearer along this parallel ray");
                let (pixels, _) = renderer.capture(&scene, &target).unwrap();
                let index = (point[1] * 256 + point[0]) * 4;
                let actual = &pixels[index..index + 4];
                let expected = if opacity == 1.0 {
                    [0_u8, 0, 255, 255]
                } else {
                    [188, 0, 188, 255]
                };
                for i in 0..4 {
                    assert!(actual[i].abs_diff(expected[i]) <= 3,
                        "{view:?}, swapped={swapped}, mirrored={mirrored}, opacity={opacity}: pixel={actual:?}, expected={expected:?}; hits={hits:?}");
                }
            }
        }
    }
}

#[test]
fn top_view_parallel_rays_preserve_opaque_and_translucent_occlusion() {
    check(ObservationView::Top, [128, 218]);
}

#[test]
fn side_view_parallel_rays_preserve_opaque_and_translucent_occlusion() {
    check(ObservationView::Side, [218, 128]);
}
