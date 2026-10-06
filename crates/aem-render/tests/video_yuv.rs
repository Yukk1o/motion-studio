use aem_core::{Layer, Project, Scene};
use aem_render::{Renderer, VideoPlane, Yuv420Frame};

fn frame(rotation: u32, standard: u32, range: u32) -> Yuv420Frame {
    Yuv420Frame {
        width: 8,
        height: 6,
        rotation,
        standard,
        range,
        phase: [1, 1],
        y: (0..48).map(|i| (i * 5 + 7) as u8).collect(),
        uv: (0..40).map(|i| (i * 19 + 3) as u8).collect(),
    }
}
fn scene(width: u32, height: u32) -> Scene {
    let mut p = Project::new(width, height, 60, 120).unwrap();
    let l = Layer::solid(
        1,
        "video",
        [width as f32, height as f32],
        [width as f32 / 2., height as f32 / 2., 0.],
        [1.; 4],
    );
    // Sampling a source uses validated metadata in ordinary projects; this GPU
    // test supplies draw pixels directly to exercise conversion/compositing.
    p.layers.push(l);
    let mut s = Scene::new(&p);
    s.sample(&p, 0., None).unwrap();
    s.layers[0].video = Some(aem_core::VideoSample {
        asset: 1,
        source_time_us: 0,
    });
    s
}
#[test]
fn gpu_yuv_matches_cpu_export_for_rotation_range_matrix_and_odd_crop_phase() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let mut pts = 0;
    for rotation in [0, 90, 180, 270] {
        for standard in [1, 2, 4] {
            for range in [1, 2] {
                let f = frame(rotation, standard, range);
                let (w, h) = f.display_size();
                renderer.upload_video_yuv(1, 1, pts, &f).unwrap();
                let target = renderer.capture_target(w, h).unwrap();
                let (out, _) = renderer.capture(&scene(w, h), &target).unwrap();
                let expected = f.to_rgba().unwrap();
                assert_eq!(out.len(), expected.len());
                assert!(
                    out.iter().zip(&expected).all(|(a, b)| a.abs_diff(*b) <= 1),
                    "rotation={rotation}, standard={standard}, range={range}"
                );
                let bytes = renderer.texture_bytes();
                let uploads = renderer.video_uploads;
                renderer.upload_video_yuv(1, 1, pts, &f).unwrap();
                assert_eq!(bytes, renderer.texture_bytes());
                assert_eq!(uploads, renderer.video_uploads);
                pts += 1;
            }
        }
    }
    assert_eq!(renderer.video_gpu_conversions, 24);
    assert_eq!(renderer.video_upload_bytes, 24 * (48 + 40));
    renderer.clear_assets();
    assert_eq!(renderer.texture_bytes(), 4);
}
#[test]
fn strided_planes_preserve_crop_and_reject_truncated_input() {
    let y = (0..48u8).collect::<Vec<_>>();
    let uv = (100..124u8).collect::<Vec<_>>();
    let make = |data: &[u8]| {
        Yuv420Frame::pack(
            3,
            3,
            [1, 1],
            0,
            1,
            2,
            [
                VideoPlane {
                    data,
                    row_stride: 8,
                    pixel_stride: 1,
                },
                VideoPlane {
                    data: &uv,
                    row_stride: 8,
                    pixel_stride: 2,
                },
                VideoPlane {
                    data: &uv[1..],
                    row_stride: 8,
                    pixel_stride: 2,
                },
            ],
        )
    };
    let f = make(&y).unwrap();
    assert_eq!(f.y, [9, 10, 11, 17, 18, 19, 25, 26, 27]);
    assert_eq!(f.uv, [100, 101, 102, 103, 108, 109, 110, 111]);
    assert_eq!(f.phase, [1, 1]);
    assert!(make(&y[..26]).is_err());
    let black = Yuv420Frame {
        width: 2,
        height: 2,
        rotation: 0,
        standard: 1,
        range: 2,
        phase: [0, 0],
        y: vec![16; 4],
        uv: vec![128; 2],
    };
    assert_eq!(black.to_rgba().unwrap(), [0, 0, 0, 255].repeat(4));
}
