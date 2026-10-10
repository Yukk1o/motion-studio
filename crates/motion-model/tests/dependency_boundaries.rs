use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
};

fn closure(name: &str, packages: &BTreeMap<String, Value>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![name.to_owned()];
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(package) = packages.get(&name) {
            for dependency in package["dependencies"].as_array().unwrap() {
                if dependency["kind"].as_str() != Some("dev") {
                    pending.push(dependency["name"].as_str().unwrap().to_owned());
                }
            }
        }
    }
    seen
}

#[test]
fn production_dependencies_keep_data_render_and_media_outside_the_editing_runtime() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--locked",
            "--offline",
        ])
        .current_dir(workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages: BTreeMap<_, _> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap().to_owned(), p.clone()))
        .collect();
    for name in ["motion-model", "motion-render", "motion-media"] {
        let dependencies = closure(name, &packages);
        for forbidden in [
            "motion-core",
            "motion-host",
            "motion-android",
            "motion-desktop",
        ] {
            assert!(
                !dependencies.contains(forbidden),
                "{name} depends on {forbidden}"
            );
        }
        assert!(
            !dependencies
                .iter()
                .any(|d| d.starts_with("rquickjs") || d.starts_with("swc_")),
            "{name} contains a JS runtime dependency"
        );
    }
    let model = closure("motion-model", &packages);
    assert!(!model.contains("motion-render") && !model.contains("motion-media"));
    let core = closure("motion-core", &packages);
    assert!(core.contains("rquickjs") && core.contains("swc_ecma_parser"));
    for forbidden in [
        "motion-render",
        "motion-media",
        "wgpu",
        "winit",
        "egui",
        "cpal",
    ] {
        assert!(!core.contains(forbidden), "core depends on {forbidden}");
    }
    assert!(!packages["motion-effects"]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["kind"].as_str() != Some("dev")
            && d["name"].as_str().unwrap().starts_with("motion-")));
}
