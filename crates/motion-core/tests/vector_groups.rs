use glam::{Mat3, Vec2};
use motion_core::{
    vector::{groups::*, *},
    Command, Content, Engine, Layer, Project, Track,
};

fn root() -> VectorGroup {
    let v = VectorGroup::wrap(VectorContent::shape(ShapeKind::Rectangle), [10.; 2]);
    let VectorSource::Group { group } = v.source else {
        unreachable!()
    };
    *group
}
fn repeat(id: u64, copies: f32, position: [f32; 2]) -> GroupItem {
    GroupItem::Repeater {
        id,
        name: "Repeater".into(),
        repeater: Repeater {
            copies: Track::constant(copies),
            position: Track::constant(position),
            ..Default::default()
        },
    }
}
#[test]
fn conversion_preserves_geometry_paints_and_shape_animation() {
    let mut original = VectorContent::shape(ShapeKind::Star);
    if let VectorSource::Shape { parameters, .. } = &mut original.source {
        let t = parameters.get_mut("inner_ratio").unwrap();
        t.set_animated(0, true).unwrap();
        t.set_at(20, 0.7).unwrap();
    }
    let grouped = VectorGroup::wrap(original.clone(), [60.; 2]);
    grouped.validate().unwrap();
    let sample = grouped.sample(10., [60.; 2]).unwrap();
    assert_eq!(sample.batches.as_ref().unwrap().len(), 1);
    assert_eq!(
        *sample.batches.as_ref().unwrap()[0].vector,
        original.sample(10., [60.; 2]).unwrap()
    );
    assert!(
        (sample.group_parameters[&2]["shape:inner_ratio"]
            .as_f64()
            .unwrap()
            - 0.6)
            .abs()
            < 1e-6
    );
    assert_eq!(sample.root_opacity, 1.);
}
#[test]
fn two_repeaters_form_editable_grid_and_keep_virtual_scopes_separate() {
    let mut g = root();
    g.items.push(repeat(3, 3., [10., 0.]));
    g.items.push(repeat(4, 2., [0., 20.]));
    g.validate().unwrap();
    let v = g.sample(0.).unwrap();
    let b = v.batches.unwrap();
    assert_eq!(b.len(), 6);
    let mut positions: Vec<_> = b
        .iter()
        .map(|p| {
            Mat3::from_cols_array(&p.transform)
                .transform_point2(Vec2::ZERO)
                .to_array()
        })
        .collect();
    positions.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    assert_eq!(
        positions,
        vec![
            [0., 0.],
            [10., 0.],
            [20., 0.],
            [0., 20.],
            [10., 20.],
            [20., 20.]
        ]
    );
    assert_eq!(g.items.len(), 3);
    let mut child = root();
    child.id = 5;
    let geometry = child.items.remove(0);
    child.items.push(match geometry {
        GroupItem::Geometry {
            vector,
            size,
            position,
            ..
        } => GroupItem::Geometry {
            id: 6,
            name: "Geometry".into(),
            vector,
            size,
            position,
        },
        _ => unreachable!(),
    });
    child.transform.opacity.value = 50.;
    let mut parent = VectorGroup {
        id: 1,
        name: "Parent".into(),
        transform: Default::default(),
        items: vec![
            GroupItem::Group {
                group: Box::new(child),
            },
            repeat(7, 2., [10., 0.]),
        ],
    };
    parent.validate().unwrap();
    let v = parent.sample(0.).unwrap();
    let b = v.batches.unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].scopes.len(), 1);
    assert_eq!(b[0].scopes[0].opacity, 0.5);
    assert_ne!(b[0].scopes[0].id, b[1].scopes[0].id);
    assert_eq!(b[0].vector.fill.unwrap()[3], 1.);
    parent.transform.opacity.value = 25.;
    let v = parent.sample(0.).unwrap();
    assert_eq!(v.root_opacity, 0.25);
}
#[test]
fn paint_order_and_group_opacity_remain_distinct_from_paint_alpha() {
    let mut g = root();
    if let GroupItem::Geometry { vector, .. } = &mut g.items[0] {
        vector.fill = None;
    }
    g.items.push(GroupItem::Stroke {
        id: 3,
        name: "Stroke".into(),
        stroke: Stroke {
            color: Track::constant([1., 0., 0., 1.]),
            width: Track::constant(2.),
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.,
            dashes: None,
        },
        composite: Composite::Below,
    });
    g.items.push(GroupItem::Fill {
        id: 4,
        name: "Fill".into(),
        color: Track::constant([0., 1., 0., 1.]),
        fill_rule: FillRule::NonZero,
        composite: Composite::Below,
    });
    g.transform.opacity.value = 50.;
    g.validate().unwrap();
    let s = g.sample(0.).unwrap();
    assert_eq!(s.root_opacity, 0.5);
    let b = s.batches.unwrap();
    assert_eq!(b.len(), 2);
    assert!(b[0].vector.fill.is_some() && b[1].vector.stroke.is_some());
    assert_eq!(b[0].vector.fill.unwrap()[3], 1.);
    assert_eq!(b[1].vector.stroke.unwrap().0[3], 1.);
}
#[test]
fn group_transform_keeps_paint_coordinates_and_an_affine_basis() {
    let mut g = root();
    g.transform.position.value = [10., 20.];
    g.transform.scale.value = [200., 50.];
    g.transform.rotation.value = 90.;
    let sample = g.sample(0.).unwrap();
    let b = &sample.batches.as_ref().unwrap()[0];
    let matrix = Mat3::from_cols_array(&b.transform);
    let p = matrix.transform_point2(Vec2::new(1., 0.));
    assert!((p - Vec2::new(10., 22.)).length() < 1e-5);
    assert!((b.vector.paths[0].nodes[0][0] + 5.).abs() < 1e-5);
}
#[test]
fn grid_counts_match_common_authored_pattern_and_budget_failure_is_explicit() {
    let mut g = root();
    g.items.push(repeat(3, 23., [1064., 0.]));
    g.items.push(repeat(4, 23., [0., 844.]));
    assert_eq!(g.sample(0.).unwrap().batches.unwrap().len(), 529);
    g.items.push(repeat(5, 23., [0., 0.]));
    assert!(g.sample(0.).unwrap_err().to_string().contains("limit"));
}
#[test]
fn group_parameter_commands_use_clip_time_and_atomic_history_and_storage() {
    let mut e = Engine::new(Project::new(128, 128, 30, 120).unwrap()).unwrap();
    let mut layer = Layer::solid(1, "Group", [10.; 2], [64., 64., 0.], [1.; 4]);
    let mut g = root();
    g.items.push(repeat(3, 3., [10., 0.]));
    layer.content = Content::Vector {
        vector: VectorContent {
            source: VectorSource::Group { group: Box::new(g) },
            fill: None,
            stroke: None,
            trim: None,
            fill_rule: FillRule::NonZero,
        },
    };
    e.apply(Command::Add { layer }).unwrap();
    assert_eq!(e.project().version, 12);
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
    for (frame, value) in [(20, 3.), (40, 5.)] {
        e.apply(Command::Vector {
            object: 1,
            action: VectorAction::SetGroupParameter {
                item: 3,
                parameter: "copies".into(),
                frame,
                value: ParameterValue::Scalar(value),
                animated: Some(true),
            },
        })
        .unwrap();
    }
    let Content::Vector { vector } = &e.project().layers[0].content else {
        unreachable!()
    };
    assert!(vector.animated());
    assert_eq!(
        vector.sample(10., [10.; 2]).unwrap().group_parameters[&3]["copies"],
        serde_json::json!(4.)
    );
    let before = e.snapshot();
    e.begin_gesture().unwrap();
    for value in [4., 7.] {
        e.apply(Command::Vector {
            object: 1,
            action: VectorAction::SetGroupParameter {
                item: 3,
                parameter: "copies".into(),
                frame: 30,
                value: ParameterValue::Scalar(value),
                animated: None,
            },
        })
        .unwrap();
    }
    e.end_gesture(true).unwrap();
    e.undo().unwrap();
    assert_eq!(e.snapshot(), before);
    e.redo().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    motion_core::storage::save(tmp.path(), e.project()).unwrap();
    assert_eq!(
        motion_core::storage::load(tmp.path()).unwrap(),
        e.snapshot()
    );
    let before = e.snapshot();
    assert!(e
        .apply(Command::Vector {
            object: 1,
            action: VectorAction::SetGroupParameter {
                item: 3,
                parameter: "copies".into(),
                frame: 30,
                value: ParameterValue::Scalar(-1.),
                animated: None
            }
        })
        .is_err());
    assert_eq!(e.snapshot(), before);
}
