use aem_effects::{builtin, ParamKind};

#[test]
fn audited_ranges_and_legacy_color_contracts_are_distinct() {
    let current = builtin::manifest();
    for (effect, param, min, max) in [
        ("posterize", "levels", 2., 255.),
        ("channel_mixer", "red_red", -200., 200.),
        ("threshold", "level", -30000., 30000.),
        ("mosaic", "horizontal", 1., 4000.),
        ("venetian_blinds", "width", 1., 32000.),
    ] {
        let p = current
            .effects
            .iter()
            .find(|e| e.id == effect)
            .unwrap()
            .params
            .iter()
            .find(|p| p.id == param)
            .unwrap();
        assert_eq!((p.min, p.max), (min, max));
    }
    assert!(current
        .effects
        .iter()
        .flat_map(|e| &e.params)
        .filter(|p| p.kind == ParamKind::Color)
        .all(|p| p.min == 0. && p.max == 1.));
    let old = builtin::packages()
        .unwrap()
        .into_iter()
        .find(|p| p.manifest.id == builtin::PLUGIN_ID && p.manifest.version == "1.1.0")
        .unwrap();
    assert!(
        old.manifest
            .effects
            .iter()
            .find(|e| e.id == "tint")
            .unwrap()
            .params[0]
            .max
            > 1.
    );
}

#[test]
fn package_rejects_fractional_defaults_and_misleading_enum_or_boolean_ranges() {
    for (kind, min, max, value, options) in [
        (ParamKind::Bool, 0., 1., 0.5, vec![]),
        (ParamKind::Bool, -1., 1., 0., vec![]),
        (ParamKind::Enum, 1., 3., 1., vec!["a", "b"]),
        (ParamKind::Enum, 1., 2., 1.5, vec!["a", "b"]),
    ] {
        let mut manifest = builtin::manifest();
        let p = &mut manifest.effects[0].params[0];
        p.kind = kind;
        p.min = min;
        p.max = max;
        p.default = [value, 0., 0., 0.];
        p.options = options.into_iter().map(str::to_owned).collect();
        assert!(manifest.validate().is_err());
    }
    assert!(!ParamKind::Enum.valid_value(&[1.5, 0., 0., 0.], 1., 2.));
    assert!(!ParamKind::Color.valid_value(&[0., 0., 0., 1.1], 0., 1.));
    assert!(!ParamKind::Float.valid_value(&[f32::NAN, 0., 0., 0.], 0., 1.));
}
