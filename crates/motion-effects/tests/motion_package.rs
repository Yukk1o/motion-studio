use motion_effects::{builtin, EffectPackage, ParamKind, Registry};

#[test]
fn unified_builtin_is_installed_and_sources_match_published_bytes() {
    let p = builtin::package().unwrap();
    assert_eq!(p.manifest.id, builtin::PLUGIN_ID);
    assert_eq!(p.manifest.effects.len(), 95);
    let registry = Registry::new_with_builtins().unwrap();
    assert!(registry
        .resolve(&p.manifest.id, &p.manifest.version, &p.hash)
        .is_ok());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin-library");
    for d in &builtin::creative_effects() {
        assert!(d.params.iter().all(|p| p.implemented));
        assert!(!d
            .known_differences
            .iter()
            .any(|v| v.contains("not implemented")));
        for (i, pass) in d.passes.iter().enumerate() {
            assert_eq!(
                std::fs::read_to_string(root.join(&pass.shader))
                    .unwrap()
                    .replace("\r\n", "\n"),
                std::str::from_utf8(&p.files[&pass.shader])
                    .unwrap()
                    .replace("\r\n", "\n")
            );
            let shader = &p.shaders[&(d.id.clone(), i)];
            assert!(shader.glsl.fragment.contains("#version 300 es"));
            assert!(
                shader.loop_work <= 2048,
                "{} work {}",
                d.id,
                shader.loop_work
            );
            assert!(shader
                .wgsl
                .contains("Copyright (c) 2026 Shader Effects Inc."));
        }
    }
    let loaded = EffectPackage::from_bytes(p.bytes.clone()).unwrap();
    assert_eq!(loaded.hash, p.hash);
}

#[test]
fn gradient_and_noise_modes_have_valid_persistable_enum_contracts() {
    let p = builtin::package().unwrap();
    for (id, param, count) in [
        ("gradient", "shape", 6),
        ("noise_generator", "basis", 6),
        ("region_blur", "mode", 2),
    ] {
        let d = p.manifest.effects.iter().find(|d| d.id == id).unwrap();
        let param = d.params.iter().find(|p| p.id == param).unwrap();
        assert_eq!(param.kind, ParamKind::Enum);
        assert_eq!(param.options.len(), count);
        assert_eq!(param.max, count as f32 - 1.);
    }
    for id in ["ascii", "displacement_map"] {
        let d = p.manifest.effects.iter().find(|d| d.id == id).unwrap();
        assert!(d.required_capabilities.iter().any(|c| c == "image_input"));
        assert!(d.params.len() <= 30);
    }
}
#[test]
fn catalogue_aliases_target_existing_effects_with_legal_values() {
    let core = builtin::package().unwrap();
    let motion = builtin::package().unwrap();
    for alias in builtin::effect_aliases().as_array().unwrap() {
        let p = if alias["plugin"] == core.manifest.id {
            &core
        } else {
            &motion
        };
        let d = p
            .manifest
            .effects
            .iter()
            .find(|d| Some(d.id.as_str()) == alias["effect"].as_str())
            .unwrap();
        for (id, value) in alias["parameters"].as_object().unwrap() {
            let param = d.params.iter().find(|p| &p.id == id).unwrap();
            let value: [f32; 4] = serde_json::from_value(value.clone()).unwrap();
            assert!(
                param.kind.valid_value(&value, param.min, param.max),
                "alias {} parameter {id}",
                alias["name"]
            );
        }
    }
}
