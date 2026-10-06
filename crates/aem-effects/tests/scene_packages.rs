use aem_effects::{builtin, EffectPackage, RendererKind};
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
fn sdk_two_generators_and_independent_editor_assets_compile_portably() {
    let package = builtin::scene_package().unwrap();
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
    aem_effects::package_directory(directory.path()).unwrap();
}
