use aem_effects::{builtin, shader, EffectPackage, Registry};
use std::{io::Write, sync::Arc};

#[test]
fn entire_library_compiles_to_wgsl_and_es300() {
    let package = builtin::package().unwrap();
    assert_eq!(package.manifest.effects.len(), 20);
    for e in &package.manifest.effects {
        assert_eq!(e.reference_version, "18.0.1");
        for i in 0..e.passes.len() {
            let shader = &package.shaders[&(e.id.clone(), i)];
            assert!(shader.glsl.fragment.contains("#version 300 es"));
            assert!(shader.glsl.vertex.contains("#version 300 es"));
            assert!(shader.glsl.fragment.contains("layout(std140) uniform"));
        }
    }
    assert!(package
        .manifest
        .effects
        .iter()
        .all(|e| e.compatibility == aem_effects::Compatibility::Approximate));
}
#[test]
fn archive_rejects_parent_paths_and_duplicate_case_names() {
    for names in [
        vec!["../manifest.json"],
        vec!["manifest.json", "MANIFEST.JSON"],
    ] {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in names {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"{}").unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        assert!(EffectPackage::from_bytes(bytes).is_err());
    }
}
#[test]
fn png_with_valid_dimensions_but_truncated_pixels_is_rejected_at_import() {
    // Synthetic input keeps clean checkouts independent of local AE references.
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(256, 256, |x, y| {
        image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    })
    .write_to(&mut encoded, image::ImageFormat::Png)
    .unwrap();
    let bytes = encoded.into_inner();
    let truncated = &bytes[..bytes.len() / 2];
    assert_eq!(
        image::ImageReader::new(std::io::Cursor::new(truncated))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap(),
        (256, 256)
    );
    let package = builtin::package().unwrap();
    let mut manifest = builtin::manifest();
    manifest.effects[0]
        .resources
        .push("assets/broken.png".into());
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in &package.files {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&if name == "manifest.json" {
                serde_json::to_vec(&manifest).unwrap()
            } else {
                data.clone()
            })
            .unwrap();
    }
    writer
        .start_file(
            "assets/broken.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(truncated).unwrap();
    let error = EffectPackage::from_bytes(writer.finish().unwrap().into_inner())
        .err()
        .unwrap();
    assert!(error.to_string().contains("resource assets/broken.png"));
}
#[test]
fn shader_rejects_unbounded_work_and_additional_bindings() {
    for source in ["fn main_fx(p:vec2<f32>)->vec4<f32>{loop {} return vec4(0.0);}","fn main_fx(p:vec2<f32>)->vec4<f32>{for(var i:i32=0;i<i32(fx.params[0].x);i=i+1){}return vec4(0.0);}","@group(9) @binding(0) var image:texture_2d<f32>;fn main_fx(p:vec2<f32>)->vec4<f32>{return vec4(0.0);}"]{assert!(shader::compile(source,"main_fx").is_err());}
    assert!(shader::compile(
        "fn main_fx(p:vec2<f32>)->vec4<f32>{for(var i:i32=0;i<10;i=i+1){i=0;}return vec4(0.0);}",
        "main_fx"
    )
    .is_err());
}
#[test]
fn installation_is_idempotent_and_snapshots_hold_resources() {
    let dir = tempfile::tempdir().unwrap();
    let p = builtin::package().unwrap();
    let input = dir.path().join("input.msfx");
    std::fs::write(&input, &p.bytes).unwrap();
    let store = dir.path().join("plugins");
    let mut r = Registry::default();
    let a = r.install(&store, &input).unwrap();
    let b = r.install(&store, &input).unwrap();
    assert_eq!(a.hash, b.hash);
    let snapshot = r.resolve(&a.id, &a.version, &a.hash).unwrap();
    r.enable(&store, &a.id, &a.version, &a.hash, false).unwrap();
    assert!(r.resolve(&a.id, &a.version, &a.hash).is_err());
    assert_eq!(snapshot.manifest.effects.len(), 20);
    assert!(!r.install(&store, &input).unwrap().enabled);
    r.enable(&store, &a.id, &a.version, &a.hash, true).unwrap();
    let loaded = Registry::load(&store).unwrap();
    assert!(loaded.resolve(&a.id, &a.version, &a.hash).is_ok());
    let mut changed = builtin::manifest();
    changed.name.push_str(" changed");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in &p.files {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        if name == "manifest.json" {
            writer
                .write_all(&serde_json::to_vec(&changed).unwrap())
                .unwrap();
        } else {
            writer.write_all(data).unwrap();
        }
    }
    let changed = EffectPackage::from_bytes(writer.finish().unwrap().into_inner()).unwrap();
    assert!(r.insert(Arc::new(changed)).is_err());
}

#[test]
fn fresh_registry_retains_bundled_versions_and_checks_other_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("plugins");
    let mut stale = Registry::load(&store).unwrap();
    let p = builtin::package().unwrap();
    assert!(store.join(format!("{}.msfx", p.hash)).is_file());
    let make = |name: &str| {
        let mut manifest = builtin::manifest();
        manifest.id = "test.custom".into();
        manifest.name = name.into();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in &p.files {
            writer
                .start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(&if path == "manifest.json" {
                    serde_json::to_vec(&manifest).unwrap()
                } else {
                    bytes.clone()
                })
                .unwrap();
        }
        writer.finish().unwrap().into_inner()
    };
    let a = dir.path().join("a.msfx");
    let b = dir.path().join("b.msfx");
    std::fs::write(&a, make("first")).unwrap();
    std::fs::write(&b, make("changed")).unwrap();
    Registry::load(&store).unwrap().install(&store, &a).unwrap();
    assert!(stale.install(&store, &b).is_err());
    assert!(Registry::load(&store).unwrap().diagnostics.is_empty());
}
