use motion_core::{storage, Asset, Project};
#[test]
fn loading_a_missing_asset_names_it_and_does_not_overwrite_the_project() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    let asset = root.path().join("assets/alpha.png");
    std::fs::write(&asset, b"test-owned asset").unwrap();
    let mut p = Project::demo();
    p.assets.push(Asset {
        id: 1,
        path: "assets/alpha.png".into(),
        width: 2,
        height: 2,
    });
    storage::save(root.path(), &p).unwrap();
    let json = std::fs::read(root.path().join("project.json")).unwrap();
    std::fs::remove_file(asset).unwrap();
    let error = storage::load(root.path()).unwrap_err().to_string();
    assert!(error.contains("assets/alpha.png"), "{error}");
    assert_eq!(
        json,
        std::fs::read(root.path().join("project.json")).unwrap()
    );
}
