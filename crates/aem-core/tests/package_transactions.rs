use aem_core::{
    storage::{self, PackageSnapshot},
    Asset, Error, Project,
};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

fn fixture(root: &Path) -> Project {
    fs::create_dir(root.join("assets")).unwrap();
    fs::write(root.join("assets/image.png"), vec![37; 1024 * 1024]).unwrap();
    let mut project = Project::new(64, 64, 30, 90).unwrap();
    project.assets.push(Asset {
        id: 1,
        path: "assets/image.png".into(),
        width: 64,
        height: 64,
    });
    storage::save(root, &project).unwrap();
    project
}
fn no_stages(root: &Path) {
    assert!(!fs::read_dir(root).unwrap().any(|e| {
        let name = e.unwrap().file_name();
        let name = name.to_string_lossy();
        name.starts_with(".ms-package-") || name.starts_with(".aem-import-")
    }));
}
#[test]
fn frozen_package_keeps_project_and_open_source_after_replacement() {
    let root = tempfile::tempdir().unwrap();
    let mut project = fixture(root.path());
    let before = project.clone();
    let snapshot = PackageSnapshot::new(root.path(), &project).unwrap();
    assert_eq!(snapshot.source_count(), 1);
    project.name = "edited after export started".into();
    storage::save(root.path(), &project).unwrap();
    fs::remove_file(root.path().join("assets/image.png")).unwrap();
    fs::write(root.path().join("assets/image.png"), b"replacement").unwrap();
    let output = root.path().join("frozen.aem");
    let total = snapshot.total_bytes();
    let mut previous = 0;
    snapshot
        .prepare(&output, |done, expected| {
            assert!(done >= previous && done <= total);
            assert_eq!(expected, total);
            previous = done;
            Ok(())
        })
        .unwrap()
        .publish()
        .unwrap();
    assert_eq!(previous, total);
    let mut zip = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
    assert_eq!(
        zip.by_name("assets/image.png").unwrap().compression(),
        zip::CompressionMethod::Stored
    );
    assert_eq!(
        storage::import_package(&output, &root.path().join("restored")).unwrap(),
        before
    );
    assert!(fs::read(root.path().join("restored/assets/image.png"))
        .unwrap()
        .iter()
        .all(|b| *b == 37));
    no_stages(root.path());
}
#[test]
fn cancellation_removes_owned_staging_and_keeps_existing_output() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let output = root.path().join("project.aem");
    fs::write(&output, b"existing output").unwrap();
    let mut calls = 0;
    let error = storage::export_package_with_progress(root.path(), &project, &output, |_, _| {
        calls += 1;
        if calls > 6 {
            Err(Error::Invalid("cancelled".into()))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    no_stages(root.path());
    storage::export_package(root.path(), &project, &output).unwrap();
    let original_json = fs::read(root.path().join("project.json")).unwrap();
    let destination = root.path().join("imported");
    let mut calls = 0;
    assert!(
        storage::import_package_with_progress(&output, &destination, |_, _| {
            calls += 1;
            if calls > 6 {
                Err(Error::Invalid("cancelled".into()))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert!(!destination.exists());
    assert_eq!(
        fs::read(root.path().join("project.json")).unwrap(),
        original_json
    );
    no_stages(root.path());
}
#[test]
fn abandoned_prepared_package_and_failed_publish_do_not_leave_temporary_files() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let output = root.path().join("output.aem");
    let prepared = PackageSnapshot::new(root.path(), &project)
        .unwrap()
        .prepare(&output, |_, _| Ok(()))
        .unwrap();
    assert!(!output.exists());
    drop(prepared);
    no_stages(root.path());
    fs::create_dir(&output).unwrap();
    assert!(storage::export_package(root.path(), &project, &output).is_err());
    assert!(output.is_dir());
    no_stages(root.path());
}
#[test]
fn packaging_rejects_source_length_changes_and_overwriting_sources() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    for length in [9, 2 * 1024 * 1024] {
        fs::write(root.path().join("assets/image.png"), vec![37; 1024 * 1024]).unwrap();
        let snapshot = PackageSnapshot::new(root.path(), &project).unwrap();
        OpenOptions::new()
            .write(true)
            .open(root.path().join("assets/image.png"))
            .unwrap()
            .set_len(length)
            .unwrap();
        let result = snapshot.prepare(&root.path().join("output.aem"), |_, _| Ok(()));
        assert!(result.err().unwrap().to_string().contains("source changed"));
        assert!(!root.path().join("output.aem").exists());
        no_stages(root.path());
    }
    for output in ["project.json", "assets/image.png"] {
        let before = fs::read(root.path().join(output)).unwrap();
        assert!(storage::export_package(root.path(), &project, &root.path().join(output)).is_err());
        assert_eq!(fs::read(root.path().join(output)).unwrap(), before);
    }
}
