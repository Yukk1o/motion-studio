use motion_effects::{builtin, BoundsExpr, OutputBounds};

#[test]
fn geometry_contract_rejects_old_sdk_ambiguous_or_invalid_rectangles() {
    // Exercise the historical SDK geometry boundary without the new SDK 6
    // package-count/image-input requirements masking the intended diagnostic.
    let original = builtin::legacy_current_package().unwrap().manifest.clone();
    let tile = original
        .effects
        .iter()
        .find(|e| e.id == "motion_tile")
        .unwrap();
    let parameters = tile
        .params
        .iter()
        .map(|p| (p.id.clone(), p.default))
        .collect();
    let rect = tile.output_bounds.as_ref().unwrap();
    assert_eq!(
        rect.evaluate_with(
            |id, c| std::collections::BTreeMap::<String, [f32; 4]>::get(&parameters, id)?
                .get(c)
                .copied(),
            [-10., -20., 300., 80.]
        )
        .unwrap(),
        [-10., -20., 300., 80.]
    );
    for sdk in [1, 2] {
        let mut manifest = original.clone();
        manifest.sdk_version = sdk;
        assert!(manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("SDK 3"));
    }
    let mut manifest = original.clone();
    let tile = manifest
        .effects
        .iter_mut()
        .find(|e| e.id == "motion_tile")
        .unwrap();
    tile.padding = BoundsExpr::Constant { value: 1. };
    assert!(manifest.validate().is_err());
    let invalid = OutputBounds {
        x: BoundsExpr::InputOrigin { component: 2 },
        ..rect.clone()
    };
    assert!(invalid
        .evaluate_with(|_, _| Some(100.), [0., 0., 64., 64.])
        .is_err());
    let invalid = OutputBounds {
        width: BoundsExpr::Constant { value: 0. },
        ..rect.clone()
    };
    assert!(invalid
        .evaluate_with(|_, _| Some(100.), [0., 0., 64., 64.])
        .is_err());
    let invalid = OutputBounds {
        height: BoundsExpr::Constant { value: f32::NAN },
        ..rect.clone()
    };
    assert!(invalid
        .evaluate_with(|_, _| Some(100.), [0., 0., 64., 64.])
        .is_err());
}

#[test]
fn new_controls_match_collected_native_ranges_instead_of_slider_guesses() {
    let manifest = builtin::manifest();
    let range = |effect: &str, param: &str| {
        let e = manifest.effects.iter().find(|e| e.id == effect).unwrap();
        let p = e.params.iter().find(|p| p.id == param).unwrap();
        (p.min, p.max, p.default[0])
    };
    assert_eq!(range("motion_tile", "tile_width"), (0., 100., 100.));
    assert_eq!(range("motion_tile", "output_width"), (0., 30000., 100.));
    assert_eq!(range("simple_choker", "choke"), (-100., 100., 0.));
    assert_eq!(range("cc_lens", "convergence"), (-200., 100., 100.));
    assert_eq!(range("spherize", "radius"), (0., 2500., 0.));
}

#[test]
fn spatial_bounds_math_is_finite_lazy_and_requires_sdk_four() {
    let eval = |json: &str| {
        serde_json::from_str::<BoundsExpr>(json)
            .unwrap()
            .evaluate_with(|_, _| None, [-5., 7., 300., 400.])
    };
    assert_eq!(eval(r#"{"op":"hypot","a":{"op":"input_size","component":0},"b":{"op":"input_size","component":1}}"#).unwrap(), 500.);
    assert_eq!(
        eval(r#"{"op":"divide","a":{"op":"constant","value":12},"b":{"op":"constant","value":3}}"#)
            .unwrap(),
        4.
    );
    assert_eq!(
        eval(r#"{"op":"sqrt","value":{"op":"constant","value":25}}"#).unwrap(),
        5.
    );
    assert_eq!(eval(r#"{"op":"min","a":{"op":"input_origin","component":0},"b":{"op":"constant","value":0}}"#).unwrap(), -5.);
    assert!(
        (eval(r#"{"op":"sin","value":{"op":"constant","value":1.5707963}}"#).unwrap() - 1.).abs()
            < 1e-6
    );
    assert_eq!(eval(r#"{"op":"select","condition":{"op":"constant","value":0},"a":{"op":"divide","a":{"op":"constant","value":1},"b":{"op":"constant","value":0}},"b":{"op":"constant","value":17}}"#).unwrap(), 17.);
    for invalid in [
        r#"{"op":"divide","a":{"op":"constant","value":1},"b":{"op":"constant","value":0}}"#,
        r#"{"op":"sqrt","value":{"op":"constant","value":-1}}"#,
        r#"{"op":"hypot","a":{"op":"constant","value":32768},"b":{"op":"constant","value":32768}}"#,
    ] {
        assert!(
            eval(invalid).is_err(),
            "accepted invalid expression {invalid}"
        );
    }
    // Reject new arithmetic even when nested inside an older SDK's padding tree.
    let mut manifest = builtin::manifest();
    manifest.effects.retain(|e| e.id == "tint");
    manifest.sdk_version = 3;
    manifest.effects[0].padding = serde_json::from_str(r#"{"op":"abs","value":{"op":"min","a":{"op":"constant","value":0},"b":{"op":"constant","value":1}}}"#).unwrap();
    assert!(manifest
        .validate()
        .unwrap_err()
        .to_string()
        .contains("SDK 4"));
    manifest.sdk_version = 4;
    manifest.validate().unwrap();
}
