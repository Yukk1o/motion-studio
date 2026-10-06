use aem_core::{
    Axis, Command, Ease, Engine, ExpressionTarget, Layer, LayerTimeline, Project, Property,
    PropertyExpression, Scene, EXPRESSION_PROFILE,
};
use std::sync::{atomic::AtomicBool, Arc};
fn fixture() -> Project {
    let mut p = Project::new(100, 100, 30, 180).unwrap();
    p.layers.push(Layer::solid(
        1,
        "Card",
        [20.0; 2],
        [10.0, 20.0, 0.0],
        [1.0; 4],
    ));
    p
}
fn target(property: Property, axis: Option<Axis>) -> ExpressionTarget {
    ExpressionTarget::Property {
        object: 1,
        property,
        axis,
    }
}
fn expression(source: &str, t: ExpressionTarget) -> PropertyExpression {
    PropertyExpression {
        target: t,
        source: source.into(),
        enabled: true,
        seed: 42,
        profile: EXPRESSION_PROFILE.into(),
    }
}
fn set(p: Project, source: &str, t: ExpressionTarget) -> Project {
    let mut e = Engine::new(p).unwrap();
    e.apply(Command::SetExpression {
        expression: expression(source, t),
        frame: 0,
    })
    .unwrap();
    e.snapshot()
}
fn x(p: &Project, frame: f64) -> f32 {
    p.evaluated_at(frame).unwrap().layers[0]
        .transform
        .position
        .sample(frame)[0]
}
#[test]
fn ae_vector_math_multiline_completion_and_real_js_functions() {
    let p = set(
        fixture(),
        "function delta(t) { return [t*30,10,0]; } var p=value+delta(time); p*=2; p;",
        target(Property::Position, None),
    );
    assert_eq!(p.version, 3);
    assert_eq!(p.layers[0].transform.position.value, [10.0, 20.0, 0.0]);
    assert_eq!(
        p.evaluated_at(30.0).unwrap().layers[0]
            .transform
            .position
            .value,
        [80.0, 60.0, 0.0]
    );
    let p = set(
        fixture(),
        "if (time<1) { value; } else { value+[10,0]; }",
        target(Property::Position, None),
    );
    assert_eq!(x(&p, 0.0), 10.0);
    assert_eq!(x(&p, 30.0), 20.0);
    let p = set(
        fixture(),
        "add([10,20],[1,2,3])",
        target(Property::Position, None),
    );
    assert_eq!(
        p.evaluated_at(0.0).unwrap().layers[0]
            .transform
            .position
            .value,
        [11.0, 22.0, 3.0]
    );
}
#[test]
fn composition_seconds_clip_keys_velocity_and_loops() {
    let mut p = fixture();
    p.layers[0].timeline = Some(LayerTimeline {
        in_frame: 0,
        out_frame: 180,
        offset_frame: -30,
    });
    p.layers[0]
        .transform
        .position
        .upsert(30, [0.0, 0.0, 0.0], Ease::Linear)
        .unwrap();
    p.layers[0]
        .transform
        .position
        .upsert(60, [30.0, 0.0, 0.0], Ease::Linear)
        .unwrap();
    let p = set(p, "loopOut('cycle')", target(Property::Position, None));
    assert_eq!(x(&p, 45.0), 15.0);
    assert_eq!(x(&p, 60.0), 0.0);
    let p = set(p, "loopOut('pingpong')", target(Property::Position, None));
    assert_eq!(x(&p, 45.0), 15.0);
    assert_eq!(x(&p, 60.0), 0.0);
    assert_eq!(x(&p, 75.0), 15.0);
    let p = set(p, "loopOut('offset')", target(Property::Position, None));
    assert_eq!(x(&p, 45.0), 45.0);
    assert_eq!(x(&p, 60.0), 60.0);
    let p = set(p, "loopOut('continue')", target(Property::Position, None));
    assert!((x(&p, 45.0) - 45.0).abs() < 0.01);
    let p = set(
        p,
        "[key(1).time, nearestKey(0.9).index, valueAtTime(.5)[0]]",
        target(Property::Position, None),
    );
    assert_eq!(
        p.evaluated_at(0.0).unwrap().layers[0]
            .transform
            .position
            .value,
        [0.0, 2.0, 15.0]
    );
    let p = set(p, "velocityAtTime(.5)", target(Property::Position, None));
    assert!((x(&p, 0.0) - 30.0).abs() < 0.01);
}
#[test]
fn opacity_percent_axis_accumulation_camera_and_scene_use_computed_values() {
    let p = set(fixture(), "value/2", target(Property::Opacity, None));
    assert_eq!(
        p.evaluated_at(30.0).unwrap().layers[0]
            .transform
            .opacity
            .value,
        0.5
    );
    let p = set(
        p,
        "value+time*30",
        target(Property::Position, Some(Axis::X)),
    );
    let p = set(p, "value+5", target(Property::Position, Some(Axis::Y)));
    let mut s = Scene::new(&p);
    s.sample(&p, 30.0, None).unwrap();
    assert_eq!(
        s.sampled_project(&p).layers[0].transform.position.value,
        [40.0, 25.0, 0.0]
    );
    assert_eq!(s.layers[0].opacity, 0.5);
    let mut p = fixture();
    p.camera.created = true;
    let p = set(
        p,
        "value+10*Math.sin(time)",
        ExpressionTarget::Property {
            object: 0,
            property: Property::Roll,
            axis: None,
        },
    );
    assert!((p.evaluated_at(30.0).unwrap().camera.roll.value - 10.0 * 1f32.sin()).abs() < 1e-5);
}
#[test]
fn random_wiggle_and_global_mutations_are_seek_order_independent() {
    let p = set(
        fixture(),
        "seedRandom(5,true); value+[random(-10,10),random(-10,10),0]",
        target(Property::Position, None),
    );
    assert_eq!(
        p.evaluated_at(0.0).unwrap().layers[0]
            .transform
            .position
            .value,
        p.evaluated_at(90.0).unwrap().layers[0]
            .transform
            .position
            .value
    );
    let p = set(p, "wiggle(2,10)", target(Property::Position, None));
    let a = x(&p, 17.0);
    for frame in [40.0, 0.0, 17.0, 100.0] {
        let _ = x(&p, frame);
    }
    assert_eq!(x(&p, 17.0), a);
    let p = set(
        p,
        "globalThis.counter=(globalThis.counter||0)+1; value+[globalThis.counter,0,0]",
        target(Property::Position, None),
    );
    assert_eq!(x(&p, 5.0), 11.0);
    assert_eq!(x(&p, 6.0), 11.0);
}
#[test]
fn syntax_type_budget_cancel_and_frame_errors_preserve_original_animation() {
    let mut e = Engine::new(fixture()).unwrap();
    let before = e.snapshot();
    for source in [
        "var =",
        "'hello'",
        "[1,2]",
        "NaN",
        "while(true){}",
        "new Array(100000000).fill(1)",
    ] {
        assert!(
            e.apply(Command::SetExpression {
                expression: expression(source, target(Property::Position, None)),
                frame: 0
            })
            .is_err(),
            "{source}"
        );
        assert_eq!(e.snapshot(), before);
        assert!(!e.can_undo());
    }
    e.apply(Command::SetExpression {
        expression: expression(
            "time<1 ? value : missing()",
            target(Property::Position, None),
        ),
        frame: 0,
    })
    .unwrap();
    assert!(e
        .project()
        .evaluated_at(30.0)
        .unwrap_err()
        .to_string()
        .contains("frame 30"));
    assert!(e
        .project()
        .evaluated_at_cancellable(0.0, Arc::new(AtomicBool::new(true)))
        .is_err());
    let mut disabled = e.project().expressions[0].clone();
    disabled.enabled = false;
    e.apply(Command::SetExpression {
        expression: disabled,
        frame: 30,
    })
    .unwrap();
    assert_eq!(x(e.project(), 30.0), 10.0);
}
#[test]
fn persistence_undo_duplicate_split_delete_and_disabled_drafts() {
    let p = set(
        fixture(),
        "value+[time,0,0]",
        target(Property::Position, None),
    );
    let encoded = serde_json::to_string(&p).unwrap();
    let loaded: Project = serde_json::from_str(&encoded).unwrap();
    loaded.validate().unwrap();
    assert_eq!(x(&loaded, 30.0), 11.0);
    let mut e = Engine::new(loaded).unwrap();
    e.apply(Command::Duplicate { object: 1 }).unwrap();
    assert_eq!(e.project().expressions.len(), 2);
    e.undo().unwrap();
    assert_eq!(e.project().expressions.len(), 1);
    e.redo().unwrap();
    e.apply(Command::SplitLayerClip {
        object: 2,
        frame: 60,
    })
    .unwrap();
    assert_eq!(e.project().expressions.len(), 3);
    e.apply(Command::Delete { object: 3 }).unwrap();
    assert_eq!(e.project().expressions.len(), 2);
    e.apply(Command::RemoveExpression {
        target: target(Property::Position, None),
    })
    .unwrap();
    assert_eq!(e.project().expressions.len(), 1);
    let mut draft = expression("var =", target(Property::Position, None));
    draft.enabled = false;
    e.apply(Command::SetExpression {
        expression: draft,
        frame: 0,
    })
    .unwrap();
    assert!(e.project().evaluated_at(30.0).is_ok());
}
#[test]
fn key_range_loop_in_subset_and_vector_assignment_side_effects() {
    let mut p = fixture();
    for (f, v) in [(30, 10.0), (60, 20.0), (90, 40.0)] {
        p.layers[0]
            .transform
            .position
            .upsert(f, [v, 20.0, 0.0], Ease::Linear)
            .unwrap();
    }
    let p = set(p, "loopIn('offset',1)", target(Property::Position, None));
    assert_eq!(x(&p, 0.0), 0.0);
    let p = set(p, "loopOut('cycle',1)", target(Property::Position, None));
    assert_eq!(x(&p, 105.0), 30.0);
    let p = set(
        p,
        "var n=0; var a=[1,2,3]; a[n++]+=2; value+[n,a[0],0]",
        target(Property::Position, None),
    );
    assert_eq!(
        p.evaluated_at(0.0).unwrap().layers[0]
            .transform
            .position
            .value,
        [11.0, 23.0, 0.0]
    );
}

