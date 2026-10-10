use motion_effects::{builtin, EffectPackage, RendererKind};
use std::io::Write;

fn rebuild(package: &EffectPackage, manifest: serde_json::Value, omit: Option<&str>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in &package.files {
        if omit == Some(path.as_str()) {
            continue;
        }
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        if path == "manifest.json" {
            writer
                .write_all(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
        } else {
            writer.write_all(bytes).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}
#[test]
fn sdk_five_birth_history_requires_an_explicit_version_and_contract() {
    let package=builtin::particle_package().unwrap();
    assert_eq!(package.manifest.id,"com.motionstudio.effects.particles");
    assert_eq!(package.manifest.sdk_version,5);
    assert_eq!(package.manifest.effects[0].renderer,RendererKind::ParticleEmitter);
    assert!(package.shaders[&("particle_emitter".into(),0)].sprite);
    assert!(package.manifest.effects[0].native_editor.is_some());
    assert!(package.manifest.effects[0].editor.is_none());
    assert!(package.files.keys().all(|p|!p.ends_with(".js")&&!p.ends_with(".html")));
    let mut manifest=serde_json::to_value(&package.manifest).unwrap();manifest["sdk_version"]=4.into();
    assert!(EffectPackage::from_bytes(rebuild(&package,manifest,None)).is_err());
    let mut manifest=serde_json::to_value(&package.manifest).unwrap();manifest["effects"][0]["params"][0]["animatable"]=true.into();
    assert!(EffectPackage::from_bytes(rebuild(&package,manifest,None)).is_err());
    let mut manifest=serde_json::to_value(&package.manifest).unwrap();manifest["effects"][0]["scene"]["particle_space"]=serde_json::Value::Null;
    assert!(EffectPackage::from_bytes(rebuild(&package,manifest,None)).is_err());
    let mut manifest=serde_json::to_value(&package.manifest).unwrap();manifest["effects"][0]["required_capabilities"]=serde_json::json!([]);
    assert!(EffectPackage::from_bytes(rebuild(&package,manifest,None)).is_err());
}
#[test]
fn native_slot_bindings_and_limits_are_validated_before_install() {
    let package=builtin::particle_package().unwrap();
    for slots in [serde_json::json!([{"kind":"parameters","params":["missing"]}]),serde_json::json!([{"kind":"preview","max_size":513}]),serde_json::json!([{"kind":"parameters","params":["size","size"]}]),serde_json::json!([{"kind":"timeline"},{"kind":"timeline"}])] {
        let mut manifest=serde_json::to_value(&package.manifest).unwrap();manifest["effects"][0]["native_editor"]["sections"][0]["slots"]=slots;
        assert!(EffectPackage::from_bytes(rebuild(&package,manifest,None)).is_err());
    }
}
#[test]
fn sdk_two_generators_and_independent_editor_assets_compile_portably() {
    let package = builtin::scene_package().unwrap();
    assert_eq!(package.manifest.version, "1.1.0");
    assert_eq!(package.manifest.sdk_version, 2);
    assert_eq!(package.manifest.effects.len(), 6);
    for e in &package.manifest.effects {
        assert_ne!(e.renderer, RendererKind::Image);
        assert!(e.editor.is_some());
        assert!(e.scene.is_some());
        let shader = &package.shaders[&(e.id.clone(), 0)];
        assert!(shader.sprite);
        assert!(shader.glsl.vertex.contains("#version 300 es"));
        assert!(shader.glsl.vertex.contains("layout(location = 0)"));
    }
    assert_eq!(
        builtin::packages()
            .unwrap()
            .into_iter()
            .find(|p| p.manifest.id == builtin::PLUGIN_ID && p.manifest.version == "1.1.0")
            .unwrap()
            .hash,
        "df9da73a4c18ee5c8d1c652d60bb5c45b01677371e461f65cafc3272746bbbf2"
    );
}

#[test]
fn scene_editor_upgrade_preserves_published_package_and_render_contracts() {
    let old = builtin::legacy_scene_package().unwrap();
    let current = builtin::scene_package().unwrap();
    assert_eq!(old.manifest.version, "1.0.0");
    assert_eq!(
        old.hash,
        "ffd6d7ddbff861072da76dd7f84f072a7b01b187f8488e59e88e4bc468c20724"
    );
    let registry = motion_effects::Registry::new_with_builtins().unwrap();
    assert!(registry
        .resolve(&old.manifest.id, "1.0.0", &old.hash)
        .is_ok());
    assert!(registry
        .resolve(&current.manifest.id, "1.1.0", &current.hash)
        .is_ok());
    for previous in &old.manifest.effects {
        let mut normalized = previous.clone();
        normalized.editor = current
            .manifest
            .effects
            .iter()
            .find(|e| e.id == previous.id)
            .unwrap()
            .editor
            .clone();
        assert_eq!(
            &normalized,
            current
                .manifest
                .effects
                .iter()
                .find(|e| e.id == previous.id)
                .unwrap()
        );
        assert_eq!(
            old.shaders[&(previous.id.clone(), 0)].wgsl,
            current.shaders[&(previous.id.clone(), 0)].wgsl
        );
    }
}
#[test]
fn invalid_editor_assets_sdk_versions_and_simulation_contracts_are_rejected() {
    let package = builtin::scene_package().unwrap();
    let original = serde_json::to_value(&package.manifest).unwrap();
    assert!(
        EffectPackage::from_bytes(rebuild(&package, original.clone(), Some("ui/editor.js")))
            .is_err()
    );
    let mut manifest = original.clone();
    manifest["sdk_version"] = serde_json::json!(1);
    assert!(EffectPackage::from_bytes(rebuild(&package, manifest, None)).is_err());
    let mut manifest = original.clone();
    manifest["effects"][0]["editor"]["entry"] = serde_json::json!("../secret.html");
    assert!(EffectPackage::from_bytes(rebuild(&package, manifest, None)).is_err());
    let mut manifest = original;
    manifest["effects"][1]["params"][0]["animatable"] = serde_json::json!(true);
    assert!(EffectPackage::from_bytes(rebuild(&package, manifest, None)).is_err());
}

#[test]
fn effects_can_share_editor_png_without_multiplying_the_resource_budget() {
    let package = builtin::scene_package().unwrap();
    let directory = tempfile::tempdir().unwrap();
    for (path, bytes) in &package.files {
        let destination = directory.path().join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, bytes).unwrap();
    }
    let mut manifest = package.manifest.clone();
    for effect in &mut manifest.effects {
        effect
            .editor
            .as_mut()
            .unwrap()
            .files
            .push("ui/shared.png".into());
    }
    std::fs::write(
        directory.path().join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    // One 36 MiB decoded image fits; counting it for all six editors does not.
    image::RgbaImage::new(3072, 3072)
        .save(directory.path().join("ui/shared.png"))
        .unwrap();
    motion_effects::package_directory(directory.path()).unwrap();
}
