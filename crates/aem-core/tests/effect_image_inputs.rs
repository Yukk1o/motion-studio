use aem_core::{
    Command, EffectAction, EffectImageInput, EffectImageStage, EffectInstance, Engine, Layer,
    Project,
};

fn effect() -> EffectInstance {
    let p = aem_effects::builtin::package().unwrap();
    EffectInstance::new(
        1,
        &p.manifest.id,
        &p.manifest.version,
        &p.hash,
        p.manifest
            .effects
            .iter()
            .find(|d| d.id == "displacement_map")
            .unwrap(),
        [64.; 2],
    )
}
fn fixture() -> Project {
    let mut p = Project::new(64, 64, 30, 30).unwrap();
    p.layers = vec![
        Layer::solid(1, "a", [64.; 2], [32., 32., 0.], [1.; 4]),
        Layer::solid(2, "b", [64.; 2], [32., 32., 0.], [1.; 4]),
    ];
    p.layers[0].effects.push(effect());
    p.layers[1].effects.push(effect());
    p.rebuild_plugin_dependencies();
    p
}
#[test]
fn effects_graph_rejects_cycles_but_allows_source_stage_and_disabled_edges() {
    let mut p = fixture();
    p.layers[0].effects[0].image_input = Some(EffectImageInput::Layer {
        layer: 2,
        stage: EffectImageStage::Effects,
    });
    p.layers[1].effects[0].image_input = Some(EffectImageInput::Layer {
        layer: 1,
        stage: EffectImageStage::Effects,
    });
    assert!(p.validate().unwrap_err().to_string().contains("cycle"));
    p.layers[1].effects[0].enabled = false;
    assert!(p.validate().is_ok());
    p.layers[1].effects[0].enabled = true;
    p.layers[1].effects[0].image_input = Some(EffectImageInput::Layer {
        layer: 1,
        stage: EffectImageStage::Source,
    });
    assert!(p.validate().is_ok());
    let default: EffectImageInput = serde_json::from_str(r#"{"kind":"layer","layer":2}"#).unwrap();
    assert_eq!(
        default,
        EffectImageInput::Layer {
            layer: 2,
            stage: EffectImageStage::Effects
        }
    );
}
#[test]
fn reference_changes_and_deleted_sources_are_atomic_and_undoable() {
    let p = fixture();
    let mut engine = Engine::new(p).unwrap();
    let input = Some(EffectImageInput::Layer {
        layer: 2,
        stage: EffectImageStage::Effects,
    });
    engine
        .apply(Command::Effect {
            object: 1,
            action: EffectAction::SetImageInput { effect: 1, input },
        })
        .unwrap();
    let revision = engine.revision();
    assert!(engine
        .apply(Command::Effect {
            object: 2,
            action: EffectAction::SetImageInput {
                effect: 1,
                input: Some(EffectImageInput::Layer {
                    layer: 1,
                    stage: EffectImageStage::Effects
                })
            }
        })
        .is_err());
    assert_eq!(engine.revision(), revision);
    assert_eq!(engine.project().layers[1].effects[0].image_input, None);
    engine.apply(Command::Delete { object: 2 }).unwrap();
    assert_eq!(
        engine.project().layers[0].effects[0].image_input,
        Some(EffectImageInput::Empty)
    );
    engine.undo().unwrap();
    assert_eq!(engine.project().layers[0].effects[0].image_input, input);
    assert_eq!(engine.project().layers.len(), 2);
    let mut invalid = engine.snapshot();
    invalid.layers[0].effects[0].image_input = Some(EffectImageInput::Layer {
        layer: 999,
        stage: EffectImageStage::Effects,
    });
    assert!(invalid.validate().is_err());
}
