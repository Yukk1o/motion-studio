use motion_core::{vector::*, Command, Content, Engine, Layer, Project, Track};

fn line(x: f32, length: f32) -> SampledPath {
    SampledPath {
        closed: false,
        nodes: vec![[x, 0., 0., 0., 0., 0.], [x + length, 0., 0., 0., 0., 0.]],
    }
}
fn trim(start: f32, end: f32, offset: f32, mode: TrimMode) -> SampledTrimPaths {
    SampledTrimPaths {
        start,
        end,
        offset,
        mode,
    }
}
#[test]
fn length_based_trim_wrap_modes_and_closed_seams() {
    let paths = vec![line(0., 100.), line(200., 300.)];
    let simultaneous =
        path_ops::trim(&paths, &trim(0., 50., 0., TrimMode::Simultaneously)).unwrap();
    assert_eq!(simultaneous[0].nodes.last().unwrap()[0], 50.);
    assert_eq!(simultaneous[1].nodes.last().unwrap()[0], 350.);
    let individual = path_ops::trim(&paths, &trim(0., 50., 0., TrimMode::Individually)).unwrap();
    assert_eq!(individual[0], paths[0]);
    assert_eq!(individual[1].nodes.last().unwrap()[0], 300.);
    assert!(
        path_ops::trim(&paths, &trim(40., 40., 0., TrimMode::Simultaneously))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        path_ops::trim(&paths, &trim(100., 0., -720., TrimMode::Simultaneously)).unwrap(),
        paths
    );
    let square = SampledPath {
        closed: true,
        nodes: vec![
            [0., 0., 0., 0., 0., 0.],
            [100., 0., 0., 0., 0., 0.],
            [100., 100., 0., 0., 0., 0.],
            [0., 100., 0., 0., 0., 0.],
        ],
    };
    let wrapped =
        path_ops::trim(&[square], &trim(0., 50., 270., TrimMode::Simultaneously)).unwrap();
    assert_eq!(wrapped.len(), 1);
    assert!(!wrapped[0].closed);
    assert_eq!(&wrapped[0].nodes[0][..2], &[0., 100.]);
    assert_eq!(&wrapped[0].nodes[1][..2], &[0., 0.]);
    assert_eq!(&wrapped[0].nodes[2][..2], &[100., 0.]);
}
#[test]
fn cubic_cuts_keep_tangents_and_use_arc_length_not_uniform_parameter() {
    let p = SampledPath {
        closed: false,
        nodes: vec![[0., 0., 0., 0., 0., 100.], [100., 0., 0., 100., 0., 0.]],
    };
    let half = path_ops::trim(&[p], &trim(0., 50., 0., TrimMode::Simultaneously)).unwrap();
    let end = half[0].nodes.last().unwrap();
    assert!((end[0] - 50.).abs() < 0.03 && (end[1] - 75.).abs() < 0.03);
    assert!(half[0].nodes[0][5] > 0. && end[2].abs() > 0.);
    // Collinear but very nonuniform parameterization; halfway is x=50,
    // not cubic(t=.5)=87.5.
    let collinear = SampledPath {
        closed: false,
        nodes: vec![[0., 0., 0., 0., 100., 0.], [100., 0., 0., 0., 0., 0.]],
    };
    let half = path_ops::trim(&[collinear], &trim(0., 50., 0., TrimMode::Simultaneously)).unwrap();
    assert!((half[0].nodes.last().unwrap()[0] - 50.).abs() < 0.03);
}
#[test]
fn dash_phase_pairs_continuity_and_resource_error_are_explicit() {
    let d = |offset| SampledDashes {
        pattern: vec![10., 10.],
        offset,
    };
    let out = path_ops::dash(&[line(0., 50.)], &d(5.)).unwrap();
    assert_eq!(
        out.iter()
            .map(|p| (p.nodes[0][0], p.nodes.last().unwrap()[0]))
            .collect::<Vec<_>>(),
        vec![(0., 5.), (15., 25.), (35., 45.)]
    );
    let p = line(0., 100.);
    assert_eq!(
        path_ops::dash(
            &[p.clone()],
            &SampledDashes {
                pattern: vec![0.1, 0.],
                offset: 4.
            }
        )
        .unwrap(),
        vec![p]
    );
    assert!(path_ops::dash(
        &[line(0., 32768.)],
        &SampledDashes {
            pattern: vec![0.1, 0.1],
            offset: 0.
        }
    )
    .unwrap_err()
    .to_string()
    .contains("limit exceeded"));
    assert!(path_ops::dash(
        &[line(0., 50.)],
        &SampledDashes {
            pattern: vec![0., 2.],
            offset: 0.
        }
    )
    .is_err());
    assert!(path_ops::dash(&[line(0., 32768.)], &SampledDashes {
        pattern: vec![0.1, 1e-20], offset: 0.
    }).unwrap_err().to_string().contains("iteration limit"));
}
fn edit(object: u64, action: VectorAction) -> Command {
    Command::Vector { object, action }
}
#[test]
fn modifiers_animate_in_clip_time_save_restore_and_undo_atomically() {
    let mut e = Engine::new(Project::new(128, 128, 30, 120).unwrap()).unwrap();
    let mut layer = Layer::solid(1, "line", [100.; 2], [64., 64., 0.], [1.; 4]);
    layer.content = Content::Vector {
        vector: VectorContent::shape(ShapeKind::Line),
    };
    e.apply(Command::Add { layer }).unwrap();
    let old = e.snapshot();
    e.apply_batch(vec![
        edit(
            1,
            VectorAction::SetTrim {
                trim: Some(TrimPaths::default()),
            },
        ),
        edit(
            1,
            VectorAction::SetDashes {
                dashes: Some(StrokeDashes::default()),
            },
        ),
    ])
    .unwrap();
    assert_eq!(e.project().version, 11);
    e.undo().unwrap();
    assert_eq!(e.snapshot(), old);
    e.redo().unwrap();
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
    for (frame, value) in [(20, 0.), (40, 100.)] {
        e.apply(edit(
            1,
            VectorAction::SetModifierParameter {
                parameter: "trim_end".into(),
                frame,
                value,
                animated: Some(true),
            },
        ))
        .unwrap();
    }
    let Content::Vector { vector } = &e.project().layers[0].content else {
        unreachable!()
    };
    assert!(vector.animated());
    assert_eq!(
        vector.sample(10., [100.; 2]).unwrap().trim.unwrap().end,
        50.
    );
    assert_eq!(
        vector
            .trim
            .as_ref()
            .unwrap()
            .end
            .keys
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 20]
    );
    let tmp = tempfile::tempdir().unwrap();
    motion_core::storage::save(tmp.path(), e.project()).unwrap();
    assert_eq!(
        motion_core::storage::load(tmp.path()).unwrap(),
        e.snapshot()
    );
    let before = e.snapshot();
    assert!(e
        .apply_batch(vec![
            edit(
                1,
                VectorAction::SetModifierParameter {
                    parameter: "trim_end".into(),
                    frame: 30,
                    value: 90.,
                    animated: None
                }
            ),
            edit(
                1,
                VectorAction::SetModifierParameter {
                    parameter: "dash_0".into(),
                    frame: 30,
                    value: -1.,
                    animated: None
                }
            )
        ])
        .is_err());
    assert_eq!(e.snapshot(), before);
    assert!(e
        .apply(edit(
            1,
            VectorAction::SetModifierParameter {
                parameter: "trim_end".into(),
                frame: 120,
                value: 50.,
                animated: None
            }
        ))
        .is_err());
    e.apply(Command::Flags {
        object: 1,
        visible: true,
        locked: true,
    })
    .unwrap();
    assert!(e
        .apply(edit(1, VectorAction::SetTrim { trim: None }))
        .is_err());
}
#[test]
fn old_vectors_serialize_unchanged_and_validate_modifier_ranges() {
    let mut v = VectorContent::shape(ShapeKind::Line);
    let old = serde_json::to_value(&v).unwrap();
    assert!(old.get("trim").is_none() && old["stroke"].get("dashes").is_none());
    assert_eq!(serde_json::from_value::<VectorContent>(old).unwrap(), v);
    v.trim = Some(TrimPaths {
        start: Track::constant(-1.),
        ..Default::default()
    });
    assert!(v.validate().is_err());
    v.trim.as_mut().unwrap().start.value = 0.;
    v.stroke.as_mut().unwrap().dashes = Some(StrokeDashes {
        pattern: vec![Track::constant(1.)],
        ..Default::default()
    });
    assert!(v.validate().is_err());
}
