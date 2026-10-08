use aem_core::{Command, EffectAction, EffectInstance, Engine, Layer, Project, Scene};
fn fixture(effect: &str) -> (Engine, u64) {
    let package = aem_effects::builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == effect)
        .unwrap();
    let mut project = Project::new(64, 64, 30, 60).unwrap();
    project.layers.push(Layer::solid(
        1,
        "test",
        [64.0; 2],
        [32.0, 32.0, 0.0],
        [1.0; 4],
    ));
    let instance = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        [64.0; 2],
    );
    let mut engine = Engine::new(project).unwrap();
    engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::Insert { instance },
        })
        .unwrap();
    (engine, 1)
}
fn apply(engine: &mut Engine, action: EffectAction) {
    engine.apply(Command::Effect { object: 1, action }).unwrap();
}

#[test]
fn common_controls_reject_invalid_edits_atomically_and_validate_saved_keys() {
    let (mut engine, id) = fixture("posterize");
    for value in [1., 256., f32::NAN] {
        let before = engine.project().clone();
        assert!(engine
            .apply(Command::Effect {
                object: 1,
                action: EffectAction::Set {
                    effect: id,
                    param: "levels".into(),
                    frame: 0,
                    value: [value, 0., 0., 0.],
                }
            })
            .is_err());
        assert_eq!(engine.project(), &before);
    }
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "levels".into(),
            frame: 0,
            enabled: true,
        },
    );
    apply(
        &mut engine,
        EffectAction::Set {
            effect: id,
            param: "levels".into(),
            frame: 30,
            value: [255., 0., 0., 0.],
        },
    );
    let mut invalid = engine.project().clone();
    invalid.layers[0].effects[0]
        .params
        .get_mut("levels")
        .unwrap()
        .track
        .keys[1]
        .value[0] = 256.;
    assert!(invalid.validate().is_err());
    engine.project().validate().unwrap();
    let before = engine.project().clone();
    engine.undo().unwrap();
    engine.redo().unwrap();
    assert_eq!(engine.project(), &before);
}
#[test]
fn ordered_instances_dependencies_and_gesture_undo_survive_storage() {
    let (mut engine, id) = fixture("tint");
    let before = engine.project().clone();
    engine.begin_gesture().unwrap();
    for value in [10.0, 30.0, 70.0] {
        apply(
            &mut engine,
            EffectAction::Set {
                effect: id,
                param: "p0003".into(),
                frame: 0,
                value: [value, 0.0, 0.0, 0.0],
            },
        );
    }
    engine.end_gesture(true).unwrap();
    engine.undo().unwrap();
    assert_eq!(&before, engine.project());
    engine.redo().unwrap();
    assert_eq!(
        engine.project().layers[0].effects[0].params["p0003"].sample(0.0)[0],
        70.0
    );
    apply(&mut engine, EffectAction::Duplicate { effect: id });
    assert_eq!(engine.project().plugin_dependencies.len(), 1);
    assert_eq!(engine.project().layers[0].effects[1].id, 2);
    apply(
        &mut engine,
        EffectAction::Move {
            effect: 2,
            index: 0,
        },
    );
    let text = serde_json::to_string(engine.project()).unwrap();
    let restored: Project = serde_json::from_str(&text).unwrap();
    restored.validate().unwrap();
    assert_eq!(&restored, engine.project());
    assert_eq!(restored.version, 8);
    apply(&mut engine, EffectAction::Remove { effect: 1 });
    apply(&mut engine, EffectAction::Remove { effect: 2 });
    assert!(engine.project().plugin_dependencies.is_empty());
}
#[test]
fn numeric_animation_and_curve_luts_are_independent_of_seek_order() {
    let (mut engine, id) = fixture("curves");
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0001".into(),
            frame: 0,
            enabled: true,
        },
    );
    let mut invert = aem_core::CurveObject::default();
    invert.channels[0] = vec![[0.0, 1.0], [1.0, 0.0]];
    apply(
        &mut engine,
        EffectAction::SetCurveObject {
            effect: id,
            param: "p0001".into(),
            frame: 30,
            value: invert,
        },
    );
    let mut scene = Scene::new(engine.project());
    scene.sample(engine.project(), 15.0, None).unwrap();
    let middle = scene.curve_luts.clone();
    for frame in [29.0, 0.0, 59.0, 9.0, 15.0] {
        scene.sample(engine.project(), frame, None).unwrap();
    }
    assert_eq!(middle, scene.curve_luts);
    assert!((middle[0][0][0] - 0.5).abs() < 1e-6);
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0001".into(),
            frame: 15,
            enabled: false,
        },
    );
    scene.sample(engine.project(), 15.0, None).unwrap();
    assert_eq!(middle, scene.curve_luts);
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0001".into(),
            frame: 15,
            enabled: true,
        },
    );
    let mut invert = aem_core::CurveObject::default();
    invert.channels[0] = vec![[0.0, 1.0], [1.0, 0.0]];
    apply(
        &mut engine,
        EffectAction::SetCurveObject {
            effect: id,
            param: "p0001".into(),
            frame: 30,
            value: invert,
        },
    );
    apply(
        &mut engine,
        EffectAction::CopyKey {
            effect: id,
            param: "p0001".into(),
            from: 30,
            to: 45,
        },
    );
    assert_eq!(
        engine.project().layers[0].effects[0].params["p0001"]
            .curve
            .as_ref()
            .unwrap()
            .keys
            .len(),
        3
    );
    apply(
        &mut engine,
        EffectAction::MoveKey {
            effect: id,
            param: "p0001".into(),
            from: 45,
            to: 50,
        },
    );
    assert_eq!(
        engine.project().layers[0].effects[0].params["p0001"]
            .curve
            .as_ref()
            .unwrap()
            .keys
            .last()
            .unwrap()
            .frame,
        50
    );
}
#[test]
fn discrete_animation_holds_and_rejected_edits_are_atomic() {
    let (mut engine, id) = fixture("gaussian_blur");
    let mut p = engine.project().clone();
    p.layers[0].effects[0]
        .params
        .get_mut("p0002")
        .unwrap()
        .animatable = true;
    engine = Engine::new(p).unwrap();
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0002".into(),
            frame: 0,
            enabled: true,
        },
    );
    apply(
        &mut engine,
        EffectAction::Set {
            effect: id,
            param: "p0002".into(),
            frame: 30,
            value: [3.0, 0.0, 0.0, 0.0],
        },
    );
    let param = &engine.project().layers[0].effects[0].params["p0002"];
    assert_eq!(param.sample(29.99)[0], 1.0);
    assert_eq!(param.sample(30.0)[0], 3.0);
    let before = engine.project().clone();
    assert!(engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::Set {
                effect: id,
                param: "p0002".into(),
                frame: 30,
                value: [2.5, 0.0, 0.0, 0.0]
            }
        })
        .is_err());
    assert_eq!(&before, engine.project());
}
#[test]
fn version_one_migrates_and_missing_dependency_keeps_all_keys() {
    let mut old = Project::demo();
    old.version = 1;
    let migrated = Engine::new(old).unwrap();
    assert_eq!(migrated.project().version, 8);
    let (engine, _) = fixture("tint");
    let mut missing = engine.project().clone();
    missing.layers[0].effects[0].hash = "a".repeat(64);
    missing.rebuild_plugin_dependencies();
    assert!(Engine::new(missing).is_ok());
}

