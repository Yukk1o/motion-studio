use aem_core::{
    Command, Curve, CurveShape, CurveSpace, Ease, Easing, Engine, Project, Property, Track,
};

fn quadratic(space: CurveSpace, control: [f64; 2], end: f64) -> Curve {
    Curve {
        space,
        shape: CurveShape::Quadratic {
            control,
            start: 0.0,
            end,
        },
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

#[test]
fn bezier_time_is_inverted_and_velocity_is_the_progress_derivative() {
    let q = quadratic(CurveSpace::Progress, [0.2, 0.8], 1.0);
    q.validate().unwrap();
    near(q.sample(0.35).progress, 0.65); // At Bezier parameter u = 0.5.
    for curve in [
        q,
        Curve {
            space: CurveSpace::Progress,
            shape: CurveShape::Cubic {
                control1: [0.8, -0.5],
                control2: [0.2, 1.5],
                start: 0.0,
                end: 1.0,
            },
        },
        Curve {
            space: CurveSpace::Progress,
            shape: CurveShape::Elastic {
                oscillations: 2.5,
                damping: 6.0,
            },
        },
    ] {
        curve.validate().unwrap();
        near(curve.sample(0.0).progress, 0.0);
        near(curve.sample(1.0).progress, 1.0);
        for t in [0.11, 0.31, 0.71, 0.91] {
            let derivative =
                (curve.sample(t + 1e-6).progress - curve.sample(t - 1e-6).progress) / 2e-6;
            near(curve.sample(t).velocity, derivative);
        }
    }
}

#[test]
fn velocity_curves_integrate_with_respect_to_time_and_finish_exactly() {
    let c = quadratic(CurveSpace::Velocity, [0.5, 3.0], 0.0);
    c.validate().unwrap();
    for t in [0.0, 0.1, 0.25, 0.5, 0.9, 1.0] {
        near(c.sample(t).progress, 3.0 * t * t - 2.0 * t * t * t);
        near(c.sample(t).velocity, 6.0 * t * (1.0 - t));
    }
    let c = quadratic(CurveSpace::Velocity, [0.2, 3.0], 0.0);
    // Independent numerical integration of y(u) dx(u), including nonlinear x.
    let integrate = |end: f64| {
        let mut total = 0.0;
        let (mut old_x, mut old_y) = (0.0, 0.0);
        for i in 1..=10000 {
            let u = end * i as f64 / 10000.0;
            let x = 0.4 * u + 0.6 * u * u;
            let y = 6.0 * u * (1.0 - u);
            total += (x - old_x) * (y + old_y) * 0.5;
            (old_x, old_y) = (x, y);
        }
        total
    };
    let u = 0.37;
    near(
        c.sample(0.4 * u + 0.6 * u * u).progress,
        integrate(u) / integrate(1.0),
    );
    let scaled = quadratic(CurveSpace::Velocity, [0.2, 6.0], 0.0);
    near(c.sample(0.43).progress, scaled.sample(0.43).progress);
}

#[test]
fn elastic_overshoot_is_retained_and_invalid_definitions_are_rejected_atomically() {
    let elastic = Curve {
        space: CurveSpace::Progress,
        shape: CurveShape::Elastic {
            oscillations: 3.0,
            damping: 5.0,
        },
    };
    assert!((1..100).any(|i| elastic.sample(i as f64 / 100.0).progress > 1.05));
    let mut t = Track::constant(0.0);
    t.upsert(0, 0.0, Ease::Linear).unwrap();
    t.upsert(60, 100.0, Ease::Linear).unwrap();
    t.set_curve(
        0,
        Easing {
            ease: Ease::Linear,
            curve: Some(elastic),
        },
    )
    .unwrap();
    assert!((1..60).any(|frame| t.sample(frame as f64) > 105.0));
    for curve in [
        quadratic(CurveSpace::Velocity, [0.5, 0.0], 0.0),
        quadratic(CurveSpace::Progress, [1.5, 1.0], 1.0),
    ] {
        let before = t.clone();
        assert!(t
            .set_curve(
                0,
                Easing {
                    ease: Ease::Linear,
                    curve: Some(curve)
                }
            )
            .is_err());
        assert_eq!(t, before);
    }
}

#[test]
fn pasting_between_vector_scalar_and_camera_preserves_target_values_times_and_undo() {
    let mut e = Engine::new(Project::demo()).unwrap();
    for (object, property) in [
        (2, Property::Position),
        (3, Property::Opacity),
        (0, Property::Fov),
    ] {
        e.apply(Command::Animate {
            object,
            property,
            frame: 12,
            enabled: true,
        })
        .unwrap();
        e.apply(Command::CopyKey {
            object,
            property,
            from: 12,
            to: 72,
        })
        .unwrap();
    }
    let easing = Easing {
        ease: Ease::Linear,
        curve: Some(quadratic(CurveSpace::Velocity, [0.2, 3.0], 0.0)),
    };
    e.apply(Command::Curve {
        object: 2,
        property: Property::Position,
        frame: 12,
        easing,
    })
    .unwrap();
    let before = e.snapshot();
    for (object, property) in [(3, Property::Opacity), (0, Property::Fov)] {
        e.apply(Command::Curve {
            object,
            property,
            frame: 12,
            easing,
        })
        .unwrap();
    }
    assert_eq!(
        e.project().layers[2].transform.opacity.keys[0].curve,
        easing.curve
    );
    assert_eq!(e.project().camera.fov.keys[0].curve, easing.curve);
    for (a, b) in e
        .project()
        .camera
        .fov
        .keys
        .iter()
        .zip(&before.camera.fov.keys)
    {
        assert_eq!((a.frame, a.value), (b.frame, b.value));
    }
    e.undo().unwrap();
    e.undo().unwrap();
    assert_eq!(e.project(), &before);
    e.redo().unwrap();
    e.redo().unwrap();
    let restored: Project =
        serde_json::from_str(&serde_json::to_string(e.project()).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, *e.project());
    let before = e.snapshot();
    assert!(e
        .apply(Command::Curve {
            object: 3,
            property: Property::Opacity,
            frame: 72,
            easing
        })
        .is_err());
    assert_eq!(e.project(), &before);
}

#[test]
fn editing_moving_and_copying_keys_keeps_custom_curves_and_presets_clear_them() {
    let c = quadratic(CurveSpace::Progress, [0.3, 0.8], 1.0);
    let mut t = Track::constant(0.0);
    t.upsert(0, 1.0, Ease::Linear).unwrap();
    t.upsert(60, 10.0, Ease::Linear).unwrap();
    t.set_curve(
        0,
        Easing {
            ease: Ease::Linear,
            curve: Some(c),
        },
    )
    .unwrap();
    t.set_at(0, 2.0).unwrap();
    assert_eq!(t.keys[0].curve, Some(c));
    t.copy_key(0, 20).unwrap();
    assert_eq!(t.keys[1].curve, Some(c));
    t.move_key(20, 30).unwrap();
    assert_eq!(t.keys[1].curve, Some(c));
    t.set_ease(30, Ease::InOut).unwrap();
    assert_eq!(t.keys[1].curve, None);
    let legacy: Track<f32> =
        serde_json::from_str(r#"{"value":0,"keys":[{"frame":0,"value":1,"ease":"in"}]}"#).unwrap();
    assert!(legacy.keys[0].curve.is_none());
}