#[test]
fn cache_rollover_and_async_rejection_do_not_contaminate_next_frame() {
    let mut e = Engine::new(fixture()).unwrap();
    for i in 0..260 {
        e.apply(Command::SetExpression {
            expression: expression(
                &format!("value+[{},0,0]", i),
                target(Property::Position, None),
            ),
            frame: 0,
        })
        .unwrap();
    }
    assert_eq!(x(e.project(), 0.0), 269.0);
    for source in ["async function f(){return 1;} value;", "(async()=>1)();"] {
        assert!(e
            .apply(Command::SetExpression {
                expression: expression(source, target(Property::Position, None)),
                frame: 0
            })
            .is_err());
    }
    assert_eq!(x(e.project(), 10.0), 269.0);
    let p = set(fixture(), "wiggle(1,10)", target(Property::Position, None));
    assert!(
        (x(&p, 20.0) - x(&p, 20.0001)).abs() < 0.001,
        "wiggle must remain continuous across subframes"
    );
}

#[test]
fn old_formats_overlap_locks_and_nonfinite_results_are_rejected_atomically() {
    let mut old = fixture();
    old.version = 1;
    let e = Engine::new(old).unwrap();
    assert_eq!(e.project().version, 2);
    assert!(e.project().expressions.is_empty());
    let p = set(fixture(), "value+[1,0,0]", target(Property::Position, None));
    let mut e = Engine::new(p).unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::SetExpression {
            expression: expression("value+1", target(Property::Position, Some(Axis::X))),
            frame: 0
        })
        .is_err());
    assert_eq!(e.snapshot(), before);
    e.apply(Command::Flags {
        object: 1,
        visible: true,
        locked: true,
    })
    .unwrap();
    let before = e.snapshot();
    assert!(e
        .apply(Command::RemoveExpression {
            target: target(Property::Position, None)
        })
        .is_err());
    assert_eq!(e.snapshot(), before);
    let p = set(
        fixture(),
        "time<1?value:[Infinity,0,0]",
        target(Property::Position, None),
    );
    assert!(p
        .evaluated_at(30.0)
        .unwrap_err()
        .to_string()
        .contains("finite"));
}
