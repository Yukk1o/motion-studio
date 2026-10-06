use aem_core::{vector::*, Command, Content, Engine, Project, Scene, Track};
fn empty() -> Project {
    Project::new(256, 256, 30, 120).unwrap()
}
#[test]
fn catalogs_validate_all_shapes_and_discrete_ranges() {
    assert_eq!(shape_catalog().as_array().unwrap().len(), 25);
    for (kind, _, _, _) in SHAPES {
        let v = VectorContent::shape(kind);
        v.validate().unwrap();
        let sample = v.sample(0., [128.; 2]).unwrap();
        assert!(!sample.paths.is_empty());
        assert!(sample
            .paths
            .iter()
            .flat_map(|p| &p.nodes)
            .flatten()
            .all(|f| f.is_finite()));
    }
    let mut v = VectorContent::shape(ShapeKind::Star);
    if let VectorSource::Shape { parameters, .. } = &mut v.source {
        parameters.get_mut("points").unwrap().value = 2.;
    }
    assert!(v.validate().is_err());
    if let VectorSource::Shape { parameters, .. } = &mut v.source {
        parameters.get_mut("points").unwrap().value = 5.5;
    }
    assert!(v.validate().is_err());
}
#[test]
fn edit_gesture_conversion_local_animation_undo_and_atomic_failure() {
    let mut e = Engine::new(empty()).unwrap();
    e.apply(Command::AddShape {
        id: 1,
        name: "star".into(),
        shape: ShapeKind::Star,
        size: [128.; 2],
        position: [128., 128., 0.],
    })
    .unwrap();
    let baseline = e.snapshot();
    e.begin_gesture().unwrap();
    for v in [0.3, 0.7] {
        e.apply(Command::Vector {
            object: 1,
            action: VectorAction::SetParameter {
                parameter: "inner_ratio".into(),
                frame: 0,
                value: v,
                animated: false,
            },
        })
        .unwrap();
    }
    e.end_gesture(true).unwrap();
    assert_ne!(*e.project(), baseline);
    e.undo().unwrap();
    assert_eq!(*e.project(), baseline);
    e.redo().unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::Vector {
            object: 1,
            action: VectorAction::SetParameter {
                parameter: "inner_ratio".into(),
                frame: 0,
                value: 1.1,
                animated: false
            }
        })
        .is_err());
    assert_eq!(*e.project(), before);
    e.apply(Command::Vector {
        object: 1,
        action: VectorAction::ConvertToPath { frame: 0 },
    })
    .unwrap();
    e.apply(Command::TrimLayerClip {
        object: 1,
        in_frame: 0,
        out_frame: 80,
    })
    .unwrap();
    e.apply(Command::MoveLayerClip {
        object: 1,
        in_frame: 20,
    })
    .unwrap();
    e.apply(Command::Vector {
        object: 1,
        action: VectorAction::SetNode {
            path: 1,
            node: 1,
            frame: 20,
            value: [0., 0., 0., 0., 0., 0.],
            animated: true,
        },
    })
    .unwrap();
    e.apply(Command::Vector {
        object: 1,
        action: VectorAction::SetNode {
            path: 1,
            node: 1,
            frame: 40,
            value: [20., 0., 0., 0., 0., 0.],
            animated: true,
        },
    })
    .unwrap();
    let mut scene = Scene::new(e.project());
    scene.sample(e.project(), 30., None).unwrap();
    assert_eq!(
        scene.layers[0].vector.as_ref().unwrap().paths[0].nodes[0][0],
        10.
    );
    let json = serde_json::to_string(e.project()).unwrap();
    let restored: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, e.snapshot());
    restored.validate().unwrap();
}
#[test]
fn adjustment_factory_identity_and_invalid_spatial_mode() {
    let mut e = Engine::new(empty()).unwrap();
    e.apply(Command::AddAdjustment {
        id: 1,
        name: "adjust".into(),
    })
    .unwrap();
    assert!(matches!(e.project().layers[0].content, Content::Adjustment));
    assert_eq!(e.project().layers[0].size, [256.; 2]);
    let before = e.snapshot();
    assert!(e
        .apply(Command::SetLayer3d {
            object: 1,
            enabled: true
        })
        .is_err());
    assert_eq!(before, e.snapshot());
    let mut old = empty();
    old.version = 5;
    assert_eq!(old.migrate().unwrap().version, 6);
    let mut invalid = e.snapshot();
    invalid.version = 5;
    assert!(invalid.validate().is_err());
}
#[test]
fn malformed_path_and_overshooting_sample_are_rejected() {
    let mut v = VectorContent::shape(ShapeKind::Rectangle);
    v.source = VectorSource::Paths {
        paths: vec![VectorPath {
            id: 1,
            closed: true,
            nodes: vec![PathNode {
                id: 1,
                geometry: Track::constant([0.; 6]),
            }],
        }],
    };
    assert!(v.validate().is_err());
}
