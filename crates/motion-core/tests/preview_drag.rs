use motion_core::{CameraMode, ObservationView, Observer, Project, Track};
use motion_render::Scene;

#[test]
fn preview_drag_tracks_screen_distance_at_any_fit_size_and_depth() {
    let mut p = Project::demo();
    for (width, height) in [(1080, 700), (360, 640), (900, 400)] {
        for z in [-800.0, 0.0, 1200.0] {
            p.layers[1].transform.position = Track::constant([430.0, 810.0, z]);
            p.layers[1].transform.scale = Track::constant([170.0, 65.0, 100.0]);
            let mut scene = Scene::new(&p);
            scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
            let point = p.layers[1].transform.position.value;
            let before = scene.project_point(point);
            let delta = scene
                .screen_translation(point, [37.0, -19.0], [width, height])
                .unwrap();
            assert!(delta[2].abs() < 0.001, "front view must preserve Z");
            let moved = std::array::from_fn(|i| point[i] + delta[i]);
            let after = scene.project_point(moved);
            let fit = (width as f32 / p.width as f32).min(height as f32 / p.height as f32);
            assert!(((after[0] - before[0]) * fit - 37.0).abs() < 0.005);
            assert!(((after[1] - before[1]) * fit + 19.0).abs() < 0.005);
            assert!((after[2] - before[2]).abs() < 0.00001);
            // Translation must preserve the projected width and height.
            for offset in [[200.0, 0.0, 0.0], [0.0, 140.0, 0.0]] {
                let edge_before =
                    scene.project_point(std::array::from_fn(|i| point[i] + offset[i]));
                let edge_after = scene.project_point(std::array::from_fn(|i| moved[i] + offset[i]));
                for axis in 0..2 {
                    assert!(
                        ((edge_after[axis] - after[axis]) - (edge_before[axis] - before[axis]))
                            .abs()
                            < 0.005
                    );
                }
            }
        }
    }
}

#[test]
fn drag_respects_orbit_and_orthographic_camera_projection() {
    let mut p = Project::demo();
    p.camera.mode = CameraMode::Orbit;
    p.camera.azimuth.value = 35.0;
    p.camera.elevation.value = 20.0;
    p.camera.roll.value = 12.0;
    let observer = Observer::new(p.width, p.height);
    let mut scene = Scene::new(&p);
    for view in [
        None,
        Some(ObservationView::Top),
        Some(ObservationView::Side),
    ] {
        let mut o = observer.clone();
        if let Some(v) = view {
            o.view = v;
        }
        scene.sample(&p, 0.0, view.map(|_| &o), &motion_core::ExpressionEvaluator).unwrap();
        let point = [540.0, 960.0, 0.0];
        let before = scene.project_point(point);
        let delta = scene
            .screen_translation(point, [-40.0, 23.0], [540, 960])
            .unwrap();
        let after = scene.project_point(std::array::from_fn(|i| point[i] + delta[i]));
        assert!(((after[0] - before[0]) * 0.5 + 40.0).abs() < 0.005);
        assert!(((after[1] - before[1]) * 0.5 - 23.0).abs() < 0.005);
        assert!((after[2] - before[2]).abs() < 0.00001);
    }
    assert!(scene
        .screen_translation([0.0; 3], [f32::NAN, 0.0], [540, 960])
        .is_err());
    assert!(scene
        .screen_translation([0.0; 3], [1.0, 1.0], [0, 960])
        .is_err());
}
