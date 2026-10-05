use aem_core::{
    Command, Content, Curve, CurveShape, CurveSpace, Ease, Easing, Engine, Layer, LayerTimeline,
    Observer, Project, Property, Scene,
};

fn fixture() -> Engine {
    let mut p = Project::demo();
    let l = &mut p.layers[1];
    l.timeline = Some(LayerTimeline {
        in_frame: 20,
        out_frame: 100,
        offset_frame: 20,
    });
    for t in [
        &mut l.transform.position,
        &mut l.transform.rotation,
        &mut l.transform.scale,
    ] {
        let a = t.value;
        t.upsert(-10, a, Ease::Hold).unwrap();
        t.upsert(0, a, Ease::Linear).unwrap();
        t.upsert(80, [a[0] + 20.0, a[1] + 10.0, a[2] + 5.0], Ease::Out)
            .unwrap();
        t.set_curve(
            0,
            Easing {
                ease: Ease::Linear,
                curve: Some(Curve {
                    space: CurveSpace::Progress,
                    shape: CurveShape::Elastic {
                        oscillations: 2.5,
                        damping: 6.0,
                    },
                }),
            },
        )
        .unwrap();
    }
    l.transform.opacity.upsert(0, 0.2, Ease::InOut).unwrap();
    l.transform.opacity.upsert(100, 0.8, Ease::Linear).unwrap();
    Engine::new(p).unwrap()
}
fn scene(p: &Project, f: f64) -> Scene {
    let mut s = Scene::new(p);
    s.sample(p, f, None).unwrap();
    s
}
fn compare(a: &Project, b: &Project, fa: f64, fb: f64, ia: u64, ib: u64) {
    let a = scene(a, fa);
    let b = scene(b, fb);
    let a = a.layers.iter().find(|l| l.id == ia).unwrap();
    let b = b.layers.iter().find(|l| l.id == ib).unwrap();
    assert!(a.model.abs_diff_eq(b.model, 1e-4));
    assert_eq!(a.opacity, b.opacity);
    assert_eq!(a.size, b.size);
    assert_eq!(a.color, b.color);
    assert_eq!(a.asset, b.asset);
}

