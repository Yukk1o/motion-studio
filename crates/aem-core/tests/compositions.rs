use aem_core::*;
use serde_json::{json, Value};

fn project() -> Project {
    let mut p = Project::new(64, 64, 30, 120).unwrap();
    p.layers = (1..=3)
        .map(|id| Layer::solid(id, "Layer", [24., 24.], [32., 32., 0.], [1., 0.2, 0.1, 0.5]))
        .collect();
    p
}
#[test]
fn save_package_and_active_view_restore_the_complete_graph_and_atomic_gestures() {
    let mut e = Engine::new(project()).unwrap();
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[1,2],"name":"child"}),
    );
    let id = e.project().compositions[0].id.clone();
    e.activate_composition(&id).unwrap();
    let root = tempfile::tempdir().unwrap();
    aem_core::storage::save(root.path(), e.project()).unwrap();
    let expected = e.project().composition(MAIN_COMPOSITION).unwrap();
    assert_eq!(aem_core::storage::load(root.path()).unwrap(), expected);
    let package = root.path().join("project.aem");
    aem_core::storage::export_package(root.path(), e.project(), &package).unwrap();
    assert_eq!(
        aem_core::storage::import_package(&package, &root.path().join("import")).unwrap(),
        expected
    );
    e.begin_gesture().unwrap();
    let before = e.snapshot();
    assert!(e.activate_composition(MAIN_COMPOSITION).is_err());
    assert!(e
        .apply(Command::Composition {
            action: CompositionAction::Precompose {
                objects: vec![1, 2],
                name: "bad".into(),
                range: "composition".into()
            }
        })
        .is_err());
    assert_eq!(e.snapshot(), before);
    e.end_gesture(false).unwrap();
}
fn edit(engine: &mut Engine, composition: &str, action: Value) -> Vec<EditResult> {
    engine
        .apply_batch(
            parse_commands(
                &json!({"op":"composition","composition":composition,"action":action}).to_string(),
            )
            .unwrap(),
        )
        .unwrap()
}
fn child(engine: &mut Engine, name: &str, fps: u32) -> String {
    let result = edit(
        engine,
        MAIN_COMPOSITION,
        json!({"kind":"create","settings":{"name":name,"width":64,"height":64,"fps":fps,"frames":120}}),
    );
    let EditResult::Composition { result } = &result[0] else {
        panic!()
    };
    result["composition"].as_str().unwrap().into()
}
#[test]
fn old_projects_migrate_without_losing_contents_and_main_identity_is_stable() {
    let mut p = project();
    p.version = 5;
    let mut old = serde_json::to_value(&p).unwrap();
    old.as_object_mut().unwrap().remove("composition_id");
    old.as_object_mut().unwrap().remove("compositions");
    let p: Project = serde_json::from_value(old).unwrap();
    let migrated = p.clone().migrate().unwrap();
    assert_eq!(migrated.version, 7);
    assert_eq!(migrated.layers, p.layers);
    assert_eq!(migrated.composition_id, MAIN_COMPOSITION);
}
#[test]
fn precompose_preserves_tracks_order_parents_clips_and_atomic_history() {
    let mut p = project();
    p.layers[1].timeline = Some(LayerTimeline {
        in_frame: 20,
        out_frame: 100,
        offset_frame: 17,
    });
    p.layers[1]
        .transform
        .position
        .upsert(12, [20., 28., 0.], Ease::InOut)
        .unwrap();
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[2,3],"name":"Nested"}),
    );
    let after = e.snapshot();
    let reference = &after.layers[1];
    let Content::Composition { clip } = &reference.content else {
        panic!()
    };
    let nested = after.composition(&clip.composition).unwrap();
    assert_eq!(nested.layers, before.layers[1..]);
    assert_eq!(nested.background, [0.; 4]);
    assert_eq!(nested.fps, before.fps);
    assert_eq!(after.layers[0], before.layers[0]);
    assert_eq!(after.layers.len(), 2);
    assert!(e.undo().unwrap());
    assert_eq!(e.snapshot(), before);
    assert!(e.redo().unwrap());
    assert_eq!(e.snapshot(), after);
}
#[test]
fn invalid_selections_cycles_and_referenced_deletions_do_not_publish_history() {
    let mut e = Engine::new(project()).unwrap();
    for objects in [json!([1, 3]), json!([1, 1]), json!([1, 99]), json!([0])] {
        let before = e.snapshot();
        let revision = e.revision();
        assert!(e.apply_batch(parse_commands(&json!({"op":"composition","action":{"kind":"precompose","objects":objects,"name":"Bad"}}).to_string()).unwrap()).is_err());
        assert_eq!(e.snapshot(), before);
        assert_eq!(e.revision(), revision);
        assert!(!e.can_undo());
    }
    let id = child(&mut e, "Child", 30);
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"reference","target":id}),
    );
    for (scope, action) in [
        (
            id.clone(),
            json!({"kind":"reference","target":MAIN_COMPOSITION}),
        ),
        (
            MAIN_COMPOSITION.into(),
            json!({"kind":"delete","target":id}),
        ),
    ] {
        let before = e.snapshot();
        let revision = e.revision();
        assert!(e
            .apply_batch(
                parse_commands(
                    &json!({"op":"composition","composition":scope,"action":action}).to_string()
                )
                .unwrap()
            )
            .is_err());
        assert_eq!(before, e.snapshot());
        assert_eq!(revision, e.revision());
    }
    let mut self_reference = project();
    self_reference.layers[0].content = Content::Composition {
        clip: CompositionClip::new(MAIN_COMPOSITION.into()),
    };
    assert!(self_reference.validate().is_err());
}
#[test]
fn scoped_commands_do_not_edit_identically_numbered_layers_in_another_composition() {
    let mut e = Engine::new(project()).unwrap();
    let id = child(&mut e, "Child", 30);
    let l = Layer::solid(
        1,
        "Child layer",
        [16., 16.],
        [32., 32., 0.],
        [0., 1., 0., 1.],
    );
    e.apply_batch(
        parse_commands(&json!({"op":"add","composition":id,"layer":l}).to_string()).unwrap(),
    )
    .unwrap();
    e.apply_batch(
        parse_commands(
            &json!({"op":"rename","composition":id,"object":1,"name":"Edited"}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(e.project().layers[0].name, "Layer");
    assert_eq!(
        e.project().composition(&id).unwrap().layers[0].name,
        "Edited"
    );
    assert_eq!(e.project().composition_id, MAIN_COMPOSITION);
}
#[test]
fn differing_frame_rates_offsets_and_duplicate_instances_sample_independently() {
    let mut e = Engine::new(project()).unwrap();
    let id = child(&mut e, "60 fps", 60);
    e.apply_batch(parse_commands(&json!({"op":"add","composition":id,"layer":Layer::solid(1,"Child",[8.,8.],[32.,32.,0.],[1.;4])}).to_string()).unwrap()).unwrap();
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"reference","target":id,"at_frame":10}),
    );
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"reference","target":id,"at_frame":20}),
    );
    let mut scene = Scene::new(e.project());
    scene.sample(e.project(), 25., None).unwrap();
    assert_eq!(scene.nested[0].scene.frame, 30.);
    assert_eq!(scene.nested[1].scene.frame, 10.);
    assert_ne!(
        scene.nested[0].scene.layers[0].id,
        scene.nested[1].scene.layers[0].id
    );
    scene.sample(e.project(), 100., None).unwrap();
    assert!(scene.nested.is_empty());
}
#[test]
fn nested_expression_views_preserve_time_and_dimensions_without_copying_the_graph() {
    let mut p = project();
    p.expressions.push(PropertyExpression {
        target: ExpressionTarget::Property {
            object: 1,
            property: Property::Position,
            axis: None,
        },
        source: "value + [time * 10 + thisComp.width / 64, 0, 0]".into(),
        enabled: true,
        seed: 7,
        profile: EXPRESSION_PROFILE.into(),
    });
    let before = p.clone();
    let mut e = Engine::new(p).unwrap();
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"precompose","objects":[1,2],"name":"expression child"}),
    );
    for frame in [0., 40.5, 80., 4.] {
        let mut original = Scene::new(&before);
        original.sample(&before, frame, None).unwrap();
        let mut nested = Scene::new(e.project());
        nested.sample(e.project(), frame, None).unwrap();
        assert_eq!(
            original.layers[0].model,
            nested.nested[0].scene.layers[0].model
        );
        assert!(nested.nested[0]
            .scene
            .sampled_project(&before)
            .compositions
            .is_empty());
    }
}
#[test]
fn settings_preview_candidate_keeps_content_and_retimes_keys_or_rejects_truncation() {
    let mut p = project();
    p.layers[0]
        .transform
        .rotation
        .upsert(15, [0., 0., 90.], Ease::Linear)
        .unwrap();
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    let settings = json!({"kind":"settings","settings":{"name":"60fps","width":64,"height":64,"fps":60,"frames":240,"timing":"preserve_seconds"}});
    edit(&mut e, MAIN_COMPOSITION, settings);
    assert_eq!(e.project().layers[0].transform.rotation.keys[0].frame, 30);
    assert_eq!(e.project().layers.len(), 3);
    assert!(e.undo().unwrap());
    assert_eq!(e.snapshot(), before);
    let settings = json!({"kind":"settings","settings":{"name":"short","width":64,"height":64,"fps":30,"frames":10,"shorten":"reject"}});
    assert!(e
        .apply_batch(
            parse_commands(&json!({"op":"composition","action":settings}).to_string()).unwrap()
        )
        .is_err());
    assert_eq!(before, e.snapshot());
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"settings","settings":{"name":"short","width":64,"height":64,"fps":30,"frames":10,"shorten":"trim"}}),
    );
    assert!(e
        .project()
        .layers
        .iter()
        .all(|l| l.clip(10).out_frame == 10));
    assert_eq!(e.project().layers.len(), 3);
}
#[test]
fn parent_links_and_locked_layers_are_rejected_without_removing_associations() {
    let mut p = project();
    p.layers[1].parent = Some(ParentLink {
        object: Some(1),
        bind: glam::Mat4::IDENTITY.to_cols_array_2d(),
    });
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    let command =
        json!({"op":"composition","action":{"kind":"precompose","objects":[2,3],"name":"bad"}});
    assert!(e
        .apply_batch(parse_commands(&command.to_string()).unwrap())
        .is_err());
    assert_eq!(before, e.snapshot());
    let mut p = project();
    p.layers[1].locked = true;
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    assert!(e
        .apply_batch(parse_commands(&command.to_string()).unwrap())
        .is_err());
    assert_eq!(before, e.snapshot());
}
