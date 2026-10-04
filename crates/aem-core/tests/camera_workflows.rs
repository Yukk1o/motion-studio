use aem_core::{CameraMode, Command, Engine, Observer, Project, Property, Scene};
#[test]
fn static_mode_conversion_does_not_create_a_dense_keyframe_track() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let initial = e.project().camera.position_at(0.0);
    e.apply(Command::CameraMode {
        mode: CameraMode::Orbit,
    })
    .unwrap();
    assert!(e.project().camera.radius.keys.is_empty());
    assert!(e.project().camera.azimuth.keys.is_empty());
    assert!(e.project().camera.elevation.keys.is_empty());
    e.apply(Command::Animate {
        object: 0,
        property: Property::Azimuth,
        frame: 0,
        enabled: true,
    })
    .unwrap();
    e.apply(Command::SetScalar {
        object: 0,
        property: Property::Azimuth,
        frame: 150,
        value: 90.0,
    })
    .unwrap();
    assert!((e.project().camera.azimuth.sample(75.0) - 45.0).abs() < 0.001);
    e.undo().unwrap();
    e.undo().unwrap();
    e.apply(Command::CameraMode {
        mode: CameraMode::Position,
    })
    .unwrap();
    assert!(e.project().camera.position.keys.is_empty());
    let final_position = e.project().camera.position_at(0.0);
    for i in 0..3 {
        assert!((initial[i] - final_position[i]).abs() < 0.002);
    }
}
#[test]
fn observation_zoom_changes_view_without_changing_the_project() {
    let p = Project::demo();
    let engine = Engine::new(p.clone()).unwrap();
    let mut observer = Observer::new(p.width, p.height);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, Some(&observer)).unwrap();
    let before = scene.project_point([700.0, 960.0, 0.0]);
    observer.zoom(2.0).unwrap();
    scene.sample(&p, 0.0, Some(&observer)).unwrap();
    let after = scene.project_point([700.0, 960.0, 0.0]);
    assert!((after[0] - 540.0) > (before[0] - 540.0) * 1.9);
    assert_eq!(engine.project(), &p);
    assert_eq!(engine.revision(), 0);
    assert!(observer.zoom(f32::NAN).is_err());
    assert!(observer.zoom(0.0).is_err());
}