#[test]
fn legacy_migration_is_in_memory_and_preserves_every_fractional_projection() {
    let mut p = Project::demo();
    p.version = 1;
    p.layers[1]
        .transform
        .rotation
        .upsert(0, [0.0; 3], Ease::Hold)
        .unwrap();
    p.layers[1]
        .transform
        .rotation
        .upsert(90, [0.0, 0.0, 720.0], Ease::Linear)
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    aem_core::storage::save(tmp.path(), &p).unwrap();
    let original = std::fs::read(tmp.path().join("project.json")).unwrap();
    let migrated = aem_core::storage::load(tmp.path()).unwrap();
    assert_eq!(
        std::fs::read(tmp.path().join("project.json")).unwrap(),
        original
    );
    assert_eq!(migrated.version, 2);
    for f in 0..360 {
        compare(&p, &migrated, f as f64 / 2.0, f as f64 / 2.0, 2, 2);
    }
    let mut unknown = p;
    unknown.version = 3;
    assert!(Engine::new(unknown).is_err());
}
#[test]
fn clip_is_left_closed_right_open_at_fractional_frames() {
    let e = fixture();
    for (f, active) in [
        (19.0, false),
        (20.0, true),
        (99.0, true),
        (99.5, true),
        (100.0, false),
    ] {
        assert_eq!(
            scene(e.project(), f).layers.iter().any(|l| l.id == 2),
            active
        );
        assert_eq!(e.project().timeline_layers(f)[1].active, active);
    }
}
#[test]
fn moves_preserve_all_curves_and_trims_are_reversible_without_baking() {
    let mut e = fixture();
    let before = e.snapshot();
    e.apply(Command::MoveLayerClip {
        object: 2,
        in_frame: 40,
    })
    .unwrap();
    assert_eq!(e.project().layers[1].transform, before.layers[1].transform);
    for f in 40..200 {
        compare(
            &before,
            e.project(),
            f as f64 / 2.0,
            f as f64 / 2.0 + 20.0,
            2,
            2,
        );
    }
    assert_eq!(
        e.project().layers[1].clip(180),
        LayerTimeline {
            in_frame: 40,
            out_frame: 120,
            offset_frame: 40
        }
    );
    e.apply(Command::MoveLayerClip {
        object: 2,
        in_frame: 20,
    })
    .unwrap();
    e.apply(Command::TrimLayerClip {
        object: 2,
        in_frame: 30,
        out_frame: 80,
    })
    .unwrap();
    assert_eq!(e.project().layers[1].transform, before.layers[1].transform);
    e.apply(Command::TrimLayerClip {
        object: 2,
        in_frame: 20,
        out_frame: 100,
    })
    .unwrap();
    assert_eq!(e.project(), &before);
}
#[test]
fn key_commands_convert_composition_time_and_keep_negative_local_keys() {
    let mut e = fixture();
    e.apply(Command::TrimLayerClip {
        object: 2,
        in_frame: 0,
        out_frame: 110,
    })
    .unwrap();
    e.apply(Command::SetVector {
        object: 2,
        property: Property::Position,
        frame: 5,
        value: [5.0; 3],
    })
    .unwrap();
    e.apply(Command::CopyKey {
        axis: None,
        object: 2,
        property: Property::Position,
        from: 5,
        to: 10,
    })
    .unwrap();
    e.apply(Command::MoveKey {
        axis: None,
        object: 2,
        property: Property::Position,
        from: 10,
        to: 15,
    })
    .unwrap();
    e.apply(Command::Curve {
        axis: None,
        object: 2,
        property: Property::Position,
        frame: 15,
        easing: Easing {
            ease: Ease::InOut,
            curve: None,
        },
    })
    .unwrap();
    let t = &e.project().timeline_layers(15.0)[1].properties.position;
    assert!(t.keys.iter().any(|k| k.frame == 5 && k.local_frame == -15));
    assert!(t
        .keys
        .iter()
        .any(|k| k.frame == 15 && k.local_frame == -5 && k.ease == Ease::InOut));
    e.apply(Command::DeleteKey {
        axis: None,
        object: 2,
        property: Property::Position,
        frame: 5,
    })
    .unwrap();
    e.apply(Command::SetScalar {
        object: 2,
        property: Property::Opacity,
        frame: 10,
        value: 0.3,
    })
    .unwrap();
    assert_eq!(e.project().layers[1].transform.opacity.sample(-10.0), 0.3);
    assert_eq!(
        scene(e.project(), 10.0)
            .layers
            .iter()
            .find(|l| l.id == 2)
            .unwrap()
            .opacity,
        0.3
    );
}
#[test]
fn split_preserves_fractional_samples_and_children_keep_the_original_parent() {
    let mut e = fixture();
    e.apply(Command::Parent {
        object: 3,
        parent: Some(2),
        frame: 20,
    })
    .unwrap();
    let before = e.snapshot();
    let results = e
        .apply_batch(vec![Command::SplitLayerClip {
            object: 2,
            frame: 61,
        }])
        .unwrap();
    assert_eq!(
        serde_json::to_value(results[0].clone()).unwrap()["right_object"],
        4
    );
    assert_eq!(
        e.project().layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![1, 2, 4, 3]
    );
    assert_eq!(
        e.project().layers[3].parent.as_ref().unwrap().object,
        Some(2)
    );
    assert_eq!(
        e.project().layers[1].transform,
        e.project().layers[2].transform
    );
    for f in 0..360 {
        let f = f as f64 / 2.0;
        compare(&before, e.project(), f, f, 3, 3);
        if (20.0..100.0).contains(&f) {
            compare(&before, e.project(), f, f, 2, if f < 61.0 { 2 } else { 4 });
        }
        assert_eq!(
            scene(&before, f).layers.len(),
            scene(e.project(), f).layers.len()
        );
    }
    let after = e.snapshot();
    e.undo().unwrap();
    assert_eq!(e.project(), &before);
    e.redo().unwrap();
    assert_eq!(e.project(), &after);
}
#[test]
fn inactive_null_parent_and_camera_rig_sample_their_own_time() {
    let mut e = fixture();
    e.apply(Command::Content {
        object: 2,
        content: Content::Null,
        size: [100.0; 2],
    })
    .unwrap();
    e.apply(Command::Parent {
        object: 3,
        parent: Some(2),
        frame: 20,
    })
    .unwrap();
    e.apply(Command::Parent {
        object: 0,
        parent: Some(2),
        frame: 20,
    })
    .unwrap();
    let s = scene(e.project(), 150.0);
    assert!(s.layers.iter().any(|l| l.id == 3));
    let mut reference = e.snapshot();
    reference.layers[1].timeline = None;
    // Shift the track's stored times to composition time only in the oracle.
    for k in &mut reference.layers[1].transform.position.keys {
        k.frame += 20;
    }
    for k in &mut reference.layers[1].transform.rotation.keys {
        k.frame += 20;
    }
    for k in &mut reference.layers[1].transform.scale.keys {
        k.frame += 20;
    }
    compare(&reference, e.project(), 150.0, 150.0, 3, 3);
    assert!(scene(&reference, 150.0)
        .camera
        .view_projection
        .abs_diff_eq(s.camera.view_projection, 1e-4));
    let mut observer = Observer::new(1080, 1920);
    observer.orbit(40.0, 10.0).unwrap();
    let mut observed = Scene::new(e.project());
    observed
        .sample(e.project(), 150.0, Some(&observer))
        .unwrap();
    assert!(observed.layers.iter().any(|l| l.id == 3));
}
#[test]
fn failures_are_atomic_and_no_ops_do_not_change_revision_or_history() {
    let mut e = fixture();
    let before = e.snapshot();
    for c in [
        Command::MoveLayerClip {
            object: 2,
            in_frame: 170,
        },
        Command::TrimLayerClip {
            object: 2,
            in_frame: 60,
            out_frame: 60,
        },
        Command::SplitLayerClip {
            object: 2,
            frame: 20,
        },
        Command::MoveLayerClip {
            object: 0,
            in_frame: 0,
        },
        Command::TrimLayerClip {
            object: 999,
            in_frame: 0,
            out_frame: 1,
        },
    ] {
        assert!(e.apply(c).is_err());
        assert_eq!(e.project(), &before);
        assert_eq!(e.revision(), 0);
        assert!(!e.can_undo());
    }
    assert!(serde_json::from_str::<Command>(
        r#"{"op":"move_layer_clip","object":2,"in_frame":-1}"#
    )
    .is_err());
    assert!(serde_json::from_str::<Command>(
        r#"{"op":"split_layer_clip","object":2,"frame":61.5}"#
    )
    .is_err());
    assert!(e
        .apply_batch(vec![
            Command::SplitLayerClip {
                object: 2,
                frame: 61
            },
            Command::MoveLayerClip {
                object: 4,
                in_frame: 179
            }
        ])
        .is_err());
    assert_eq!(e.project(), &before);
    assert_eq!(e.revision(), 0);
    e.apply(Command::MoveLayerClip {
        object: 2,
        in_frame: 20,
    })
    .unwrap();
    e.apply(Command::TrimLayerClip {
        object: 2,
        in_frame: 20,
        out_frame: 100,
    })
    .unwrap();
    assert_eq!(e.revision(), 0);
    assert!(!e.can_undo());
    e.apply(Command::Flags {
        object: 2,
        visible: true,
        locked: true,
    })
    .unwrap();
    let locked = e.snapshot();
    assert!(e
        .apply(Command::SplitLayerClip {
            object: 2,
            frame: 60
        })
        .is_err());
    assert_eq!(e.project(), &locked);
}
#[test]
fn split_limit_and_id_overflow_cannot_leave_half_a_clip() {
    let mut p = fixture().snapshot();
    while p.layers.len() < aem_core::MAX_LAYERS {
        let id = p.layers.len() as u64 + 1;
        p.layers
            .push(Layer::solid(id, "x", [1.0; 2], [0.0; 3], [1.0; 4]));
    }
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::SplitLayerClip {
            object: 2,
            frame: 60
        })
        .is_err());
    assert_eq!(e.project(), &before);
    let mut p = fixture().snapshot();
    p.layers[2].id = u64::MAX;
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::SplitLayerClip {
            object: 2,
            frame: 60
        })
        .is_err());
    assert_eq!(e.project(), &before);
    let mut p = fixture().snapshot();
    p.layers[1].timeline.as_mut().unwrap().offset_frame = i32::MAX;
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::MoveLayerClip {
            object: 2,
            in_frame: 21
        })
        .is_err());
    assert_eq!(e.project(), &before);
}
#[test]
fn clip_gestures_save_and_package_preserve_ids_curves_and_hidden_keys() {
    let mut e = fixture();
    let before = e.snapshot();
    e.begin_gesture().unwrap();
    for f in 21..40 {
        e.apply(Command::MoveLayerClip {
            object: 2,
            in_frame: f,
        })
        .unwrap();
    }
    e.end_gesture(false).unwrap();
    assert_eq!(e.project(), &before);
    e.begin_gesture().unwrap();
    for f in 21..40 {
        e.apply(Command::MoveLayerClip {
            object: 2,
            in_frame: f,
        })
        .unwrap();
    }
    e.end_gesture(true).unwrap();
    let moved = e.snapshot();
    e.undo().unwrap();
    assert_eq!(e.project(), &before);
    assert!(!e.can_undo());
    e.redo().unwrap();
    assert_eq!(e.project(), &moved);
    e.apply(Command::SplitLayerClip {
        object: 2,
        frame: 63,
    })
    .unwrap();
    e.apply(Command::Reorder {
        object: 4,
        index: 0,
    })
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    aem_core::storage::save(tmp.path(), e.project()).unwrap();
    assert_eq!(aem_core::storage::load(tmp.path()).unwrap(), e.snapshot());
    let package = tmp.path().join("motion.aem");
    aem_core::storage::export_package(tmp.path(), e.project(), &package).unwrap();
    assert_eq!(
        aem_core::storage::import_package(&package, &tmp.path().join("import")).unwrap(),
        e.snapshot()
    );
    assert!(!std::fs::read_to_string(tmp.path().join("project.json"))
        .unwrap()
        .contains("timeline_layers"));
}
