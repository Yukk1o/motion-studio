use motion_core::{
    plugin_editor::{EditorRequest, PluginEditorSession},
    Command, EffectAction, EffectInstance, Engine, Layer, Project,
};
use motion_effects::{builtin, Registry};

fn setup() -> (Engine, Registry) {
    let registry = Registry::new_with_builtins().unwrap();
    let package = builtin::scene_package().unwrap();
    let definition = &package.manifest.effects[0];
    let mut p = Project::new(128, 128, 30, 120).unwrap();
    p.layers.push(Layer::solid(
        1,
        "光效",
        [128., 128.],
        [64., 64., 0.],
        [0., 0., 0., 0.],
    ));
    let mut engine = Engine::new(p).unwrap();
    engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::Insert {
                instance: EffectInstance::new(
                    1,
                    &package.manifest.id,
                    &package.manifest.version,
                    &package.hash,
                    definition,
                    [128., 128.],
                ),
            },
        })
        .unwrap();
    (engine, registry)
}

#[test]
fn editor_transform_values_use_sampled_local_keyframe_time() {
    let (engine, registry) = setup();
    let mut project = engine.snapshot();
    project.layers[0].timeline = Some(motion_core::LayerTimeline {
        in_frame: 0,
        out_frame: 120,
        offset_frame: 10,
    });
    project.layers[0].transform.position.keys = vec![
        motion_core::Keyframe {
            frame: 0,
            value: [20., 64., 0.],
            ease: motion_core::Ease::Linear,
            curve: None,
            spatial: None,
        },
        motion_core::Keyframe {
            frame: 40,
            value: [100., 64., 0.],
            ease: motion_core::Ease::Linear,
            curve: None,
            spatial: None,
        },
    ];
    let engine = Engine::new(project).unwrap();
    let editor = PluginEditorSession::open(engine.project(), &registry, 1, 1).unwrap();
    let state = editor.state(&engine, 30).unwrap();
    assert_eq!(
        state["transform_values"]["position"],
        serde_json::json!([60., 64., 0.])
    );
    assert_eq!(
        state["transform"]["position"]["value"],
        serde_json::json!([64., 64., 0.])
    );
}
#[test]
fn custom_editor_gesture_commit_cancel_and_storage_preserve_components() {
    let (mut engine, registry) = setup();
    assert_eq!(engine.project().version, 8);
    let mut editor = PluginEditorSession::open(engine.project(), &registry, 1, 1).unwrap();
    let original = engine.snapshot();
    editor
        .request(&mut engine, 0, EditorRequest::Begin { revision: 1 })
        .unwrap();
    for value in [2., 3., 4.] {
        let revision = engine.revision();
        editor
            .request(
                &mut engine,
                0,
                EditorRequest::Set {
                    revision,
                    param: "intensity".into(),
                    value: [value, 0., 0., 0.],
                },
            )
            .unwrap();
    }
    let revision = engine.revision();
    editor
        .request(&mut engine, 0, EditorRequest::Commit { revision })
        .unwrap();
    assert!(engine.undo().unwrap());
    assert_eq!(engine.project(), &original);
    assert!(engine.redo().unwrap());
    assert_eq!(
        editor.state(&engine, 0).unwrap()["values"]["intensity"][0],
        4.
    );
    let after = engine.snapshot();
    let revision = engine.revision();
    editor
        .request(&mut engine, 0, EditorRequest::Begin { revision })
        .unwrap();
    let mut settings = after.layers[0].effects[0].scene.clone().unwrap();
    settings.elements.reverse();
    settings.elements[0].enabled = false;
    editor
        .request(&mut engine, 0, EditorRequest::Scene { revision, settings })
        .unwrap();
    editor.close(&mut engine, false).unwrap();
    assert_eq!(engine.project(), &after);
    let json = serde_json::to_string(engine.project()).unwrap();
    assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), after);
}
#[test]
fn editor_rejects_stale_edits_invalid_components_and_locked_objects_atomically() {
    let (mut engine, registry) = setup();
    let mut editor = PluginEditorSession::open(engine.project(), &registry, 1, 1).unwrap();
    let revision = engine.revision();
    editor
        .request(
            &mut engine,
            0,
            EditorRequest::Transform {
                revision,
                property: motion_core::Property::Position,
                value: [80., 64., 5.],
            },
        )
        .unwrap();
    assert_eq!(
        engine.project().layers[0].transform.position.value,
        [80., 64., 5.]
    );
    let revision = engine.revision();
    assert!(editor
        .request(
            &mut engine,
            0,
            EditorRequest::Transform {
                revision,
                property: motion_core::Property::Fov,
                value: [60., 0., 0.]
            }
        )
        .is_err());
    assert!(engine.undo().unwrap());
    let before = engine.snapshot();
    assert!(editor
        .request(
            &mut engine,
            0,
            EditorRequest::Set {
                revision: 0,
                param: "intensity".into(),
                value: [2., 0., 0., 0.]
            }
        )
        .is_err());
    let current_revision = engine.revision();
    let mut settings = before.layers[0].effects[0].scene.clone().unwrap();
    settings.elements[0].size[0] = -1.;
    assert!(editor
        .request(
            &mut engine,
            0,
            EditorRequest::Scene {
                revision: current_revision,
                settings
            }
        )
        .is_err());
    assert_eq!(engine.project(), &before);
    engine
        .apply(Command::Flags {
            object: 1,
            visible: true,
            locked: true,
        })
        .unwrap();
    let revision = engine.revision();
    assert!(editor
        .request(&mut engine, 0, EditorRequest::Seed { revision, seed: 77 })
        .is_err());
    assert!(serde_json::from_str::<EditorRequest>(
        r#"{"op":"set","revision":1,"object":2,"param":"intensity","value":[1,0,0,0]}"#
    )
    .is_err());
}
#[test]
fn editor_tracks_keys_uses_clip_time_and_invalidates_removed_instances() {
    let (mut engine, registry) = setup();
    let mut editor = PluginEditorSession::open(engine.project(), &registry, 1, 1).unwrap();
    editor
        .request(
            &mut engine,
            0,
            EditorRequest::Animate {
                revision: 1,
                param: "intensity".into(),
                enabled: true,
            },
        )
        .unwrap();
    let revision = engine.revision();
    editor
        .request(
            &mut engine,
            30,
            EditorRequest::Set {
                revision,
                param: "intensity".into(),
                value: [3., 0., 0., 0.],
            },
        )
        .unwrap();
    assert_eq!(
        editor.state(&engine, 15).unwrap()["values"]["intensity"][0],
        2.
    );
    engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::Remove { effect: 1 },
        })
        .unwrap();
    assert!(editor.state(&engine, 0).is_err());
}
