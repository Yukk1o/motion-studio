use motion_core::{Command, Content, Engine, Layer, Project};
use motion_media::{AudioJobs, AudioMixer, ImportOptions, Limits};
use std::{
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn a_slow_source_open_does_not_block_the_editor_and_can_be_cancelled() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (continue_tx, continue_rx) = std::sync::mpsc::channel();
    jobs.start_with_source(
        move || {
            started_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            Ok((
                Box::new(File::open(fixture("tone-stereo-48000.wav")).unwrap()),
                None,
            ))
        },
        ImportOptions {
            request_id: "blocked-open".into(),
            at_frame: 0,
            track: None,
            name: "audio".into(),
        },
        false,
    )
    .unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    e.apply(Command::Add {
        layer: Layer::solid(1, "editing", [16.0; 2], [0.0; 3], [1.0; 4]),
    })
    .unwrap();
    assert_eq!(jobs.cancel("blocked-open").unwrap().state, "cancelled");
    continue_tx.send(()).unwrap();
    assert_eq!(
        jobs.commit("blocked-open", &mut e).unwrap().state,
        "cancelled"
    );
    assert_eq!(e.project().layers.len(), 1);
}

#[test]
fn two_instances_of_one_source_keep_distinct_source_offsets() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    import(&jobs, &mut e, "audio", "tone-stereo-48000.wav", 0);
    let mut reference = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut a = vec![0.0; 8192];
    let mut b = a.clone();
    reference.mix(8000, &mut a).unwrap();
    reference.mix(32000, &mut b).unwrap();
    e.apply(Command::Duplicate { object: 1 }).unwrap();
    let mut audio = motion_core::AudioClip::new(1);
    audio.source_offset_us = 500_000;
    e.apply_batch(vec![
        Command::TrimLayerClip {
            object: 2,
            in_frame: 0,
            out_frame: 45,
        },
        Command::Content {
            object: 2,
            content: Content::Audio { audio },
            size: [0.0; 2],
        },
    ])
    .unwrap();
    let mut mixer = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut actual = vec![0.0; a.len()];
    mixer.mix(8000, &mut actual).unwrap();
    assert_eq!(
        actual,
        a.iter()
            .zip(&b)
            .map(|(a, b)| (a + b).clamp(-1.0, 1.0))
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "large-file acceptance: run explicitly in release mode"]
fn imports_128_mib_sources_and_roundtrips_a_project_over_256_mib() {
    use std::io::Write;
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("large.wav");
    let data_bytes = 128u32 * 1024 * 1024;
    let mut f = File::create(&source).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(data_bytes + 36).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&48000u32.to_le_bytes()).unwrap();
    f.write_all(&192000u32.to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_bytes.to_le_bytes()).unwrap();
    let mut block = vec![0; 65536];
    for n in (0..16384).step_by(4096) {
        for channel in 0..2 {
            block[n * 4 + channel * 2..n * 4 + channel * 2 + 2]
                .copy_from_slice(&16384i16.to_le_bytes());
        }
    }
    for _ in 0..data_bytes as usize / block.len() {
        f.write_all(&block).unwrap();
    }
    drop(f);
    let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = Engine::new(Project::new(256, 256, 30, 36000).unwrap()).unwrap();
    let start = Instant::now();
    for n in 0..2 {
        let id = format!("large-{n}");
        jobs.start(
            Box::new(File::open(&source).unwrap()),
            Some(u64::from(data_bytes) + 44),
            ImportOptions {
                request_id: id.clone(),
                at_frame: 0,
                track: None,
                name: id.clone(),
            },
            false,
        )
        .unwrap();
        let ready = wait(&jobs, &id);
        assert_eq!(ready.state, "ready", "{ready:?}");
        assert_eq!(jobs.commit(&id, &mut e).unwrap().state, "succeeded");
    }
    let import_ms = start.elapsed().as_millis();
    let bytes: u64 = e.project().audio_assets.iter().map(|a| a.bytes).sum();
    assert!(bytes > 256 * 1024 * 1024);
    let mut mixer = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut out = vec![0.0; 8192];
    let mix_start = Instant::now();
    for start in (0..48000u64 * 60).step_by(4096) {
        let frames = mixer.mix(start, &mut out).unwrap();
        for n in 0..frames {
            let expected = if (start + n as u64) % 4096 == 0 {
                1.0
            } else {
                0.0
            };
            assert_eq!(out[n * 2], expected);
            assert_eq!(out[n * 2 + 1], expected);
        }
    }
    let mix_ms = mix_start.elapsed().as_millis();
    assert!(mix_ms < 12000, "60s audio exceeded CPU budget: {mix_ms}ms");
    let archive = root.path().join("backup.msproj");
    motion_core::storage::export_package(root.path(), e.project(), &archive).unwrap();
    let restored =
        motion_core::storage::import_package(&archive, &root.path().join("restored")).unwrap();
    assert_eq!(&restored, e.project());
    println!(
        "{}",
        serde_json::json!({"source_bytes":bytes,"assets":2,"import_ms":import_ms,"mixed_seconds":60,"mix_ms":mix_ms,"sample_clock_exact":true,"package_roundtrip":true})
    );
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
fn wait(j: &AudioJobs, id: &str) -> motion_media::TaskStatus {
    let start = Instant::now();
    loop {
        let s = j.status(id).unwrap();
        if s.state != "running" {
            return s;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn import(j: &AudioJobs, e: &mut Engine, id: &str, name: &str, at: u32) -> u64 {
    j.start(
        Box::new(File::open(fixture(name)).unwrap()),
        None,
        ImportOptions {
            request_id: id.into(),
            at_frame: at,
            track: None,
            name: name.into(),
        },
        false,
    )
    .unwrap();
    let s = wait(j, id);
    assert_eq!(s.state, "ready", "{:?}", s);
    let s = j.commit(id, e).unwrap();
    assert_eq!(s.state, "succeeded", "{:?}", s);
    s.edit_result.unwrap()["object"].as_u64().unwrap()
}
fn engine() -> Engine {
    Engine::new(Project::new(256, 256, 30, 180).unwrap()).unwrap()
}
#[test]
fn two_level_precomposition_preserves_pcm_and_offsets_with_shared_assets() {
    let _guard=TEST_LOCK.lock().unwrap();let root=tempfile::tempdir().unwrap();let jobs=AudioJobs::new(root.path().into(),Limits::default()).unwrap();let mut e=engine();
    let a=import(&jobs,&mut e,"first-nested","tone-stereo-48000.wav",7);
    let b=import(&jobs,&mut e,"second-nested","tone-mono-44100.mp3",23);
    let before=e.snapshot();
    e.apply(Command::Composition{action:motion_core::CompositionAction::Precompose{objects:vec![a,b],name:"inner".into(),range:"composition".into()}}).unwrap();
    let reference=e.project().layers[0].id;
    e.apply(Command::Composition{action:motion_core::CompositionAction::Precompose{objects:vec![reference],name:"outer".into(),range:"composition".into()}}).unwrap();
    let frozen=e.snapshot();let mut original=AudioMixer::new(before,root.path()).unwrap();let mut nested=AudioMixer::new(frozen.clone(),root.path()).unwrap();
    for start in [0,48000,12000,96000,36900,48000] {let mut a=vec![0.;12000*2];let mut b=a.clone();original.mix(start,&mut a).unwrap();nested.mix(start,&mut b).unwrap();assert_eq!(a,b,"sample {start}");}
    let mut p=frozen;let inner=p.compositions.iter().find(|c|c.name=="inner").unwrap().id.clone();
    let settings=motion_core::CompositionSettings{name:"inner60".into(),width:256,height:256,fps:60,frames:360,timing:"preserve_seconds".into(),shorten:"reject".into()};
    let mut e=Engine::new(p.clone()).unwrap();e.apply(Command::InComposition{composition:inner,command:Box::new(Command::Composition{action:motion_core::CompositionAction::Settings{settings}})}).unwrap();p=e.snapshot();
    let mut retimed=AudioMixer::new(p,root.path()).unwrap();let mut a=vec![0.;24000*2];let mut b=a.clone();nested.mix(36000,&mut a).unwrap();retimed.mix(36000,&mut b).unwrap();assert_eq!(a,b);
}

#[test]
fn nondivisor_frame_rates_preserve_frozen_pcm_across_two_precompositions() {
    let _guard = TEST_LOCK.lock().unwrap();
    for fps in [59, 144] {
        let root = tempfile::tempdir().unwrap();
        let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
        let mut e = Engine::new(Project::new(256, 256, fps, fps * 3).unwrap()).unwrap();
        let a = import(&jobs, &mut e, "stereo", "tone-stereo-48000.wav", 7);
        let b = import(&jobs, &mut e, "mono", "tone-mono-44100.mp3", 23);
        let original = e.snapshot();
        e.apply(Command::Composition { action: motion_core::CompositionAction::Precompose {
            objects: vec![a, b], name: "inner".into(), range: "composition".into(),
        }}).unwrap();
        let reference = e.project().layers[0].id;
        e.apply(Command::Composition { action: motion_core::CompositionAction::Precompose {
            objects: vec![reference], name: "outer".into(), range: "composition".into(),
        }}).unwrap();
        let mut before = AudioMixer::new(original, root.path()).unwrap();
        let mut after = AudioMixer::new(e.snapshot(), root.path()).unwrap();
        for start in [0, 48000, 12000, 96000, 36900, 48000] {
            let mut a = vec![0.; 12000 * 2]; let mut b = a.clone();
            before.mix(start, &mut a).unwrap(); after.mix(start, &mut b).unwrap();
            assert_eq!(a, b, "fps {fps}, sample {start}");
        }
    }
}

#[test]
fn all_three_formats_decode_owned_audio_and_reopen_without_external_source() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    for (id, name, rate, ch) in [
        ("wav", "tone-stereo-48000.wav", 48000, 2),
        ("mp3", "tone-mono-44100.mp3", 44100, 1),
        ("aac", "tone-stereo-48000.m4a", 48000, 2),
    ] {
        import(&jobs, &mut e, id, name, 30);
        let a = e.project().audio_assets.last().unwrap();
        assert_eq!((a.sample_rate, a.channels), (rate, ch));
        assert!(
            a.duration_us.abs_diff(2_000_000) <= 1000,
            "{} duration {}",
            id,
            a.duration_us
        );
        assert_eq!(e.project().layers.last().unwrap().size, [0.0; 2]);
    }
    let reopened = motion_core::storage::load(root.path()).unwrap();
    assert_eq!(&reopened, e.project());
    let mut scene = motion_core::Scene::new(&reopened);
    scene.sample(&reopened, 45.0, None).unwrap();
    assert!(scene.layers.is_empty());
    let mut m = AudioMixer::new(reopened, root.path()).unwrap();
    let mut out = vec![0.0; 4800 * 2];
    m.mix(48000 + 4800, &mut out).unwrap();
    assert!(out.iter().any(|v| v.abs() > 0.2));
    let wave = m.waveform(1, 0, 30).unwrap();
    assert!(wave[..9].iter().all(|w| w.max == 0.0));
    assert!(wave[15].rms > 0.15);
}
#[test]
fn sample_mapping_split_move_visibility_and_frozen_mixes_are_exact() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let j = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    let id = import(&j, &mut e, "wav", "tone-stereo-48000.wav", 0);
    let original = e.snapshot();
    let mut frozen = AudioMixer::new(original.clone(), root.path()).unwrap();
    let mut reference = vec![0.0; 48000 * 2];
    frozen.mix(48000, &mut reference).unwrap();
    e.apply(Command::SplitLayerClip {
        object: id,
        frame: 30,
    })
    .unwrap();
    let mut split = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut b = vec![0.0; reference.len()];
    split.mix(48000, &mut b).unwrap();
    assert_eq!(b, reference);
    e.apply(Command::Flags {
        object: id,
        visible: false,
        locked: false,
    })
    .unwrap();
    let mut hidden = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut first = vec![0.0; 48000 * 2];
    hidden.mix(0, &mut first).unwrap();
    assert!(first.iter().any(|v| v.abs() > 0.2));
    e.apply(Command::SetAudio {
        object: 2,
        volume: None,
        muted: Some(true),
    })
    .unwrap();
    let mut muted = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    muted.mix(48000, &mut b).unwrap();
    assert!(b.iter().all(|v| *v == 0.0));
    frozen.mix(48000, &mut b).unwrap();
    assert_eq!(b, reference);
    e.undo().unwrap();
    e.apply(Command::MoveLayerClip {
        object: 2,
        in_frame: 60,
    })
    .unwrap();
    let mut moved = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    moved.mix(96000, &mut b).unwrap();
    assert_eq!(b, reference);
    let p = e.snapshot();
    let rev = e.revision();
    assert!(e
        .apply(Command::TrimLayerClip {
            object: 2,
            in_frame: 60,
            out_frame: 121
        })
        .is_err());
    assert_eq!(p, e.snapshot());
    assert_eq!(rev, e.revision());
    assert!(e
        .apply(Command::SetLayer3d {
            object: id,
            enabled: true
        })
        .is_err());
    let solid = Layer::solid(3, "shape", [20.0; 2], [0.0; 3], [1.0; 4]);
    e.apply(Command::Add { layer: solid }).unwrap();
    assert!(e
        .apply(Command::Parent {
            object: 3,
            parent: Some(id),
            frame: 0
        })
        .is_err());
    assert!(e
        .apply(Command::Parent {
            object: id,
            parent: Some(3),
            frame: 0
        })
        .is_err());
}
#[test]
fn cancel_fail_and_save_fail_never_publish_partial_import_or_history() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let j = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    let before = e.snapshot();
    j.start(
        Box::new(Cursor::new(vec![1; 100])),
        None,
        ImportOptions {
            request_id: "bad".into(),
            at_frame: 0,
            track: None,
            name: "bad".into(),
        },
        false,
    )
    .unwrap();
    assert_eq!(wait(&j, "bad").state, "failed");
    assert_eq!(j.commit("bad", &mut e).unwrap().state, "failed");
    assert_eq!(e.snapshot(), before);
    j.start(
        Box::new(File::open(fixture("tone-stereo-48000.wav")).unwrap()),
        None,
        ImportOptions {
            request_id: "cancel".into(),
            at_frame: 0,
            track: None,
            name: "cancel".into(),
        },
        false,
    )
    .unwrap();
    assert_eq!(wait(&j, "cancel").state, "ready");
    assert_eq!(j.cancel("cancel").unwrap().state, "cancelled");
    assert_eq!(j.commit("cancel", &mut e).unwrap().state, "cancelled");
    assert_eq!(e.snapshot(), before);
    j.start(
        Box::new(File::open(fixture("tone-stereo-48000.wav")).unwrap()),
        None,
        ImportOptions {
            request_id: "save-fail".into(),
            at_frame: 0,
            track: None,
            name: "test".into(),
        },
        false,
    )
    .unwrap();
    assert_eq!(wait(&j, "save-fail").state, "ready");
    fs::create_dir(root.path().join("project.json")).unwrap();
    assert_eq!(j.commit("save-fail", &mut e).unwrap().state, "failed");
    assert_eq!(e.snapshot(), before);
    assert_eq!(e.revision(), 0);
    assert!(!e.can_undo());
    assert_eq!(fs::read_dir(root.path().join("assets")).unwrap().count(), 0);
}
#[test]
fn undo_redo_packages_keep_owned_sources_and_missing_caches_rebuild() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let j = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    import(&j, &mut e, "source", "tone-mono-44100.mp3", 0);
    let source = e.snapshot();
    let path = root.path().join(&source.audio_assets[0].path);
    e.undo().unwrap();
    assert!(path.exists());
    assert!(e.project().layers.is_empty());
    e.redo().unwrap();
    assert_eq!(source, e.snapshot());
    let archive = root.path().join("backup.msproj");
    motion_core::storage::export_package(root.path(), e.project(), &archive).unwrap();
    let imported = root.path().join("reopened");
    let p = motion_core::storage::import_package(&archive, &imported).unwrap();
    assert_eq!(p, source);
    assert!(AudioMixer::new(p.clone(), &imported).is_err());
    let r = AudioJobs::new(imported.clone(), Limits::default()).unwrap();
    r.prepare_cache("rebuild", p.audio_assets[0].clone())
        .unwrap();
    assert_eq!(wait(&r, "rebuild").state, "succeeded");
    let mut a = AudioMixer::new(source, root.path()).unwrap();
    let mut b = AudioMixer::new(p, &imported).unwrap();
    let mut x = vec![0.0; 48000 * 2];
    let mut y = x.clone();
    a.mix(123, &mut x).unwrap();
    b.mix(123, &mut y).unwrap();
    assert_eq!(x, y);
    // Resampling is a function of absolute time, not the previous block size.
    let mut chunk = vec![0.0; 12000 * 2];
    b.mix(123, &mut chunk).unwrap();
    assert_eq!(&x[..chunk.len()], &chunk);
    assert_eq!(a.waveform(1, 10, 10).unwrap().len(), 10);
}
#[test]
fn summing_and_clipping_are_deterministic_and_short_blocks_stop_at_composition_end() {
    let _guard = TEST_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let j = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
    let mut e = engine();
    let id = import(&j, &mut e, "src", "tone-stereo-48000.wav", 0);
    e.apply(Command::SetAudio {
        object: id,
        volume: Some(2.0),
        muted: None,
    })
    .unwrap();
    e.apply(Command::Duplicate { object: id }).unwrap();
    let mut mix = AudioMixer::new(e.snapshot(), root.path()).unwrap();
    let mut out = vec![0.0; 48000 * 2];
    mix.mix(0, &mut out).unwrap();
    assert!(out.iter().any(|v| *v == 1.0));
    assert!(out.iter().any(|v| *v == -1.0));
    assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)));
    let count = mix.mix(mix.total_frames() - 17, &mut out).unwrap();
    assert_eq!(count, 17);
    assert!(out[34..].iter().all(|v| *v == 0.0));
    assert!(mix.mix(mix.total_frames() + 1, &mut out).is_err());
    let before = e.snapshot();
    assert!(e
        .apply(Command::SetAudio {
            object: id,
            volume: Some(f32::NAN),
            muted: None
        })
        .is_err());
    assert_eq!(before, e.snapshot());
    assert!(matches!(
        e.project().layers[0].content,
        Content::Audio { .. }
    ));
}
