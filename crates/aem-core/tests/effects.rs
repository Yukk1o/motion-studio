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
    assert_eq!(restored.version, 2);
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
    assert_eq!(migrated.project().version, 2);
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
