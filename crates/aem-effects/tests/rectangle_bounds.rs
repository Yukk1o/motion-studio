use aem_effects::{builtin, BoundsExpr, OutputBounds};

#[test]
fn geometry_contract_rejects_old_sdk_ambiguous_or_invalid_rectangles() {
    let original = builtin::manifest();
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