#[test]
fn disabled_curves_keep_saved_keys_without_materializing_parameter_resources() {
    let (mut engine, id) = fixture("curves");
    apply(
        &mut engine,
        EffectAction::Enable {
            effect: id,
            enabled: false,
        },
    );
    let mut scene = Scene::new(engine.project());
    scene.sample(engine.project(), 0.0, None).unwrap();
    assert!(scene.curve_luts.is_empty());
    assert!(scene.effects[0].lut.is_none());
    assert!(engine.project().layers[0].effects[0].params["p0001"]
        .curve
        .is_some());
    apply(
        &mut engine,
        EffectAction::Enable {
            effect: id,
            enabled: true,
        },
    );
    scene.sample(engine.project(), 0.0, None).unwrap();
    assert_eq!(scene.curve_luts.len(), 1);
}

#[test]
fn effect_keys_follow_clip_local_time_through_trim_move_split_and_undo() {
    let (mut engine, id) = fixture("tint");
    apply(
        &mut engine,
        EffectAction::Set {
            effect: id,
            param: "p0003".into(),
            frame: 0,
            value: [0.0; 4],
        },
    );
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0003".into(),
            frame: 0,
            enabled: true,
        },
    );
    apply(
        &mut engine,
        EffectAction::Set {
            effect: id,
            param: "p0003".into(),
            frame: 30,
            value: [100.0, 0.0, 0.0, 0.0],
        },
    );
    engine
        .apply(Command::TrimLayerClip {
            object: 1,
            in_frame: 10,
            out_frame: 50,
        })
        .unwrap();
    engine
        .apply(Command::MoveLayerClip {
            object: 1,
            in_frame: 0,
        })
        .unwrap();
    let mut scene = Scene::new(engine.project());
    for frame in [0.0, 19.0, 5.0] {
        scene.sample(engine.project(), frame, None).unwrap();
        let effect = &scene.effects[0];
        assert_eq!(effect.local_frame, frame + 10.0);
        let index = effect
            .param_ids
            .iter()
            .position(|id| id == "p0003")
            .unwrap();
        assert!((effect.values[index][0] - ((frame + 10.0) / 30.0 * 100.0) as f32).abs() < 0.001);
    }
    let before_split = engine.project().clone();
    engine
        .apply(Command::SplitLayerClip {
            object: 1,
            frame: 20,
        })
        .unwrap();
    assert_eq!(
        engine.project().layers[0].effects,
        engine.project().layers[1].effects
    );
    scene.sample(engine.project(), 20.0, None).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert_eq!(scene.layers[0].id, engine.project().layers[1].id);
    assert_eq!(scene.effects[1].local_frame, 30.0);
    engine.undo().unwrap();
    assert_eq!(engine.project(), &before_split);
    engine
        .apply(Command::MoveLayerClip {
            object: 1,
            in_frame: 20,
        })
        .unwrap();
    apply(
        &mut engine,
        EffectAction::Set {
            effect: id,
            param: "p0003".into(),
            frame: 5,
            value: [25.0, 0.0, 0.0, 0.0],
        },
    );
    assert!(engine.project().layers[0].effects[0].params["p0003"]
        .track
        .keys
        .iter()
        .any(|k| k.frame == -5));
    apply(
        &mut engine,
        EffectAction::CopyKey {
            effect: id,
            param: "p0003".into(),
            from: 5,
            to: 6,
        },
    );
    apply(
        &mut engine,
        EffectAction::MoveKey {
            effect: id,
            param: "p0003".into(),
            from: 6,
            to: 7,
        },
    );
    apply(
        &mut engine,
        EffectAction::DeleteKey {
            effect: id,
            param: "p0003".into(),
            frame: 7,
        },
    );
    let restored: Project =
        serde_json::from_str(&serde_json::to_string(engine.project()).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, *engine.project());
}

