use aem_core::{Command, Ease, Engine, Layer, Project, Property, SpatialTangents, Track};

fn project() -> Project {
    let mut p = Project::new(320, 240, 30, 90).unwrap();
    p.camera.created = false;
    let mut l = Layer::solid(1, "Path", [32., 32.], [20., 40., 0.], [1.; 4]);
    l.transform
        .position
        .upsert(0, [20., 40., 0.], Ease::Linear)
        .unwrap();
    l.transform
        .position
        .upsert(60, [260., 40., 0.], Ease::Linear)
        .unwrap();
    p.layers.push(l);
    p
}

#[test]
fn cubic_path_keeps_endpoints_and_independent_temporal_easing() {
    let mut t = Track::constant([0., 0., 0.]);
    t.upsert(0, [0., 0., 0.], Ease::Linear).unwrap();
    t.upsert(60, [120., 0., 0.], Ease::Linear).unwrap();
    t.set_spatial(
        0,
        Some(SpatialTangents {
            incoming: None,
            outgoing: Some([40., 80., 0.]),
        }),
    )
    .unwrap();
    t.set_spatial(
        60,
        Some(SpatialTangents {
            incoming: Some([-40., 80., 0.]),
            outgoing: None,
        }),
    )
    .unwrap();
    assert_eq!(t.sample(0.), [0., 0., 0.]);
    assert_eq!(t.sample(60.), [120., 0., 0.]);
    assert_eq!(t.sample(30.), [60., 60., 0.]);
    t.set_ease(0, Ease::In).unwrap();
    assert_eq!(t.sample(30.), [30., 45., 0.]);
    t.set_ease(0, Ease::Hold).unwrap();
    assert_eq!(t.sample(59.), [0., 0., 0.]);
    assert_eq!(t.sample(60.), [120., 0., 0.]);
    for f in [17., 44., 9., 17.] {
        assert_eq!(t.sample(f), [0., 0., 0.]);
    }
    assert!(t.separate().is_err());
}

#[test]
fn spatial_edit_is_atomic_and_round_trips_history_and_format() {
    let mut e = Engine::new(project()).unwrap();
    let before = e.snapshot();
    let command = Command::Spatial {
        object: 1,
        property: Property::Position,
        frame: 0,
        tangents: Some(SpatialTangents {
            incoming: None,
            outgoing: Some([80., 120., 0.]),
        }),
    };
    e.apply(command).unwrap();
    assert_eq!(e.project().version, 9);
    let saved = serde_json::to_string(e.project()).unwrap();
    let restored: Project = serde_json::from_str(&saved).unwrap();
    restored.validate().unwrap();
    assert_eq!(
        restored.layers[0].transform.position.sample(30.),
        [140., 85., 0.]
    );
    e.undo().unwrap();
    assert_eq!(e.project().layers, before.layers);
    e.redo().unwrap();
    assert_eq!(e.project().layers, restored.layers);
    let revision = e.revision();
    let bad = Command::Spatial {
        object: 1,
        property: Property::Position,
        frame: 60,
        tangents: Some(SpatialTangents {
            incoming: None,
            outgoing: Some([0., 0., 0.]),
        }),
    };
    assert!(e.apply(bad).is_err());
    assert_eq!(e.revision(), revision);
    assert_eq!(e.project().layers, restored.layers);
    assert_eq!(Engine::new(restored).unwrap().project().version, 9);
}

#[test]
fn moving_copying_and_editing_keys_preserves_spatial_handles() {
    let mut t = project().layers[0].transform.position.clone();
    t.set_spatial(
        0,
        Some(SpatialTangents {
            incoming: None,
            outgoing: Some([80., 120., 0.]),
        }),
    )
    .unwrap();
    let handle = t.keys[0].spatial.clone();
    t.set_at(0, [30., 50., 0.]).unwrap();
    assert_eq!(t.keys[0].spatial, handle);
    t.copy_key(0, 15).unwrap();
    assert_eq!(t.keys[1].spatial, handle);
    t.move_key(15, 20).unwrap();
    assert_eq!(t.keys[1].spatial, handle);
    t.validate_local().unwrap();
    t.delete_key(20).unwrap();
    t.set_spatial(0, None).unwrap();
    t.separate().unwrap();
}

