//! Transaction tests inject probing; real platform decode is covered by Android instrumentation.
use aem_core::{Command, Engine, Project, VideoAsset};
use aem_media::{AudioMixer, ProbeVideo, VideoImportOptions, VideoJobs, VideoProbe};
use std::{
    fs::File,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
const VIDEO: &str = "tests/fixtures/video/sound-24fps.mp4";
fn probe_stub() -> ProbeVideo {
    Arc::new(|source, _, audio, check| {
        check()?;
        let pts = (0..72)
            .map(|n| (n * 1_000_000 + 12) / 24)
            .collect::<Vec<_>>();
        Ok(VideoProbe {
            asset: VideoAsset {
                id: 1,
                path: "assets/probe.mp4".into(),
                bytes: std::fs::metadata(source).unwrap().len(),
                mime: "video/avc".into(),
                track: 0,
                width: 256,
                height: 144,
                rotation: 0,
                display_width: 256,
                display_height: 144,
                video_start_us: 0,
                video_end_us: 3_000_000,
                duration_us: 3_000_000,
                frame_count: 72,
                variable_frame_rate: false,
                nominal_frame_rate: 24.0,
                color_standard: 1,
                color_range: 2,
                audio_asset: None,
            },
            timestamps: pts,
            audio_track: if audio == Some(u32::MAX) {
                None
            } else {
                Some(1)
            },
            first_rgba: vec![255; 256 * 144 * 4],
            tracks: serde_json::json!([]),
        })
    })
}
fn options(id: &str) -> VideoImportOptions {
    VideoImportOptions {
        request_id: id.into(),
        at_frame: 0,
        name: "video".into(),
        track: None,
        audio_track: None,
        with_audio: true,
    }
}
fn start(jobs: &VideoJobs, id: &str) {
    jobs.start(
        || {
            let f = File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join(VIDEO)).unwrap();
            let bytes = f.metadata().unwrap().len();
            Ok((Box::new(f), Some(bytes)))
        },
        options(id),
        false,
        probe_stub(),
    )
    .unwrap();
}
fn wait(jobs: &VideoJobs, id: &str) -> aem_media::VideoTaskStatus {
    let time = Instant::now();
    loop {
        let t = jobs.status(id).unwrap();
        if t.state != "running" {
            return t;
        }
        assert!(time.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn source_and_original_audio_share_one_owned_file_and_one_history_entry() {
    let root = tempfile::tempdir().unwrap();
    let jobs = VideoJobs::new(root.path().into()).unwrap();
    let mut engine = Engine::new(Project::new(256, 144, 60, 240).unwrap()).unwrap();
    start(&jobs, "import");
    assert_eq!(wait(&jobs, "import").state, "ready");
    let t = jobs.commit("import", &mut engine).unwrap();
    assert_eq!(t.state, "succeeded", "{:?}", t.error);
    assert_eq!(
        std::fs::read_dir(root.path().join("assets"))
            .unwrap()
            .count(),
        1
    );
    let p = engine.project();
    assert_eq!(p.video_assets[0].audio_asset, Some(p.audio_assets[0].id));
    assert_eq!(p.video_assets[0].path, p.audio_assets[0].path);
    let mut mixer = AudioMixer::new(p.clone(), root.path()).unwrap();
    let mut pcm = vec![0.; 2048];
    mixer.mix(6000, &mut pcm).unwrap();
    assert!(pcm.iter().any(|s| s.abs() > 0.03));
    let old = p.clone();
    engine
        .apply_batch(vec![Command::SplitLayerClip {
            object: 1,
            frame: 15,
        }])
        .unwrap();
    let mut split = AudioMixer::new(engine.snapshot(), root.path()).unwrap();
    let mut after = vec![0.; 2048];
    split.mix(6000, &mut after).unwrap();
    assert_eq!(pcm, after);
    engine.undo().unwrap();
    assert_eq!(*engine.project(), old);
    engine.undo().unwrap();
    assert!(engine.project().layers.is_empty());
    assert!(root.path().join(&old.video_assets[0].path).is_file());
    engine.redo().unwrap();
    assert_eq!(*engine.project(), old);
    let package = root.path().join("project.aem");
    aem_core::storage::export_package(root.path(), engine.project(), &package).unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&package).unwrap()).unwrap();
    assert_eq!(archive.len(), 2);
    assert!(archive.by_name(&old.video_assets[0].path).is_ok());
    let destination = root.path().join("restored");
    assert_eq!(
        aem_core::storage::import_package(&package, &destination).unwrap(),
        old
    );
    jobs.prepare_cache("rebuild", old.video_assets[0].clone(), probe_stub())
        .unwrap();
    assert_eq!(wait(&jobs, "rebuild").state, "succeeded");
}

#[test]
fn cancelled_probe_and_failed_save_preserve_the_original_project() {
    let root = tempfile::tempdir().unwrap();
    let jobs = VideoJobs::new(root.path().into()).unwrap();
    let mut engine = Engine::new(Project::new(256, 144, 60, 240).unwrap()).unwrap();
    let before = engine.snapshot();
    start(&jobs, "cancel");
    assert_eq!(wait(&jobs, "cancel").state, "ready");
    assert_eq!(jobs.cancel("cancel").unwrap().state, "cancelled");
    jobs.commit("cancel", &mut engine).unwrap();
    assert_eq!(before, engine.snapshot());
    start(&jobs, "save-fail");
    assert_eq!(wait(&jobs, "save-fail").state, "ready");
    std::fs::create_dir(root.path().join("project.json")).unwrap();
    let t = jobs.commit("save-fail", &mut engine).unwrap();
    assert_eq!(t.state, "failed");
    assert_eq!(before, engine.snapshot());
    assert!(!engine.can_undo());
    assert_eq!(
        std::fs::read_dir(root.path().join("assets"))
            .unwrap()
            .count(),
        0
    );
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".video-import")));
}
