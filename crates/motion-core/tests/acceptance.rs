use motion_core::{
    CameraMode, Command, Content, Ease, Engine, Layer, Observer, Project, Property, Scene, Track,
};
use glam::Vec3;

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.02, "{a} != {b}");
}
fn vclose(a: [f32; 3], b: [f32; 3]) {
    for i in 0..3 {
        close(a[i], b[i]);
    }
}
fn set(frame: u32, value: [f32; 3]) -> Command {
    Command::SetVector {
        object: 2,
        property: Property::Position,
        frame,
        value,
    }
}

#[test]
fn a1_motion_easing_holds_and_unwrapped_rotation() {
    let mut p = Project::demo();
    let layer = &mut p.layers[1];
    layer
        .transform
        .position
        .upsert(0, [0.0, 0.0, 0.0], Ease::Linear)
        .unwrap();
    layer
        .transform
        .position
        .upsert(60, [120.0, 240.0, 60.0], Ease::Linear)
        .unwrap();
    layer
        .transform
        .scale
        .upsert(0, [100.0; 3], Ease::In)
        .unwrap();
    layer
        .transform
        .scale
        .upsert(60, [200.0; 3], Ease::Linear)
        .unwrap();
    layer
        .transform
        .rotation
        .upsert(0, [0.0; 3], Ease::Linear)
        .unwrap();
    layer
        .transform
        .rotation
        .upsert(60, [0.0, 0.0, 360.0], Ease::Linear)
        .unwrap();
    layer.transform.opacity.upsert(0, 0.0, Ease::InOut).unwrap();
    layer
        .transform
        .opacity
        .upsert(60, 1.0, Ease::Linear)
        .unwrap();
    vclose(layer.transform.position.sample(30.0), [60.0, 120.0, 30.0]);
    close(layer.transform.scale.sample(30.0)[0], 125.0);
    close(layer.transform.rotation.sample(30.0)[2], 180.0);
    close(layer.transform.opacity.sample(15.0), 0.15625);
    close(Ease::Out.map(0.25), 0.4375);
    close(layer.transform.opacity.sample(120.0), 1.0);
    layer.transform.opacity.set_ease(0, Ease::Hold).unwrap();
    close(layer.transform.opacity.sample(59.99), 0.0);
    close(layer.transform.opacity.sample(60.0), 1.0);
    p.validate().unwrap();
}

#[test]
fn a2_key_collision_move_undo_and_redo_restore_both_keys() {
    let mut engine = Engine::new(Project::demo()).unwrap();
    engine
        .apply(Command::Animate {
            axis: None,
            object: 2,
            property: Property::Position,
            frame: 0,
            enabled: true,
        })
        .unwrap();
    engine.apply(set(60, [640.0, 960.0, 0.0])).unwrap();
    engine.apply(set(60, [740.0, 960.0, 0.0])).unwrap();
    assert_eq!(engine.project().layers[1].transform.position.keys.len(), 2);
    let before = engine.snapshot();
    engine
        .apply(Command::MoveKey {
            axis: None,
            object: 2,
            property: Property::Position,
            from: 60,
            to: 0,
        })
        .unwrap();
    assert_eq!(engine.project().layers[1].transform.position.keys.len(), 1);
    close(
        engine.project().layers[1].transform.position.sample(0.0)[0],
        740.0,
    );
    assert!(engine.undo().unwrap());
    assert_eq!(engine.project(), &before);
    assert!(engine.redo().unwrap());
    assert_eq!(engine.project().layers[1].transform.position.keys.len(), 1);
}

#[test]
fn first_key_at_nonzero_time_holds_before_it_and_disabling_keeps_sample() {
    let mut track = Track::constant(0.2);
    track.set_animated(30, true).unwrap();
    track.set_at(60, 1.0).unwrap();
    close(track.sample(0.0), 0.2);
    track.set_animated(45, false).unwrap();
    assert!(track.keys.is_empty());
    close(track.value, 0.6);
}