#[test]
fn path_query_matches_flat_parent_projection_without_seeking_or_mutating() {
    let mut p = project();
    p.layers[0].three_d = false;
    p.layers[0]
        .transform
        .position
        .set_spatial(
            0,
            Some(SpatialTangents {
                incoming: None,
                outgoing: Some([80., 120., 0.]),
            }),
        )
        .unwrap();
    p.version = 9;
    let original = p.clone();
    let mut scene = aem_core::Scene::new(&p);
    scene.sample(&p, 30., None).unwrap();
    let target = aem_core::position_path::PositionTarget::Property {
        object: 1,
        property: Property::Position,
    };
    let path = aem_core::position_path::sample(&p, &scene, &target, 30.).unwrap();
    assert_eq!(path["value"], serde_json::json!([140., 85., 0.]));
    let matrix = glam::Mat4::from_cols_array(&std::array::from_fn(|i| {
        path["matrix"][i].as_f64().unwrap() as f32
    }));
    let projected = matrix * glam::Vec4::new(140., 85., 0., 1.);
    let x = (projected.x / projected.w + 1.) * 160.;
    let y = (1. - projected.y / projected.w) * 120.;
    assert!((x - 140.).abs() < 0.001 && (y - 85.).abs() < 0.001);
    assert_eq!(
        path["keys"][0]["outgoing"],
        serde_json::json!([100., 160., 0.])
    );
    assert_eq!(path["samples"].as_array().unwrap().len(), 257);
    assert_eq!(p, original);
    assert_eq!(scene.frame, 30.);
}

#[test]
fn native_parameter_scope_cancels_only_its_own_track() {
    use aem_core::{
        plugin_editor::{EditorRequest, PluginEditorSession},
        EffectAction, EffectInstance,
    };
    let package = aem_effects::builtin::particle_package().unwrap();
    let registry = aem_effects::Registry::new_with_builtins().unwrap();
    let mut p = project();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "particle_emitter")
        .unwrap();
    p.layers[0].effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        [32., 32.],
    ));
    p.rebuild_plugin_dependencies();
    let mut e = Engine::new(p).unwrap();
    let mut editor = PluginEditorSession::open(e.project(), &registry, 1, 1).unwrap();
    editor
        .request(&mut e, 0, EditorRequest::Begin { revision: 0 })
        .unwrap();
    e.apply(Command::Effect {
        object: 1,
        action: EffectAction::Set {
            effect: 1,
            param: "rate".into(),
            frame: 0,
            value: [500., 0., 0., 0.],
        },
    })
    .unwrap();
    let before = e.project().layers[0].effects[0].params["position"]
        .track
        .clone();
    let revision = e.revision();
    editor
        .request(
            &mut e,
            0,
            EditorRequest::ParameterBegin {
                revision,
                param: "position".into(),
            },
        )
        .unwrap();
    editor
        .request(
            &mut e,
            0,
            EditorRequest::Set {
                revision,
                param: "position".into(),
                value: [30., 40., 0., 0.],
            },
        )
        .unwrap();
    let revision = e.revision();
    editor
        .request(
            &mut e,
            0,
            EditorRequest::ParameterFinish {
                revision,
                commit: false,
            },
        )
        .unwrap();
    assert_eq!(
        e.project().layers[0].effects[0].params["position"].track,
        before
    );
    assert_eq!(
        e.project().layers[0].effects[0].params["rate"].track.value[0],
        500.
    );
    let revision = e.revision();
    editor
        .request(&mut e, 0, EditorRequest::Commit { revision })
        .unwrap();
    assert!(editor.state(&e, 0).unwrap()["parameter_edit"].is_null());
    e.undo().unwrap();
    assert_ne!(
        e.project().layers[0].effects[0].params["rate"].track.value[0],
        500.
    );
}
