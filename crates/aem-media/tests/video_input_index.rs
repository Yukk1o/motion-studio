use aem_core::VideoAsset;
use aem_media::{load_video_index, save_index, video_cache_path};

#[test]
fn full_hour_at_240fps_keeps_every_source_timestamp_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let asset = VideoAsset {
        id: 1,
        path: "assets/high-rate.mp4".into(),
        bytes: 1024,
        mime: "video/avc".into(),
        track: 0,
        width: 3840,
        height: 2160,
        rotation: 0,
        display_width: 3840,
        display_height: 2160,
        video_start_us: 0,
        video_end_us: 3_600_000_000,
        duration_us: 3_600_000_000,
        frame_count: 864_000,
        variable_frame_rate: false,
        nominal_frame_rate: 240.0,
        color_standard: 1,
        color_range: 2,
        audio_asset: None,
    };
    asset.validate().unwrap();
    let pts: Vec<u64> = (0..u64::from(asset.frame_count))
        .map(|n| (n * 1_000_000 + 120) / 240)
        .collect();
    let path = video_cache_path(root.path(), &asset).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_index(&path, &asset, &pts).unwrap();
    assert_eq!(load_video_index(root.path(), &asset).unwrap(), pts);
    // A complete extra entry cannot bypass the asset's declared count/budget.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&asset.video_end_us.to_le_bytes())
        .unwrap();
    assert!(load_video_index(root.path(), &asset).is_err());
}
