use aem_core::{
    Command, EffectInstance, Engine, ExpressionTarget, Layer, Project, Property,
    PropertyExpression, Scene, EXPRESSION_PROFILE,
};
use aem_render::{effect_plan::PlanBuilder, Renderer};
#[test]
fn expressions_drive_geometry_effects_and_the_shared_export_plan() {
    let mut p = Project::new(64, 64, 30, 90).unwrap();
    p.background = [0.0; 4];
    p.layers.push(Layer::solid(
        1,
        "Card",
        [16.0; 2],
        [16.0, 32.0, 0.0],
        [1.0, 0.0, 0.0, 1.0],
    ));
    let pkg = aem_effects::builtin::package().unwrap();
    let tint = pkg
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "tint")
        .unwrap();
    p.layers[0].effects.push(EffectInstance::new(
        1,
        &pkg.manifest.id,
        &pkg.manifest.version,
        &pkg.hash,
        tint,
        [16.0; 2],
    ));
    p.rebuild_plugin_dependencies();
    let mut engine = Engine::new(p).unwrap();
    for (target, source) in [
        (
            ExpressionTarget::Property {
                object: 1,
                property: Property::Position,
                axis: None,
            },
            "value+[time*16,0,0]",
        ),
        (
            ExpressionTarget::Effect {
                object: 1,
                effect: 1,
                param: "p0003".into(),
            },
            "linear(time,0,1,0,100)",
        ),
    ] {
        engine
            .apply(Command::SetExpression {
                expression: PropertyExpression {
                    target,
                    source: source.into(),
                    enabled: true,
                    seed: 0,
                    profile: EXPRESSION_PROFILE.into(),
                },
                frame: 0,
            })
            .unwrap();
    }
    let mut scene = Scene::new(engine.project());
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let target = renderer.capture_target(64, 64).unwrap();
    scene.sample(engine.project(), 0.0, None).unwrap();
    let (start, _) = renderer.capture(&scene, &target).unwrap();
    scene.sample(engine.project(), 30.0, None).unwrap();
    let (end, _) = renderer.capture(&scene, &target).unwrap();
    let px = |v: &[u8], x: usize| v[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 4].to_vec();
    assert!(px(&start, 16)[0] > 250 && px(&start, 16)[1] < 3);
    assert_eq!(px(&end, 16), [0, 0, 0, 0]);
    let gray = px(&end, 32);
    assert!(gray[0] > 50 && gray[0].abs_diff(gray[1]) < 2 && gray[1].abs_diff(gray[2]) < 2);
    let mut builder =
        PlanBuilder::new(aem_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let plan = builder.build(&scene, &[0], 64, 64, true).unwrap();
    let mut bytes = vec![0; plan.buffer_bytes(&scene)];
    assert_eq!(plan.write(&scene, &mut bytes).unwrap(), bytes.len());
    let tint_amount = scene.effects[0]
        .param_ids
        .iter()
        .position(|id| id == "p0003")
        .unwrap();
    assert_eq!(scene.effects[0].values[tint_amount][0], 100.0);
    engine
        .apply(Command::Effect {
            object: 1,
            action: aem_core::EffectAction::Duplicate { effect: 1 },
        })
        .unwrap();
    assert_eq!(engine.project().expressions.len(), 3);
    engine
        .apply(Command::Effect {
            object: 1,
            action: aem_core::EffectAction::Remove { effect: 2 },
        })
        .unwrap();
    assert_eq!(engine.project().expressions.len(), 2);
    let frozen = engine.snapshot();
    engine
        .apply(Command::RemoveExpression {
            target: ExpressionTarget::Property {
                object: 1,
                property: Property::Position,
                axis: None,
            },
        })
        .unwrap();
    assert_eq!(
        frozen.evaluated_at(30.0).unwrap().layers[0]
            .transform
            .position
            .value,
        [32.0, 32.0, 0.0]
    );
}