#[test]
fn edit_batches_are_atomic_and_gestures_have_one_history_entry() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let original = e.snapshot();
    assert!(e
        .apply_batch(vec![
            set(0, [500.0, 960.0, 0.0]),
            Command::SetScalar {
                object: 2,
                property: Property::Opacity,
                frame: 0,
                value: 3.0
            }
        ])
        .is_err());
    assert_eq!(e.project(), &original);
    assert!(!e.can_undo());
    assert_eq!(e.revision(), 0);
    e.begin_gesture().unwrap();
    for x in 0..50 {
        e.apply(set(0, [540.0 + x as f32, 960.0, 0.0])).unwrap();
    }
    assert!(e.undo().is_err());
    e.end_gesture(true).unwrap();
    e.undo().unwrap();
    assert_eq!(e.project(), &original);
    assert!(!e.can_undo());
    e.redo().unwrap();
    let committed = e.snapshot();
    e.begin_gesture().unwrap();
    e.apply(set(0, [100.0, 960.0, 0.0])).unwrap();
    e.end_gesture(false).unwrap();
    assert_eq!(e.project(), &committed);
}

#[test]
fn a3_dolly_changes_near_scale_more_without_moving_layers_or_changing_fov() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let before = e.snapshot();
    let mut scene = Scene::new(&before);
    scene.sample(&before, 0.0, None).unwrap();
    let measure = |s: &Scene, z: f32| {
        let a = s.project_point([490.0, 960.0, z]);
        let b = s.project_point([590.0, 960.0, z]);
        b[0] - a[0]
    };
    let near_before = measure(&scene, -500.0);
    let far_before = measure(&scene, 500.0);
    e.apply(Command::Dolly {
        frame: 0,
        amount: 200.0,
    })
    .unwrap();
    scene.sample(e.project(), 0.0, None).unwrap();
    assert!(measure(&scene, -500.0) / near_before > measure(&scene, 500.0) / far_before);
    assert_eq!(e.project().layers, before.layers);
    assert_eq!(e.project().camera.fov, before.camera.fov);
}

#[test]
fn a4_pan_keeps_direction_and_produces_depth_dependent_parallax() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let original = e.snapshot();
    let mut scene = Scene::new(&original);
    scene.sample(&original, 0.0, None).unwrap();
    let direction_before = (scene.camera.target - scene.camera.eye).normalize();
    let near = scene.project_point([540.0, 960.0, -500.0])[0];
    let far = scene.project_point([540.0, 960.0, 500.0])[0];
    e.apply(Command::Pan {
        frame: 0,
        x: 100.0,
        y: 20.0,
    })
    .unwrap();
    scene.sample(e.project(), 0.0, None).unwrap();
    assert!(
        (direction_before - (scene.camera.target - scene.camera.eye).normalize()).length() < 1e-5
    );
    assert!(
        (scene.project_point([540.0, 960.0, -500.0])[0] - near).abs()
            > (scene.project_point([540.0, 960.0, 500.0])[0] - far).abs()
    );
    assert_eq!(e.project().layers, original.layers);
}

#[test]
fn a5_orbit_samples_an_arc_with_constant_radius_and_valid_target_orientation() {
    let mut p = Project::demo();
    p.camera.mode = CameraMode::Orbit;
    p.camera.radius.value = 2000.0;
    p.camera.azimuth.upsert(0, 0.0, Ease::Linear).unwrap();
    p.camera.azimuth.upsert(60, 90.0, Ease::Linear).unwrap();
    for frame in 0..=60 {
        let pos = Vec3::from_array(p.camera.position_at(frame as f64));
        let target = Vec3::from_array(p.camera.target.sample(frame as f64));
        close(pos.distance(target), 2000.0);
        let pose = p.camera.pose(frame as f64, p.width, p.height);
        let clip = pose.view_projection * pose.target.extend(1.0);
        close(clip.x / clip.w, 0.0);
        close(clip.y / clip.w, 0.0);
        assert!(pose.view_projection.is_finite());
    }
    let mid = Vec3::from_array(p.camera.position_at(30.0));
    let chord = (Vec3::from_array(p.camera.position_at(0.0))
        + Vec3::from_array(p.camera.position_at(60.0)))
        / 2.0;
    assert!(mid.distance(chord) > 500.0);
}

