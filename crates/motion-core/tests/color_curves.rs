use motion_core::color_curves::ColorInterpolation;
use motion_core::{Command, CurveObject, EffectAction, EffectInstance, Engine, Layer, Project};

#[test]
fn smooth_curve_has_known_cubic_shape_and_legacy_missing_mode_stays_linear() {
    let mut curve = CurveObject::default();
    curve.channels[0] = vec![[0., 0.], [0.5, 1.], [1., 0.]];
    let lut = curve.lut();
    let x = 64.0f32 / 255.;
    let expected = 3. * x - 4. * x * x * x;
    assert!((lut[64][0] - expected).abs() < 1e-6);
    let mut old = serde_json::to_value(&curve).unwrap();
    old.as_object_mut().unwrap().remove("interpolation");
    let old: CurveObject = serde_json::from_value(old).unwrap();
    assert_eq!(old.interpolation, [ColorInterpolation::Linear; 5]);
    assert_eq!(old.lut()[64][0], x * 2.);
    let graph = curve.graph();
    let segments = graph["channels"][0]["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    assert!(segments[0][1][1].as_f64().unwrap() > 0.4);
}

#[test]
fn five_channels_are_independent_and_alpha_does_not_change_rgb() {
    let mut curve = CurveObject::default();
    curve.channels[1] = vec![[0., 0.], [0.4, 0.2], [1., 1.]];
    curve.channels[2] = vec![[0., 0.], [0.4, 0.8], [1., 1.]];
    let rgb = curve.lut();
    curve.channels[4] = vec![[0., 0.], [1., 0.5]];
    let alpha = curve.lut();
    for i in 0..256 {
        assert_eq!(rgb[i][..3], alpha[i][..3]);
        assert!((alpha[i][3] - i as f32 / 510.).abs() < 1e-6);
    }
    assert!(alpha[102][0] < alpha[102][1]);
    assert_eq!(curve.graph()["channels"].as_array().unwrap().len(), 5);
    assert_eq!(curve.graph()["channels"][4]["name"], "Alpha");
}

#[test]
fn frozen_current_frame_is_exact_and_editing_one_channel_preserves_the_rest() {
    let mut a = CurveObject::default();
    a.channels[0] = vec![[0., 0.], [0.3, 0.7], [1., 1.]];
    a.channels[4] = vec![[0., 0.], [0.6, 0.2], [1., 0.9]];
    let mut frozen = a.clone();
    frozen.sampled_lut = Some(a.lut().to_vec());
    let mut editable = frozen.editable();
    assert_eq!(editable.lut(), frozen.lut());
    let before = editable.lut();
    editable.channel_luts[1] = None;
    editable.channels[1] = vec![[0., 1.], [1., 0.]];
    editable.interpolation[1] = ColorInterpolation::NaturalCubic;
    let after = editable.lut();
    assert_ne!(before[80][0], after[80][0]);
    for i in 0..256 {
        assert_eq!(before[i][1..], after[i][1..]);
    }
    let roundtrip: CurveObject =
        serde_json::from_str(&serde_json::to_string(&editable).unwrap()).unwrap();
    assert_eq!(roundtrip.lut(), after);
}

#[test]
fn spline_handles_tiny_x_intervals_and_lookup_rejects_nonfinite_or_wrong_lengths() {
    let mut curve = CurveObject::default();
    curve.channels[1] = vec![
        [0., 0.],
        [f32::from_bits(1), 1.],
        [0.99999994, 0.],
        [1., 1.],
    ];
    curve.validate().unwrap();
    assert!(curve
        .lut()
        .iter()
        .flatten()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v)));
    curve.channel_luts[2] = Some(vec![0.; 255]);
    assert!(curve.validate().is_err());
    curve.channel_luts[2] = Some(vec![0.; 256]);
    curve.channel_luts[2].as_mut().unwrap()[42] = f32::NAN;
    assert!(curve.validate().is_err());
}

#[test]
fn curve_edits_and_static_colors_are_atomic_and_undo_redo_preserve_resources() {
    let mut p = Project::new(64, 64, 30, 60).unwrap();
    p.layers
        .push(Layer::solid(1, "solid", [64.; 2], [32., 32., 0.], [1.; 4]));
    let package = motion_effects::builtin::package().unwrap();
    let def = package
        .manifest
        .effects
        .iter()
        .find(|d| d.id == "curves")
        .unwrap();
    p.layers[0].effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        def,
        [64.; 2],
    ));
    p.rebuild_plugin_dependencies();
    let mut e = Engine::new(p).unwrap();
    let before = e.project().clone();
    e.begin_gesture().unwrap();
    for a in [0.9, 0.7, 0.5] {
        e.apply(Command::SetColor {
            object: 1,
            value: [1., 0.8, 0.2, a],
        })
        .unwrap();
    }
    e.end_gesture(false).unwrap();
    assert_eq!(e.project(), &before);
    e.begin_gesture().unwrap();
    e.apply(Command::SetColor {
        object: 0,
        value: [0., 0., 0., 0.],
    })
    .unwrap();
    let mut curve = CurveObject::default();
    curve.channels[4] = vec![[0., 0.], [1., 0.5]];
    let param = def
        .params
        .iter()
        .find(|p| p.kind == motion_effects::ParamKind::Curve)
        .unwrap()
        .id
        .clone();
    e.apply(Command::Effect {
        object: 1,
        action: EffectAction::SetCurveObject {
            effect: 1,
            param,
            frame: 0,
            value: curve,
        },
    })
    .unwrap();
    e.end_gesture(true).unwrap();
    let edited = e.project().clone();
    assert!(e.undo().unwrap());
    assert_eq!(e.project(), &before);
    assert!(e.redo().unwrap());
    assert_eq!(e.project(), &edited);
    let invalid = Command::SetColor {
        object: 1,
        value: [1.1, 0., 0., 1.],
    };
    assert!(e.apply(invalid).is_err());
    assert_eq!(e.project(), &edited);
    assert!(!Command::SetColor {
        object: 1,
        value: [1.; 4]
    }
    .changes_resources());
}
