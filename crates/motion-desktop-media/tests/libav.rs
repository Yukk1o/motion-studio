#![cfg(feature = "ffmpeg")]
use motion_desktop_media::av::{probe, Decoder};
use motion_host::platform::VideoDecoder;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../motion-media/tests/fixtures/video")
        .join(name)
}

#[test]
fn all_visible_frames_match_reference_pts_and_scrub_in_both_directions() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("manifest.json")).unwrap()).unwrap();
    for name in [
        "silent-24fps.mp4",
        "sound-24fps.mp4",
        "audio-delayed.mp4",
        "variable.mp4",
        "rotated-90.mp4",
    ] {
        let path = fixture(name);
        let probed = probe(&path, None, None, &|| Ok(())).unwrap();
        probed.asset.validate().unwrap();
        let expected: Vec<u64> = manifest[name]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (f["best_effort_timestamp_time"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
                    * 1_000_000.0)
                    .round() as u64
            })
            .collect();
        assert_eq!(
            probed.timestamps, expected,
            "{name} must include B/P frames"
        );
        assert_eq!(
            probed.first_rgba.len(),
            probed.asset.display_width as usize * probed.asset.display_height as usize * 4
        );
        assert_eq!(
            probed.asset.rotation,
            if name == "rotated-90.mp4" { 90 } else { 0 }
        );
        assert_eq!(probed.asset.variable_frame_rate, name == "variable.mp4");
        assert_eq!(
            probed.audio_track.is_some(),
            matches!(name, "sound-24fps.mp4" | "audio-delayed.mp4")
        );
        let mut decoder =
            Decoder::open(&path, probed.asset.clone(), probed.timestamps.clone()).unwrap();
        for index in (0..expected.len()).chain((0..expected.len()).rev()).chain([
            0,
            expected.len() - 1,
            expected.len() / 2,
        ]) {
            let frame = decoder.frame(expected[index], &|| Ok(())).unwrap();
            assert_eq!(frame.pts, expected[index], "{name} frame {index}");
            assert_eq!(
                frame.end,
                expected
                    .get(index + 1)
                    .copied()
                    .unwrap_or(probed.asset.video_end_us)
            );
            assert_eq!(frame.rgba().unwrap().len(), probed.first_rgba.len());
            assert!(matches!(
                frame.pixels,
                motion_host::video_frame::VideoPixels::Yuv(_)
            ));
        }
        assert!(decoder.seeks() > 0);
    }
}

#[test]
fn stream_selection_and_cancel_do_not_silently_fall_back() {
    let path = fixture("sound-24fps.mp4");
    assert!(probe(&path, Some(99), None, &|| Ok(())).is_err());
    assert!(probe(&path, None, Some(99), &|| Ok(())).is_err());
    let probed = probe(&path, None, Some(u32::MAX), &|| Ok(())).unwrap();
    assert_eq!(probed.audio_track, None);
    let mut decoder = Decoder::open(&path, probed.asset, probed.timestamps.clone()).unwrap();
    assert_eq!(
        decoder
            .frame(probed.timestamps[0], &|| Err(
                "video target superseded".into()
            ))
            .err()
            .unwrap(),
        "video target superseded"
    );
    assert_eq!(
        decoder.frame(probed.timestamps[0], &|| Ok(())).unwrap().pts,
        probed.timestamps[0]
    );
}