#[test]
fn camera_mode_conversion_preserves_integer_frame_poses() {
    let mut p = Project::demo();
    p.camera
        .position
        .upsert(0, [540.0, 960.0, -2300.0], Ease::Linear)
        .unwrap();
    p.camera
        .position
        .upsert(150, [1000.0, 800.0, -1800.0], Ease::InOut)
        .unwrap();
    let original = p.camera.clone();
    p.camera.convert_mode(CameraMode::Orbit, p.frames).unwrap();
    for f in 0..p.frames {
        vclose(
            p.camera.position_at(f as f64),
            original.position_at(f as f64),
        );
    }
    p.camera
        .convert_mode(CameraMode::Position, p.frames)
        .unwrap();
    for f in 0..p.frames {
        vclose(
            p.camera.position_at(f as f64),
            original.position_at(f as f64),
        );
    }
}

#[test]
fn a6_observing_a_scene_does_not_modify_project_or_history() {
    let e = Engine::new(Project::demo()).unwrap();
    let original = e.snapshot();
    let revision = e.revision();
    let mut observer = Observer::new(original.width, original.height);
    let mut scene = Scene::new(&original);
    scene.sample(&original, 0.0, None).unwrap();
    let active = scene.camera.view_projection;
    observer.orbit(50.0, 20.0).unwrap();
    scene.sample(e.project(), 0.0, Some(&observer)).unwrap();
    assert_ne!(scene.camera.view_projection, active);
    assert_eq!(e.project(), &original);
    assert_eq!(e.revision(), revision);
    assert!(!e.can_undo());
    scene.sample(e.project(), 0.0, None).unwrap();
    assert_eq!(scene.camera.view_projection, active);
}

#[test]
fn anchor_compensation_preserves_rendered_geometry_through_animation() {
    let mut p = Project::demo();
    p.layers[1]
        .transform
        .rotation
        .upsert(0, [0.0; 3], Ease::Linear)
        .unwrap();
    p.layers[1]
        .transform
        .rotation
        .upsert(150, [20.0, 30.0, 90.0], Ease::InOut)
        .unwrap();
    let before = p.clone();
    let mut e = Engine::new(p).unwrap();
    e.apply(Command::Anchor {
        object: 2,
        anchor: [0.0, 0.0],
    })
    .unwrap();
    let mut a = Scene::new(&before);
    let mut b = Scene::new(e.project());
    for frame in 0..before.frames {
        a.sample(&before, frame as f64, None).unwrap();
        b.sample(e.project(), frame as f64, None).unwrap();
        let am = a.layers.iter().find(|l| l.id == 2).unwrap().model;
        let bm = b.layers.iter().find(|l| l.id == 2).unwrap().model;
        for point in [
            Vec3::ZERO,
            Vec3::new(280.0, 380.0, 0.0),
            Vec3::new(-280.0, -380.0, 0.0),
        ] {
            assert!((am.transform_point3(point) - bm.transform_point3(point)).length() < 0.02);
        }
    }
}

