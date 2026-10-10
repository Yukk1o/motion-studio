use motion_core::{Command, Content, Engine, Layer, LayerTimeline, Project, VideoAsset, VideoClip};
use motion_render::Scene;
fn project() -> Project {
    let mut p = Project::new(64, 64, 60, 240).unwrap();
    p.video_assets.push(VideoAsset {
        id: 10,
        path: "assets/video.mp4".into(),
        bytes: 1024,
        mime: "video/avc".into(),
        track: 0,
        width: 64,
        height: 64,
        rotation: 0,
        display_width: 64,
        display_height: 64,
        video_start_us: 200_000,
        video_end_us: 2_000_000,
        duration_us: 2_000_000,
        frame_count: 54,
        variable_frame_rate: false,
        nominal_frame_rate: 30.0,
        color_standard: 1,
        color_range: 2,
        audio_asset: None,
    });
    let mut l = Layer::solid(1, "video", [64.; 2], [32., 32., 0.], [1.; 4]);
    l.content = Content::Video {
        video: VideoClip::new(10),
    };
    l.timeline = Some(LayerTimeline {
        in_frame: 0,
        out_frame: 120,
        offset_frame: 0,
    });
    p.layers.push(l);
    p.validate().unwrap();
    p
}
#[test]
fn video_uses_source_time_and_leading_and_trailing_transparency_through_clip_edits() {
    let mut e = Engine::new(project()).unwrap();
    let mut scene = Scene::new(e.project());
    scene.sample(e.project(), 0., None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(scene.layers.is_empty());
    scene.sample(e.project(), 30.5, None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(
        scene.layers[0].video.as_ref().unwrap().source_time_us,
        508333
    );
    e.apply_batch(vec![
        Command::SplitLayerClip {
            object: 1,
            frame: 30,
        },
        Command::MoveLayerClip {
            object: 2,
            in_frame: 60,
        },
    ])
    .unwrap();
    scene.sample(e.project(), 60.5, None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(
        scene.layers[0].video.as_ref().unwrap().source_time_us,
        508333
    );
    scene.sample(e.project(), 150., None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(scene.layers.is_empty());
    let before = e.snapshot();
    assert!(e
        .apply_batch(vec![Command::TrimLayerClip {
            object: 2,
            in_frame: 60,
            out_frame: 180
        }])
        .is_err());
    assert_eq!(before, e.snapshot());
}
#[test]
fn format_four_migrates_in_memory_and_unknown_future_versions_are_rejected() {
    let mut old = Project::new(64, 64, 60, 240).unwrap();
    old.version = 4;
    let bytes = serde_json::to_string(&old).unwrap();
    let migrated: Project = serde_json::from_str(&bytes).unwrap();
    assert_eq!(migrated.migrate().unwrap().version, 8);
    assert!(old.video_assets.is_empty());
    old.version = 10;
    assert!(old.validate().is_err());
}
