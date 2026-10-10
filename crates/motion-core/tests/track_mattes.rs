use motion_render::Scene;
use motion_core::{compositing::*, *};
use serde_json::json;

fn fixture() -> Project {
    let mut p = Project::new(64, 64, 24, 120).unwrap();
    p.layers = vec![
        Layer::solid(1, "Matte", [24.; 2], [32., 32., 0.], [1., 1., 1., 0.5]),
        Layer::solid(2, "Target", [32.; 2], [32., 32., 0.], [1., 0.2, 0.1, 0.7]),
    ];
    p
}
fn link() -> TrackMatte {
    TrackMatte {
        source: 1,
        mode: MatteMode::Alpha,
        hide_source: true,
    }
}

#[test]
fn matte_and_blend_edit_history_and_storage_preserve_identity() {
    let mut engine = Engine::new(fixture()).unwrap();
    let original = engine.snapshot();
    engine
        .apply_batch(vec![
            Command::SetTrackMatte {
                object: 2,
                matte: Some(link()),
            },
            Command::SetLayerBlend {
                object: 2,
                mode: Some(BlendMode::Screen),
                space: Some(BlendSpace::Srgb),
            },
        ])
        .unwrap();
    let edited = engine.snapshot();
    assert_eq!(original.version, 8);
    assert_eq!(edited.version, 10);
    assert_eq!(edited.layers[1].track_matte, Some(link()));
    assert_eq!(edited.layers[1].blend.mode, BlendMode::Screen);
    engine.undo().unwrap();
    assert_eq!(engine.snapshot(), original);
    engine.redo().unwrap();
    assert_eq!(engine.snapshot(), edited);
    let folder = tempfile::tempdir().unwrap();
    motion_core::storage::save(folder.path(), &edited).unwrap();
    assert_eq!(motion_core::storage::load(folder.path()).unwrap(), edited);
    let file = folder.path().join("owned.aem");
    motion_core::storage::export_package(folder.path(), &edited, &file).unwrap();
    assert_eq!(
        motion_core::storage::import_package(&file, &folder.path().join("imported")).unwrap(),
        edited
    );
}

#[test]
fn invalid_links_and_locked_targets_are_atomic() {
    let mut engine = Engine::new(fixture()).unwrap();
    for source in [2, 999] {
        let before = engine.snapshot();
        let revision = engine.revision();
        assert!(engine
            .apply(Command::SetTrackMatte {
                object: 2,
                matte: Some(TrackMatte { source, ..link() })
            })
            .is_err());
        assert_eq!(engine.snapshot(), before);
        assert_eq!(engine.revision(), revision);
    }
    engine
        .apply(Command::SetTrackMatte {
            object: 2,
            matte: Some(link()),
        })
        .unwrap();
    let before = engine.snapshot();
    assert!(engine
        .apply(Command::SetTrackMatte {
            object: 1,
            matte: Some(TrackMatte {
                source: 2,
                ..link()
            })
        })
        .is_err());
    assert_eq!(engine.snapshot(), before);
    let mut locked = fixture();
    locked.layers[1].locked = true;
    let mut engine = Engine::new(locked).unwrap();
    let before = engine.snapshot();
    assert!(engine
        .apply(Command::SetTrackMatte {
            object: 2,
            matte: Some(link())
        })
        .is_err());
    assert_eq!(engine.snapshot(), before);
}

#[test]
fn hidden_matte_is_sampled_and_precompose_keeps_complete_association() {
    let mut p = fixture();
    p.version = 10;
    p.layers[0].visible = false;
    p.layers[1].track_matte = Some(link());
    let mut engine = Engine::new(p).unwrap();
    let before = engine.snapshot();
    let split = parse_commands(
        &json!({"op":"composition","action":{"kind":"precompose","objects":[2],"name":"split"}})
            .to_string(),
    )
    .unwrap();
    assert!(engine.apply_batch(split).is_err());
    assert_eq!(engine.snapshot(), before);
    let together = parse_commands(&json!({"op":"composition","action":{"kind":"precompose","objects":[1,2],"name":"complete"}}).to_string()).unwrap();
    engine.apply_batch(together).unwrap();
    let mut scene = Scene::new(engine.project());
    scene.sample(engine.project(), 0., None, &motion_core::ExpressionEvaluator).unwrap();
    let child = &scene.nested[0].scene;
    let source = child
        .layers
        .iter()
        .find(|l| child.source_object(l.id) == 1)
        .unwrap();
    let target = child
        .layers
        .iter()
        .find(|l| child.source_object(l.id) == 2)
        .unwrap();
    assert!(!source.composite_visible);
    assert!(target.composite_visible);
    assert_eq!(target.track_matte.unwrap().source, source.id);
}

#[test]
fn duplicating_target_keeps_shared_source_and_source_deletion_is_rejected() {
    let mut engine = Engine::new(fixture()).unwrap();
    engine
        .apply(Command::SetTrackMatte {
            object: 2,
            matte: Some(link()),
        })
        .unwrap();
    engine.apply(Command::Duplicate { object: 2 }).unwrap();
    assert_eq!(engine.project().layers[2].track_matte, Some(link()));
    let before = engine.snapshot();
    assert!(engine.apply(Command::Delete { object: 1 }).is_err());
    assert_eq!(engine.snapshot(), before);
    assert!(engine
        .apply(Command::Remove {
            object: 1,
            frame: 0
        })
        .is_err());
    assert_eq!(engine.snapshot(), before);
}

#[test]
fn separable_transfer_matches_known_color_values() {
    for (mode, expected) in [
        (BlendMode::Normal, 0.7),
        (BlendMode::Add, 0.9),
        (BlendMode::Multiply, 0.14),
        (BlendMode::Screen, 0.76),
        (BlendMode::Overlay, 0.28),
        (BlendMode::Darken, 0.2),
        (BlendMode::Lighten, 0.7),
        (BlendMode::Difference, 0.5),
        (BlendMode::Exclusion, 0.62),
        (BlendMode::Subtract, 0.),
        (BlendMode::HardLight, 0.52),
    ] {
        assert!(
            (transfer(mode, 0.2, 0.7) - expected).abs() < 0.00001,
            "{mode:?}"
        );
    }
}