#[test]
fn curve_object_keys_support_negative_local_time_and_full_signed_span() {
    let (mut engine, id) = fixture("curves");
    engine
        .apply(Command::TrimLayerClip {
            object: 1,
            in_frame: 0,
            out_frame: 40,
        })
        .unwrap();
    engine
        .apply(Command::MoveLayerClip {
            object: 1,
            in_frame: 20,
        })
        .unwrap();
    apply(
        &mut engine,
        EffectAction::Animate {
            effect: id,
            param: "p0001".into(),
            frame: 5,
            enabled: true,
        },
    );
    let mut altered = aem_core::CurveObject::default();
    altered.channels[1] = vec![[0.0, 0.0], [1.0, 0.0]];
    apply(
        &mut engine,
        EffectAction::SetCurveObject {
            effect: id,
            param: "p0001".into(),
            frame: 25,
            value: altered.clone(),
        },
    );
    let curve = engine.project().layers[0].effects[0].params["p0001"]
        .curve
        .as_ref()
        .unwrap();
    assert_eq!(
        curve.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
        [-15, 5]
    );
    let mut scene = Scene::new(engine.project());
    scene.sample(engine.project(), 20.0, None).unwrap();
    assert!((scene.curve_luts[0][255][0] - 0.25).abs() < 0.001);
    let mut project = engine.project().clone();
    let curve = project.layers[0].effects[0]
        .params
        .get_mut("p0001")
        .unwrap()
        .curve
        .as_mut()
        .unwrap();
    curve.keys[0].frame = i32::MIN;
    curve.keys[1].frame = i32::MAX;
    assert!((curve.sample(0.0)[255][0] - 0.5).abs() < 0.001);
    project.validate().unwrap();
    let unsupported = serde_json::from_value::<aem_core::Track<[f32; 4]>>(serde_json::json!({
        "axes": {"x":{"value":0,"keys":[]},"y":{"value":0,"keys":[]},"z":{"value":0,"keys":[]}}
    }));
    assert!(
        unsupported.is_err(),
        "four-component effect parameters cannot become XYZ tracks"
    );
}
