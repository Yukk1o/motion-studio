use motion_core::{AudioAsset, AudioClip, Command, Content, Engine, Layer, Project};
use motion_media::{AudioJobs, AudioMixer, ImportOptions, Limits};
use std::{
    fs::{self, File},
    io::Write,
    time::{Duration, Instant},
};

fn project() -> Project {
    let mut p = Project::demo();
    p.layers.clear();
    p.frames = 180;
    p
}
fn wait(j: &AudioJobs, id: &str) -> motion_media::TaskStatus {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let t = j.status(id).unwrap();
        if t.state != "running" {
            return t;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn portable_formats_preserve_native_rates_samples_waveforms_and_rebuilds() {
    let cases = [
        ("pcm8.wav", 8000),
        ("pcm24.wav", 96000),
        ("pcm32.wav", 192000),
        ("float32.wav", 32000),
        ("float64.wav", 11025),
        ("lossless.flac", 96000),
        ("lossless.m4a", 44100),
        ("vorbis.ogg", 22050),
        ("linear.aiff", 48000),
    ];
    for (name, rate) in cases {
        let root = tempfile::tempdir().unwrap();
        let jobs = AudioJobs::new(root.path().into(), Limits::default()).unwrap();
        let mut e = Engine::new(project()).unwrap();
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/formats")
            .join(name);
        jobs.start(
            Box::new(File::open(source).unwrap()),
            None,
            ImportOptions {
                request_id: "import".into(),
                at_frame: 0,
                track: None,
                name: name.into(),
            },
            false,
        )
        .unwrap();
        let t = wait(&jobs, "import");
        assert_eq!(t.state, "ready", "{name}: {:?}", t.error);
        let asset = jobs.commit("import", &mut e).unwrap().metadata.unwrap();
        assert_eq!(asset.sample_rate, rate, "{name}");
        assert!(
            (asset.duration_us as i64 - 2_000_000).abs() < 40000,
            "{name}: {}",
            asset.duration_us
        );
        let mut mixer = AudioMixer::new(e.snapshot(), root.path()).unwrap();
        let mut output = vec![0.0; 8192];
        mixer.mix(10000, &mut output).unwrap();
        assert!(output.iter().any(|v| v.abs() > 0.1), "{name}");
        let wave = motion_media::read_waveform(root.path(), &asset, 0, 400).unwrap();
        assert_eq!(
            wave.len() as u64,
            (asset.sample_frames * 100).div_ceil(u64::from(rate)),
            "{name}"
        );
        let cache = motion_media::cache_path(root.path(), &asset).unwrap();
        fs::remove_file(cache).unwrap();
        jobs.prepare_cache("rebuild", asset).unwrap();
        assert_eq!(wait(&jobs, "rebuild").state, "succeeded", "{name}");
        let mut reopened = AudioMixer::new(e.snapshot(), root.path()).unwrap();
        let mut other = output.clone();
        reopened.mix(10000, &mut other).unwrap();
        assert_eq!(output, other, "{name}");
    }
}

#[test]
fn downsampling_filters_ultrasonic_aliases_and_is_independent_of_block_boundaries() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("cache/audio-v1")).unwrap();
    let rate = 192000;
    let n = rate as u64;
    let a = AudioAsset {
        id: 1,
        path: "assets/test.wav".into(),
        mime: "audio/wav".into(),
        bytes: 1,
        track: 0,
        sample_rate: rate,
        channels: 1,
        sample_frames: n,
        duration_us: 1_000_000,
    };
    let mut f = File::create(motion_media::cache_path(root.path(), &a).unwrap()).unwrap();
    for i in 0..n {
        let x = (2.0 * std::f64::consts::PI * 32000.0 * i as f64 / f64::from(rate)).sin() as f32;
        f.write_all(&x.to_le_bytes()).unwrap();
    }
    drop(f);
    let mut p = project();
    p.frames = 30;
    p.audio_assets.push(a);
    let mut l = Layer::solid(1, "audio", [0.0; 2], [0.0; 3], [1.0; 4]);
    l.content = Content::Audio {
        audio: AudioClip::new(1),
    };
    p.layers.push(l);
    let mut mixer = AudioMixer::new(p.clone(), root.path()).unwrap();
    let mut all = vec![0.0; 20000];
    mixer.mix(1000, &mut all).unwrap();
    let rms = (all.iter().map(|v| v * v).sum::<f32>() / all.len() as f32).sqrt();
    assert!(rms < 0.015, "alias RMS={rms}");
    let mut split = AudioMixer::new(p, root.path()).unwrap();
    let mut left = vec![0.0; 6666];
    let mut right = vec![0.0; 13334];
    split.mix(1000, &mut left).unwrap();
    split.mix(4333, &mut right).unwrap();
    left.extend(right);
    assert_eq!(all, left);
}

#[test]
fn unsupported_sources_and_cancelled_platform_fallbacks_never_commit() {
    let mkv =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formats/avc.mkv");
    assert!(motion_media::decode_audio(
        &mkv,
        std::path::Path::new("unused.pcm"),
        None,
        1024,
        &mut |_| Ok(())
    )
    .unwrap_err()
    .contains("source clock"));
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::new(project()).unwrap();
    let decoder: motion_media::DecodeAudio = std::sync::Arc::new(|_, _, _, _, check| {
        check(0.0)?;
        Err("unsupported test codec".into())
    });
    let j = AudioJobs::with_decoder(root.path().into(), Limits::default(), decoder).unwrap();
    j.start(
        Box::new(std::io::Cursor::new(vec![1; 16])),
        None,
        ImportOptions {
            request_id: "fail".into(),
            at_frame: 0,
            track: None,
            name: "test".into(),
        },
        false,
    )
    .unwrap();
    assert_eq!(wait(&j, "fail").state, "failed");
    j.commit("fail", &mut e).unwrap();
    assert!(e.project().audio_assets.is_empty());
    assert!(!e.can_undo());
    assert!(e
        .apply(Command::RegisterAudioAsset {
            asset: AudioAsset {
                id: 1,
                path: "assets/a.wav".into(),
                mime: "audio/wav".into(),
                bytes: 16,
                track: 0,
                sample_rate: 0,
                channels: 2,
                sample_frames: 1,
                duration_us: 0
            }
        })
        .is_err());
}
