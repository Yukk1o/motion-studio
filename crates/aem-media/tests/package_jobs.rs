use aem_core::{
    storage::{self, PackageSnapshot},
    Asset, Project,
};
use aem_media::{PackageJobs, PackageTaskStatus};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
fn fixture(root: &Path) -> Project {
    fs::create_dir(root.join("assets")).unwrap();
    fs::write(root.join("assets/one.png"), vec![17; 2 * 1024 * 1024]).unwrap();
    let mut project = Project::new(64, 64, 30, 90).unwrap();
    project.assets.push(Asset {
        id: 1,
        path: "assets/one.png".into(),
        width: 64,
        height: 64,
    });
    project
}
fn wait(jobs: &PackageJobs, id: &str) -> PackageTaskStatus {
    let start = Instant::now();
    loop {
        let status = jobs.status(id).unwrap();
        if status.state != "running" {
            return status;
        }
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn asynchronous_export_is_frozen_and_reports_payload_progress() {
    let root = tempfile::tempdir().unwrap();
    let mut project = fixture(root.path());
    let old = project.clone();
    let jobs = PackageJobs::default();
    let snapshot = PackageSnapshot::new(root.path(), &project).unwrap();
    let bytes = snapshot.total_bytes();
    let status = jobs
        .start_export("pack", snapshot, root.path().join("frozen.aem"), 77)
        .unwrap();
    assert_eq!(status.state, "running");
    assert_eq!(status.frozen_revision, 77);
    assert_eq!(status.source_count, 1);
    assert_eq!(status.total_bytes, bytes);
    assert!(status.path.is_none());
    project.name = "new name".into();
    storage::save(root.path(), &project).unwrap();
    let status = wait(&jobs, "pack");
    assert_eq!(status.state, "succeeded", "{:?}", status.error);
    assert_eq!(status.bytes_processed, bytes);
    assert_eq!(status.progress, 1.);
    assert_eq!(
        storage::import_package(status.path.as_ref().unwrap(), &root.path().join("restored"))
            .unwrap(),
        old
    );
    assert_eq!(jobs.cancel("pack").unwrap().state, "succeeded");
    assert!(jobs
        .start_export(
            "pack",
            PackageSnapshot::new(root.path(), &project).unwrap(),
            root.path().join("other.aem"),
            78
        )
        .is_err());
    jobs.release("pack").unwrap();
    assert!(!jobs.contains("pack"));
}
#[test]
fn publication_failure_is_terminal_and_keeps_previous_destination() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    let output = root.path().join("cannot-replace.aem");
    fs::create_dir(&output).unwrap();
    let jobs = PackageJobs::default();
    jobs.start_export(
        "bad",
        PackageSnapshot::new(root.path(), &project).unwrap(),
        output.clone(),
        0,
    )
    .unwrap();
    let status = wait(&jobs, "bad");
    assert_eq!(status.state, "failed");
    assert!(status.error.is_some());
    assert!(status.path.is_none());
    assert!(output.is_dir());
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ms-package-")));
    jobs.release("bad").unwrap();
}

#[test]
fn cancelling_a_background_export_never_publishes_partial_output() {
    let root = tempfile::tempdir().unwrap();
    let project = fixture(root.path());
    fs::write(
        root.path().join("assets/one.png"),
        vec![17; 32 * 1024 * 1024],
    )
    .unwrap();
    let output = root.path().join("old.aem");
    fs::write(&output, b"previous output").unwrap();
    let jobs = PackageJobs::default();
    jobs.start_export(
        "cancel",
        PackageSnapshot::new(root.path(), &project).unwrap(),
        output.clone(),
        0,
    )
    .unwrap();
    let acknowledged = jobs.cancel("cancel").unwrap();
    let done = wait(&jobs, "cancel");
    if acknowledged.state == "succeeded" {
        // Completion won the lock; cancellation cannot undo a published result.
        assert_eq!(done.state, "succeeded");
        storage::import_package(&output, &root.path().join("restored")).unwrap();
    } else {
        assert_eq!(acknowledged.phase, "cancelling");
        assert_eq!(done.state, "cancelled");
        assert!(done.path.is_none());
        assert_eq!(fs::read(&output).unwrap(), b"previous output");
    }
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ms-package-")));
    jobs.release("cancel").unwrap();
}
