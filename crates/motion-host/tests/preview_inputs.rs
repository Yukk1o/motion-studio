use motion_core::{Asset, Content, Layer, LayerTimeline, Project, VideoAsset, VideoClip};
use motion_host::{
    preview_inputs::*,
    video_frame::{DecodedFrame, VideoPixels},
    Platform, Result, VideoDecoder, VideoQuery,
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
struct PlatformStub;
impl Platform for PlatformStub {
    fn name(&self) -> &'static str {
        "preview-test"
    }
    fn probe_video(
        &self,
        _: &Path,
        _: Option<u32>,
        _: Option<u32>,
        _: &dyn Fn() -> Result<()>,
    ) -> Result<motion_media::VideoProbe> {
        Err("not used".into())
    }
    fn open_decoder(
        &self,
        _: &Path,
        asset: VideoAsset,
        pts: Vec<u64>,
    ) -> Result<Box<dyn VideoDecoder>> {
        Ok(Box::new(Decoder { asset, pts }))
    }
    fn decode_audio(
        &self,
        _: &Path,
        _: &Path,
        _: Option<u32>,
        _: u64,
        _: &mut dyn FnMut(f64) -> Result<()>,
    ) -> Result<motion_core::AudioAsset> {
        Err("not used".into())
    }
    fn media_capabilities(&self, _: Option<&VideoQuery>) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    fn attach_surface(
        &self,
        _: Arc<dyn wgpu::WindowHandle + Send + Sync>,
        _: u32,
        _: u32,
    ) -> Result<motion_host::SurfaceTarget> {
        Err("not used".into())
    }
}
struct Decoder {
    asset: VideoAsset,
    pts: Vec<u64>,
}
impl VideoDecoder for Decoder {
    fn frame(&mut self, time: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        for _ in 0..12 {
            check()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        let index = self
            .pts
            .partition_point(|pts| *pts <= time)
            .saturating_sub(1);
        Ok(DecodedFrame {
            pixels: VideoPixels::Yuv(motion_render::Yuv420Frame {
                width: 4,
                height: 2,
                rotation: 90,
                standard: 1,
                range: 2,
                phase: [0, 0],
                y: vec![90; 8],
                uv: vec![128; 4],
                chroma_layout: motion_render::ChromaLayout::Uv,
            }),
            pts: self.pts[index],
            end: self
                .pts
                .get(index + 1)
                .copied()
                .unwrap_or(self.asset.video_end_us),
            width: 2,
            height: 4,
            decode_us: 0,
            codec_us: 0,
            transfer_us: 0,
            pack_us: 0,
            source_transfer: "yuv",
            decoder_name: "test".into(),
        })
    }
}
fn ready(reader: &mut PreviewInputReader, sequence: u64, revision: u64) -> PreparedInput {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        match reader.poll(sequence, revision).unwrap() {
            InputPoll::Ready(packet) => return packet,
            InputPoll::Pending(_) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}
fn image_project(root: &Path) -> Project {
    std::fs::create_dir_all(root.join("assets")).unwrap();
    image::RgbaImage::from_pixel(9, 17, image::Rgba([255, 0, 0, 128]))
        .save(root.join("assets/portrait.png"))
        .unwrap();
    let mut project = Project::new(320, 180, 30, 120).unwrap();
    project.assets.push(Asset {
        id: 1,
        path: "assets/portrait.png".into(),
        width: 9,
        height: 17,
    });
    let mut layer = Layer::solid(3, "Portrait", [18., 34.], [160., 90., 0.], [1.; 4]);
    layer.content = Content::Image { asset: 1 };
    layer.visible = false;
    layer.timeline = Some(LayerTimeline {
        in_frame: 15,
        out_frame: 90,
        offset_frame: 15,
    });
    project.layers.push(layer);
    project
}
fn request(sequence: u64, source: InputSource, frame: f64) -> InputRequest {
    InputRequest {
        sequence,
        source,
        frame,
        max_edge: 8,
    }
}
#[test]
fn actual_images_keep_aspect_logical_size_fractional_clock_and_snapshot() {
    let folder = tempfile::tempdir().unwrap();
    let project = image_project(folder.path());
    let before = project.clone();
    let mut reader = PreviewInputReader::new(Arc::new(PlatformStub));
    let info = reader
        .request(
            &project,
            folder.path(),
            7,
            request(
                1,
                InputSource::Layer {
                    composition: "comp-main".into(),
                    object: 3,
                },
                21.5,
            ),
        )
        .unwrap();
    assert_eq!(info.source_size, [9, 17]);
    assert_eq!(info.logical_size, [18., 34.]);
    assert_eq!(info.raster_size, [5, 8]);
    assert_eq!(info.clock.local_frame, 6.5);
    assert!((info.clock.seconds - 6.5 / 30.).abs() < 1e-12);
    assert!(info.active);
    let packet = ready(&mut reader, 1, 7);
    let InputPixels::Image(pixels) = packet.pixels else {
        panic!("actual image expected")
    };
    assert_eq!((pixels.width, pixels.height), (5, 8));
    assert_eq!(pixels.rgba[3], 128);
    assert!(
        pixels.rgba[0] > 180 && pixels.rgba[0] < 195,
        "image input uses renderer's linear-premultiplied sRGB bytes"
    );
    assert_eq!(project, before);
    assert_eq!(input_choices(&project).len(), 2);
}
#[test]
fn cancelled_and_replaced_requests_never_publish_old_pixels_or_revision() {
    let folder = tempfile::tempdir().unwrap();
    let project = image_project(folder.path());
    let mut reader = PreviewInputReader::new(Arc::new(PlatformStub));
    let source = InputSource::ImageAsset { asset: 1 };
    reader
        .request(&project, folder.path(), 1, request(1, source.clone(), 0.))
        .unwrap();
    reader
        .request(&project, folder.path(), 2, request(2, source.clone(), 0.))
        .unwrap();
    assert!(reader.poll(1, 1).is_err());
    assert!(reader.poll(2, 1).is_err());
    ready(&mut reader, 2, 2);
    assert!(reader
        .request(&project, folder.path(), 2, request(2, source.clone(), 1.))
        .is_err());
    reader.cancel();
    assert!(reader.poll(2, 2).is_err());
    assert!(reader
        .request(&project, folder.path(), 2, request(2, source.clone(), 0.))
        .is_err());
    reader
        .request(&project, folder.path(), 2, request(3, source, 0.))
        .unwrap();
    ready(&mut reader, 3, 2);
}
#[test]
fn clip_end_is_transparent_and_changed_owned_image_is_rejected() {
    let folder = tempfile::tempdir().unwrap();
    let project = image_project(folder.path());
    let mut reader = PreviewInputReader::new(Arc::new(PlatformStub));
    reader
        .request(
            &project,
            folder.path(),
            1,
            request(
                1,
                InputSource::Layer {
                    composition: "comp-main".into(),
                    object: 3,
                },
                90.,
            ),
        )
        .unwrap();
    let packet = ready(&mut reader, 1, 1);
    assert!(!packet.info.active);
    assert!(matches!(packet.pixels, InputPixels::Transparent));
    let source = InputSource::ImageAsset { asset: 1 };
    reader
        .request(&project, folder.path(), 1, request(2, source.clone(), 0.))
        .unwrap();
    ready(&mut reader, 2, 1);
    std::fs::write(folder.path().join("assets/portrait.png"), b"changed source").unwrap();
    assert!(reader.poll(2, 1).is_err());
    assert!(reader
        .request(
            &project,
            folder.path(),
            1,
            InputRequest {
                frame: f64::NAN,
                ..request(3, source.clone(), 0.)
            }
        )
        .is_err());
    assert!(reader
        .request(
            &project,
            folder.path(),
            1,
            InputRequest {
                max_edge: 0,
                ..request(3, source, 0.)
            }
        )
        .is_err());
}
#[test]
fn host_entry_points_are_revision_checked_and_do_not_seek_or_edit_session() {
    let folder = tempfile::tempdir().unwrap();
    let project = image_project(folder.path());
    let id = motion_host::session::open(
        folder.path().into(),
        project.clone(),
        Arc::new(PlatformStub),
    )
    .unwrap();
    let result = (|| {
        let choices = motion_host::ops::preview_inputs::choices(id).unwrap();
        assert_eq!(choices["inputs"].as_array().unwrap().len(), 2);
        let revision = choices["revision"].as_u64().unwrap();
        let text =
            serde_json::to_string(&request(1, InputSource::ImageAsset { asset: 1 }, 21.5)).unwrap();
        assert!(motion_host::ops::preview_inputs::request(id, &text, revision + 1).is_err());
        motion_host::ops::preview_inputs::request(id, &text, revision).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline);
            if let InputPoll::Ready(_) = motion_host::ops::preview_inputs::poll(id, 1).unwrap() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        motion_host::session::with_session(id, |session| {
            assert_eq!(session.frame, 0.);
            assert_eq!(session.engine.project(), &project);
            assert!(!session.engine.can_undo());
            session
                .engine
                .apply(motion_core::Command::SetScalar {
                    object: 3,
                    property: motion_core::Property::Opacity,
                    frame: 0,
                    value: 0.5,
                })
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
        assert!(motion_host::ops::preview_inputs::poll(id, 1).is_err());
        motion_host::ops::preview_inputs::cancel(id).unwrap();
    })();
    motion_host::session::close(id);
    result
}
#[test]
fn inactive_image_input_never_opens_missing_source_and_other_composition_clock_is_used() {
    let folder = tempfile::tempdir().unwrap();
    let mut project = image_project(folder.path());
    let created = project
        .edit_composition(motion_core::CompositionAction::Create {
            settings: motion_core::CompositionSettings {
                name: "Child".into(),
                width: 320,
                height: 180,
                fps: 60,
                frames: 120,
                timing: "preserve_seconds".into(),
                shorten: "reject".into(),
            },
        })
        .unwrap();
    let child = created["composition"].as_str().unwrap().to_owned();
    // Existing Project activation API operates on this test snapshot only.
    project.activate_composition(&child).unwrap();
    let mut layer = Layer::solid(9, "Child portrait", [9., 17.], [0.; 3], [1.; 4]);
    layer.content = Content::Image { asset: 1 };
    project.layers.push(layer);
    project.activate_composition("comp-main").unwrap();
    let before = project.clone();
    let mut reader = PreviewInputReader::new(Arc::new(PlatformStub));
    let info = reader
        .request(
            &project,
            folder.path(),
            0,
            request(
                1,
                InputSource::Layer {
                    composition: child,
                    object: 9,
                },
                30.5,
            ),
        )
        .unwrap();
    assert_eq!(info.clock.fps, 60);
    assert!((info.clock.seconds - 30.5 / 60.).abs() < 1e-12);
    ready(&mut reader, 1, 0);
    assert_eq!(project, before);
    std::fs::remove_file(folder.path().join("assets/portrait.png")).unwrap();
    reader
        .request(
            &project,
            folder.path(),
            0,
            request(
                2,
                InputSource::Layer {
                    composition: "comp-main".into(),
                    object: 3,
                },
                120.,
            ),
        )
        .unwrap();
    assert!(matches!(
        ready(&mut reader, 2, 0).pixels,
        InputPixels::Transparent
    ));
}
#[test]
fn video_uses_local_clip_time_vfr_pts_rotation_and_latest_request() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/test.mp4"), b"123").unwrap();
    let mut project = Project::new(320, 180, 30, 120).unwrap();
    let asset = VideoAsset {
        id: 2,
        path: "assets/test.mp4".into(),
        bytes: 3,
        mime: "video/avc".into(),
        track: 0,
        width: 4,
        height: 2,
        rotation: 90,
        display_width: 2,
        display_height: 4,
        video_start_us: 0,
        video_end_us: 800_000,
        duration_us: 800_000,
        frame_count: 4,
        variable_frame_rate: true,
        nominal_frame_rate: 5.,
        color_standard: 1,
        color_range: 2,
        audio_asset: None,
    };
    asset.validate().unwrap();
    let cache = motion_media::video_cache_path(folder.path(), &asset).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    motion_media::save_index(&cache, &asset, &[0, 100_000, 300_000, 500_000]).unwrap();
    project.video_assets.push(asset);
    let mut layer = Layer::solid(8, "Video", [2., 4.], [0.; 3], [1.; 4]);
    layer.content = Content::Video {
        video: VideoClip {
            asset: 2,
            source_offset_us: 100_000,
            volume: 1.,
            muted: false,
        },
    };
    layer.visible = false;
    layer.timeline = Some(LayerTimeline {
        in_frame: 15,
        out_frame: 90,
        offset_frame: 15,
    });
    project.layers.push(layer);
    let before = project.clone();
    let mut reader = PreviewInputReader::new(Arc::new(PlatformStub));
    let source = InputSource::Layer {
        composition: "comp-main".into(),
        object: 8,
    };
    reader
        .request(&project, folder.path(), 9, request(1, source.clone(), 15.5))
        .unwrap();
    let info = reader
        .request(&project, folder.path(), 9, request(2, source, 21.5))
        .unwrap();
    assert!(reader.poll(1, 9).is_err());
    assert_eq!(info.source_size, [2, 4]);
    assert_eq!(info.clock.source_time_us, Some(316_667));
    let packet = ready(&mut reader, 2, 9);
    let InputPixels::Video(frame) = packet.pixels else {
        panic!("native video expected")
    };
    assert_eq!(frame.pts, 300_000);
    assert_eq!(frame.end, 500_000);
    assert!(matches!(frame.pixels, VideoPixels::Yuv(_)));
    assert_eq!(project, before);
}
