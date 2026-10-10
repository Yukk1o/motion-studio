use motion_core::{Project, VideoAsset, VideoClip, MAX_VIDEO_FRAMES};

fn asset(width: u32, height: u32, fps: f64) -> VideoAsset {
    VideoAsset {
        id: 1,
        path: "assets/original.mp4".into(),
        bytes: 1024,
        mime: "video/avc".into(),
        track: 0,
        width,
        height,
        rotation: 0,
        display_width: width,
        display_height: height,
        video_start_us: 0,
        video_end_us: 1_000_000,
        duration_us: 1_000_000,
        frame_count: 240,
        variable_frame_rate: false,
        nominal_frame_rate: fps,
        color_standard: 1,
        color_range: 2,
        audio_asset: None,
    }
}

#[test]
fn source_limits_allow_uhd_dci_portrait_fractional_and_high_frame_rates() {
    for (w, h, fps) in [
        (3840, 2160, 60.0),
        (4096, 2160, 120.0),
        (2160, 3840, 59.94005994),
        (1920, 1080, 240.0),
        (4096, 512, 60.0),
        (2560, 2560, 60.0),
        (512, 4096, 60.0),
    ] {
        let mut p = Project::new(256, 144, 24, 240).unwrap();
        p.video_assets.push(asset(w, h, fps));
        p.validate().unwrap();
        let saved = serde_json::to_string(&p).unwrap();
        let reopened: Project = serde_json::from_str(&saved).unwrap();
        reopened.validate().unwrap();
        assert_eq!(p.video_assets, reopened.video_assets);
        assert_eq!(reopened.fps, 24);
    }
    for a in [
        asset(4096, 2161, 60.0),
        asset(4097, 1080, 60.0),
        asset(3840, 2160, 240.01),
        asset(16, 16, f64::NAN),
    ] {
        assert!(a.validate().is_err());
    }
    let mut full = asset(3840, 2160, 240.0);
    full.duration_us = 3_600_000_000;
    full.video_end_us = full.duration_us;
    full.frame_count = 864_000;
    full.validate().unwrap();
    full.frame_count = MAX_VIDEO_FRAMES + 1;
    assert!(full.validate().is_err());
}

#[test]
fn composition_rates_do_not_rewrite_the_source_clock() {
    for fps in [1, 24, 25, 30, 50, 60, 90, 120, 144, 240] {
        let p = Project::new(3840, 2160, fps, fps * 2).unwrap();
        let clip = VideoClip::new(1);
        assert_eq!(clip.source_time_us(f64::from(fps) / 2.0, p.fps), 500_000);
        assert_eq!(clip.source_time_us(f64::from(fps) / 4.0, p.fps), 250_000);
    }
    assert!(Project::new(64, 64, 0, 1).is_err());
    assert!(Project::new(64, 64, 241, 1).is_err());
}