#[test]
fn same_depth_uses_stack_order_and_distinct_depth_uses_camera_space() {
    let mut p = Project::new(1080, 1920, 30, 180).unwrap();
    p.layers = vec![
        Layer::solid(
            1,
            "red",
            [100.0; 2],
            [540.0, 960.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
        ),
        Layer::solid(
            2,
            "blue",
            [100.0; 2],
            [540.0, 960.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
        ),
    ];
    for layer in &mut p.layers {
        layer.three_d = true;
    }
    let mut s = Scene::new(&p);
    s.sample(&p, 0.0, None).unwrap();
    assert_eq!(
        s.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    p.layers[0].transform.position.value[2] = -200.0;
    s.sample(&p, 0.0, None).unwrap();
    assert_eq!(
        s.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn a8_project_package_roundtrip_keeps_assets_and_text_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("assets")).unwrap();
    // Storage verifies ownership and exact bytes; image decoding is a renderer test.
    std::fs::write(root.join("assets/text.png"), b"raster-resource-test").unwrap();
    let mut p = Project::demo();
    p.assets.push(motion_core::Asset {
        id: 1,
        path: "assets/text.png".into(),
        width: 1,
        height: 1,
    });
    p.layers[1].content = Content::Text {
        text: "你好 Motion Studio".into(),
        font: "Noto Sans SC".into(),
        color: [1.0; 4],
        raster_asset: 1,
    };
    motion_core::storage::save(&root, &p).unwrap();
    assert_eq!(motion_core::storage::load(&root).unwrap(), p);
    let archive = temp.path().join("backup.aem");
    motion_core::storage::export_package(&root, &p, &archive).unwrap();
    let imported = temp.path().join("imported");
    assert_eq!(
        motion_core::storage::import_package(&archive, &imported).unwrap(),
        p
    );
    assert_eq!(
        std::fs::read(imported.join("assets/text.png")).unwrap(),
        b"raster-resource-test"
    );
    assert!(motion_core::storage::import_package(&archive, &imported).is_err());
    std::fs::remove_file(imported.join("assets/text.png")).unwrap();
    assert!(motion_core::storage::load(&imported).is_err());
}

#[test]
fn invalid_edits_and_bad_import_metadata_are_rejected_without_mutation() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let p = e.snapshot();
    assert!(e.apply(set(180, [0.0; 3])).is_err());
    assert!(e.apply(set(0, [f32::NAN, 0.0, 0.0])).is_err());
    assert!(e.apply(set(0, [f32::MAX, 0.0, 0.0])).is_err());
    assert_eq!(e.project(), &p);
    for path in [
        "../x",
        "assets/../x",
        "assets/a\\b",
        "assets/C:/x",
        "/assets/a",
    ] {
        assert!(motion_core::storage::validate_relative_path(path).is_err());
    }
    let mut s = Scene::new(&p);
    assert!(s.sample(&p, f64::NAN, None).is_err());
    assert!(s.sample(&p, -1.0, None).is_err());
    e.apply(Command::Flags {
        object: 2,
        visible: true,
        locked: true,
    })
    .unwrap();
    assert!(e.apply(set(0, [0.0; 3])).is_err());
}

#[test]
fn frozen_export_has_180_strictly_ordered_timestamps_and_is_edit_independent() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let frozen = e.snapshot();
    e.apply(set(0, [100.0, 100.0, 100.0])).unwrap();
    let pts: Vec<_> = (0..frozen.frames)
        .map(|f| frozen.frame_pts_us(f).unwrap())
        .collect();
    assert_eq!(pts.len(), 180);
    assert_eq!(pts[0], 0);
    assert_eq!(pts[179], 5_966_667);
    assert!(pts.windows(2).all(|w| w[1] > w[0]));
    assert_eq!(frozen.frames as f64 / frozen.fps as f64, 6.0);
    assert_ne!(
        e.project().layers[1].transform.position,
        frozen.layers[1].transform.position
    );
}

#[test]
fn degenerate_camera_target_and_vertical_view_remain_finite() {
    let mut p = Project::demo();
    p.camera.position.value = p.camera.target.value;
    assert!(p
        .camera
        .pose(0.0, p.width, p.height)
        .view_projection
        .is_finite());
    p.camera.position.value = [540.0, 0.0, 0.0];
    assert!(p
        .camera
        .pose(0.0, p.width, p.height)
        .view_projection
        .is_finite());
}
