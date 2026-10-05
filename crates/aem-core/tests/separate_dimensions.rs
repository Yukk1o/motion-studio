use aem_core::{
    parse_commands, Axis, Command, Curve, CurveShape, CurveSpace, Ease, Easing, Engine,
    LayerTimeline, Project, Property, Scene, Track,
};

fn edit(e: &mut Engine, text: &str) {
    e.apply_batch(parse_commands(text).unwrap()).unwrap();
}
fn separate(e: &mut Engine, property: Property) {
    e.apply(Command::SeparateDimensions {
        object: 2,
        property,
    })
    .unwrap();
}
fn component(e: &mut Engine, axis: Axis, frame: u32, value: f32) {
    e.apply(Command::SetComponent {
        object: 2,
        property: Property::Position,
        axis,
        frame,
        value,
    })
    .unwrap();
}
fn animate(e: &mut Engine, axis: Axis, frame: u32, enabled: bool) {
    e.apply(Command::Animate {
        object: 2,
        property: Property::Position,
        axis: Some(axis),
        frame,
        enabled,
    })
    .unwrap();
}
fn engine() -> Engine {
    Engine::new(Project::demo()).unwrap()
}

#[test]
fn separation_is_explicit_lossless_and_undoable_for_all_curve_kinds() {
    for curve in [
        None,
        Some(Curve {
            space: CurveSpace::Progress,
            shape: CurveShape::Quadratic {
                control: [0.4, 1.3],
                start: 0.0,
                end: 1.0,
            },
        }),
        Some(Curve {
            space: CurveSpace::Velocity,
            shape: CurveShape::Cubic {
                control1: [0.2, 1.5],
                control2: [0.8, 2.0],
                start: 0.0,
                end: 0.0,
            },
        }),
        Some(Curve {
            space: CurveSpace::Progress,
            shape: CurveShape::Elastic {
                oscillations: 3.0,
                damping: 6.0,
            },
        }),
    ] {
        for ease in [Ease::Linear, Ease::Hold, Ease::InOut] {
            let mut p = Project::demo();
            let t = &mut p.layers[1].transform.rotation;
            t.upsert(0, [0.0, 20.0, 720.0], ease).unwrap();
            t.upsert(90, [360.0, -400.0, 1440.0], Ease::Linear).unwrap();
            t.set_curve(0, Easing { ease, curve }).unwrap();
            let original = t.clone();
            let mut e = Engine::new(p).unwrap();
            let before = e.snapshot();
            assert!(e.project().layers[1].transform.rotation.axes.is_none());
            separate(&mut e, Property::Rotation);
            let t = &e.project().layers[1].transform.rotation;
            assert!(t.keys.is_empty());
            assert_eq!(t.axes.as_ref().unwrap().x.keys.len(), 2);
            for i in 0..720 {
                assert_eq!(t.sample(i as f64 / 4.0), original.sample(i as f64 / 4.0));
            }
            let revision = e.revision();
            separate(&mut e, Property::Rotation);
            assert_eq!(e.revision(), revision);
            e.undo().unwrap();
            assert_eq!(e.project(), &before);
            e.redo().unwrap();
            assert!(e.project().layers[1].transform.rotation.axes.is_some());
        }
    }
}
#[test]
fn independent_axes_keep_different_keys_static_values_and_segment_curves() {
    let mut e = engine();
    separate(&mut e, Property::Position);
    animate(&mut e, Axis::X, 0, true);
    component(&mut e, Axis::X, 60, 640.0);
    animate(&mut e, Axis::Y, 15, true);
    component(&mut e, Axis::Y, 90, 1060.0);
    component(&mut e, Axis::Z, 30, 25.0);
    let axes = e.project().layers[1]
        .transform
        .position
        .axes
        .as_ref()
        .unwrap();
    assert_eq!(
        axes.x.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![0, 60]
    );
    assert_eq!(
        axes.y.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
        vec![15, 90]
    );
    assert!(axes.z.keys.is_empty());
    let y = axes.y.clone();
    let z = axes.z.clone();
    edit(
        &mut e,
        r#"{"op":"curve","composition":"comp-main","object":2,"property":"position","axis":"x","frame":0,"easing":{"ease":"hold"}}"#,
    );
    for (f, x, yvalue) in [(30.0, 540.0, 980.0), (60.0, 640.0, 1020.0)] {
        assert_eq!(
            e.project().layers[1].transform.position.sample(f),
            [x, yvalue, 25.0]
        );
    }
    component(&mut e, Axis::X, 30, 555.0);
    edit(
        &mut e,
        r#"{"op":"copy_key","object":2,"property":"position","axis":"x","from":30,"to":40}"#,
    );
    edit(
        &mut e,
        r#"{"op":"move_key","object":2,"property":"position","axis":"x","from":40,"to":50}"#,
    );
    edit(
        &mut e,
        r#"{"op":"delete_key","object":2,"property":"position","axis":"x","frame":50}"#,
    );
    let a = e.project().layers[1]
        .transform
        .position
        .axes
        .as_ref()
        .unwrap();
    assert_eq!(a.y, y);
    assert_eq!(a.z, z);
}
#[test]
fn linked_component_values_are_an_explicit_atomic_batch() {
    let mut e = engine();
    separate(&mut e, Property::Scale);
    edit(
        &mut e,
        r#"{"op":"animate","object":2,"property":"scale","axis":"x","frame":0,"enabled":true}"#,
    );
    edit(
        &mut e,
        r#"{"op":"animate","object":2,"property":"scale","axis":"y","frame":15,"enabled":true}"#,
    );
    let z = e.project().layers[1]
        .transform
        .scale
        .axes
        .as_ref()
        .unwrap()
        .z
        .clone();
    edit(
        &mut e,
        r#"[{"op":"set_component","object":2,"property":"scale","axis":"x","frame":30,"value":200},{"op":"set_component","object":2,"property":"scale","axis":"y","frame":30,"value":150}]"#,
    );
    let a = e.project().layers[1].transform.scale.axes.as_ref().unwrap();
    assert_eq!(a.z, z);
    assert_eq!(a.x.keys[0].frame, 0);
    assert_eq!(a.y.keys[0].frame, 15);
    let before = e.snapshot();
    let revision = e.revision();
    let commands=parse_commands(r#"[{"op":"set_component","object":2,"property":"scale","axis":"x","frame":45,"value":300},{"op":"set_component","object":2,"property":"scale","axis":"y","frame":45,"value":200000}]"#).unwrap();
    assert!(e.apply_batch(commands).is_err());
    assert_eq!(e.project(), &before);
    assert_eq!(e.revision(), revision);
}
#[test]
fn whole_legacy_key_calls_validate_all_axes_without_inserting_missing_sources() {
    let mut e = engine();
    separate(&mut e, Property::Position);
    animate(&mut e, Axis::X, 0, true);
    component(&mut e, Axis::X, 60, 600.0);
    animate(&mut e, Axis::Y, 15, true);
    component(&mut e, Axis::Y, 90, 990.0);
    let before = e.snapshot();
    for text in [
        r#"{"op":"move_key","object":2,"property":"position","from":0,"to":10}"#,
        r#"{"op":"curve","object":2,"property":"position","frame":0,"easing":{"ease":"hold"}}"#,
        r#"{"op":"copy_key","object":2,"property":"position","from":15,"to":50}"#,
    ] {
        assert!(e.apply_batch(parse_commands(text).unwrap()).is_err());
        assert_eq!(e.project(), &before);
    }
    let mut e = engine();
    edit(
        &mut e,
        r#"{"op":"animate","object":2,"property":"position","frame":0,"enabled":true}"#,
    );
    separate(&mut e, Property::Position);
    edit(
        &mut e,
        r#"{"op":"move_key","object":2,"property":"position","from":0,"to":10}"#,
    );
    for t in e.project().layers[1]
        .transform
        .position
        .axes
        .as_ref()
        .unwrap()
        .tracks()
    {
        assert_eq!(t.keys[0].frame, 10);
    }
}
#[test]
fn curve_paste_gesture_undo_and_cancel_only_edit_the_explicit_axis() {
    let mut e = engine();
    separate(&mut e, Property::Position);
    animate(&mut e, Axis::X, 0, true);
    component(&mut e, Axis::X, 60, 640.0);
    animate(&mut e, Axis::Y, 15, true);
    component(&mut e, Axis::Y, 90, 1000.0);
    let before = e.snapshot();
    let target_before = e.project().layers[1]
        .transform
        .position
        .axes
        .as_ref()
        .unwrap()
        .y
        .keys
        .clone();
    let easing = Easing {
        ease: Ease::Linear,
        curve: Some(Curve {
            space: CurveSpace::Progress,
            shape: CurveShape::Elastic {
                oscillations: 2.5,
                damping: 6.0,
            },
        }),
    };
    let curve = Command::Curve {
        object: 2,
        property: Property::Position,
        axis: Some(Axis::Y),
        frame: 15,
        easing,
    };
    e.begin_gesture().unwrap();
    e.apply(curve.clone()).unwrap();
    e.end_gesture(false).unwrap();
    assert_eq!(e.project(), &before);
    e.begin_gesture().unwrap();
    for _ in 0..12 {
        e.apply(curve.clone()).unwrap();
    }
    e.end_gesture(true).unwrap();
    let a = e.project().layers[1]
        .transform
        .position
        .axes
        .as_ref()
        .unwrap();
    let b = before.layers[1].transform.position.axes.as_ref().unwrap();
    assert_eq!(a.x, b.x);
    assert_eq!(a.z, b.z);
    for (a, b) in a.y.keys.iter().zip(target_before.iter()) {
        assert_eq!(a.frame, b.frame);
        assert_eq!(a.value, b.value);
    }
    let after = e.snapshot();
    e.undo().unwrap();
    assert_eq!(e.project(), &before);
    e.redo().unwrap();
    assert_eq!(e.project(), &after);
}
#[test]
fn axis_tracks_survive_clips_parenting_and_storage_without_vector_shadow_data() {
    let mut e = engine();
    separate(&mut e, Property::Position);
    animate(&mut e, Axis::X, 0, true);
    component(&mut e, Axis::X, 60, 640.0);
    e.apply(Command::TrimLayerClip {
        object: 2,
        in_frame: 0,
        out_frame: 100,
    })
    .unwrap();
    e.apply(Command::MoveLayerClip {
        object: 2,
        in_frame: 20,
    })
    .unwrap();
    component(&mut e, Axis::Y, 5, 123.0);
    let expected = e.snapshot();
    e.apply(Command::Parent {
        object: 3,
        parent: Some(2),
        frame: 20,
    })
    .unwrap();
    let parented = e.snapshot();
    e.apply(Command::SplitLayerClip {
        object: 2,
        frame: 61,
    })
    .unwrap();
    assert_eq!(
        e.project().layers[1].transform,
        e.project().layers[2].transform
    );
    for f in 0..180 {
        let mut a = Scene::new(&parented);
        let mut b = Scene::new(e.project());
        a.sample(&parented, f as f64, None).unwrap();
        b.sample(e.project(), f as f64, None).unwrap();
        assert!(a
            .world_matrix(3)
            .unwrap()
            .abs_diff_eq(b.world_matrix(3).unwrap(), 1e-4));
    }
    let timeline = e.project().timeline_layers(25.0);
    let a = timeline[1].properties.position.axes.as_ref().unwrap();
    assert_eq!(a.x.keys[0].frame, 20);
    assert_eq!(a.x.keys[0].local_frame, 0);
    let tmp = tempfile::tempdir().unwrap();
    aem_core::storage::save(tmp.path(), e.project()).unwrap();
    assert_eq!(aem_core::storage::load(tmp.path()).unwrap(), e.snapshot());
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.path().join("project.json")).unwrap())
            .unwrap();
    let t = &json["layers"][1]["transform"]["position"];
    assert!(t.get("value").is_none() && t.get("keys").is_none() && t.get("axes").is_some());
    let package = tmp.path().join("axis.aem");
    aem_core::storage::export_package(tmp.path(), e.project(), &package).unwrap();
    assert_eq!(
        aem_core::storage::import_package(&package, &tmp.path().join("import")).unwrap(),
        e.snapshot()
    );
    assert_eq!(
        expected.layers[1].timeline,
        Some(LayerTimeline {
            in_frame: 20,
            out_frame: 120,
            offset_frame: 20
        })
    );
}
#[test]
fn unsupported_axes_modes_or_targets_fail_without_silent_activation() {
    let mut e = engine();
    let before = e.snapshot();
    assert!(e
        .apply(Command::SetComponent {
            object: 2,
            property: Property::Position,
            axis: Axis::X,
            frame: 0,
            value: 20.0
        })
        .is_err());
    assert_eq!(e.project(), &before);
    assert!(e
        .apply(Command::SeparateDimensions {
            object: 2,
            property: Property::Opacity
        })
        .is_err());
    assert!(parse_commands(
        r#"{"op":"separate_dimensions","composition":"missing","object":2,"property":"position"}"#
    )
    .is_err());
    assert!(parse_commands(
        r#"{"op":"animate","object":2,"property":"position","axis":"w","frame":0,"enabled":true}"#
    )
    .is_err());
    e.apply(Command::CameraMode {
        mode: aem_core::CameraMode::Orbit,
    })
    .unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::SeparateDimensions {
            object: 0,
            property: Property::Position
        })
        .is_err());
    assert_eq!(e.project(), &before);
    e.apply(Command::SeparateDimensions {
        object: 0,
        property: Property::Target,
    })
    .unwrap();
    edit(
        &mut e,
        r#"{"op":"set_component","object":0,"property":"target","axis":"x","frame":0,"value":600}"#,
    );
    assert_eq!(e.project().camera.target.sample(0.0)[0], 600.0);
    let scalar = Track::<f32>::constant(0.5);
    assert!(scalar.axes.is_none());
}
