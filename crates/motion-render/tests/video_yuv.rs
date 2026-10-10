use motion_core::{Layer, Project, Scene};
use motion_render::{ChromaLayout, Renderer, VideoPlane, Yuv420Frame};

fn frame(rotation: u32, standard: u32, range: u32) -> Yuv420Frame {
    Yuv420Frame {
        width: 8,
        height: 6,
        rotation,
        standard,
        range,
        phase: [1, 1],
        chroma_layout: ChromaLayout::Uv,
        y: (0..48).map(|i| (i * 5 + 7) as u8).collect(),
        uv: (0..40).map(|i| (i * 19 + 3) as u8).collect(),
    }
}
fn relayout(mut f: Yuv420Frame, layout: ChromaLayout) -> Yuv420Frame {
    let (cw, _) = f.chroma_size();
    f.uv =
        f.uv.chunks_exact(cw as usize * 2)
            .flat_map(|row| match layout {
                ChromaLayout::Uv => row.to_vec(),
                ChromaLayout::Vu => row.chunks_exact(2).flat_map(|p| [p[1], p[0]]).collect(),
                ChromaLayout::PlanarRows => row
                    .iter()
                    .step_by(2)
                    .chain(row.iter().skip(1).step_by(2))
                    .copied()
                    .collect(),
            })
            .collect();
    f.chroma_layout = layout;
    f
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
    s.layers[0].video = Some(motion_core::VideoSample {
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
                for layout in [ChromaLayout::Uv, ChromaLayout::Vu, ChromaLayout::PlanarRows] {
                    let f = frame(rotation, standard, range);
                    let expected = f.to_rgba().unwrap();
                    let f = relayout(f, layout);
                    assert_eq!(f.to_rgba().unwrap(), expected);
                    let (w, h) = f.display_size();
                    renderer.upload_video_yuv(1, 1, pts, &f).unwrap();
                    let target = renderer.capture_target(w, h).unwrap();
                    let (out, _) = renderer.capture(&scene(w, h), &target).unwrap();
                    assert_eq!(out.len(), expected.len());
                    assert!(
                    out.iter().zip(&expected).all(|(a, b)| a.abs_diff(*b) <= 1),
                    "rotation={rotation}, standard={standard}, range={range}, layout={layout:?}"
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
    }
    assert_eq!(renderer.video_gpu_conversions, 72);
    assert_eq!(renderer.video_upload_bytes, 72 * (48 + 40));
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
        chroma_layout: ChromaLayout::Uv,
        y: vec![16; 4],
        uv: vec![128; 2],
    };
    assert_eq!(black.to_rgba().unwrap(), [0, 0, 0, 255].repeat(4));
}

#[test]
fn native_planar_uv_and_vu_spans_preserve_pixels_without_reordering() {
    let y = [120; 16];
    let u = [10, 20, 30, 40];
    let v = [100, 110, 120, 130];
    let uv = [10, 100, 20, 110, 30, 120, 40, 130];
    let vu = [100, 10, 110, 20, 120, 30, 130, 40];
    let pack = |ud: &[u8], vd: &[u8], row_stride, pixel_stride| {
        Yuv420Frame::pack(
            4,
            4,
            [0, 0],
            0,
            1,
            2,
            [
                VideoPlane {
                    data: &y,
                    row_stride: 4,
                    pixel_stride: 1,
                },
                VideoPlane {
                    data: ud,
                    row_stride,
                    pixel_stride,
                },
                VideoPlane {
                    data: vd,
                    row_stride,
                    pixel_stride,
                },
            ],
        )
        .unwrap()
    };
    let planar = pack(&u, &v, 2, 1);
    assert_eq!(planar.chroma_layout, ChromaLayout::PlanarRows);
    assert_eq!(planar.uv, [10, 20, 100, 110, 30, 40, 120, 130]);
    // Both Android views may end at their final own sample, not at the
    // end of the full interleaved pair. Never read past either view.
    let nv12 = pack(&uv[..7], &uv[1..], 4, 2);
    assert_eq!(nv12.chroma_layout, ChromaLayout::Uv);
    assert_eq!(nv12.uv, uv);
    let nv21 = pack(&vu[1..], &vu[..7], 4, 2);
    assert_eq!(nv21.chroma_layout, ChromaLayout::Vu);
    assert_eq!(nv21.uv, vu);
    assert_eq!(planar.to_rgba().unwrap(), nv12.to_rgba().unwrap());
    assert_eq!(nv12.to_rgba().unwrap(), nv21.to_rgba().unwrap());
}

#[test]
fn separate_chroma_strides_still_pack_exact_samples() {
    let f = Yuv420Frame::pack(
        4,
        4,
        [0, 0],
        0,
        1,
        2,
        [
            VideoPlane {
                data: &[120; 16],
                row_stride: 4,
                pixel_stride: 1,
            },
            VideoPlane {
                data: &[10, 9, 9, 20, 9, 9, 30, 9, 9, 40],
                row_stride: 6,
                pixel_stride: 3,
            },
            VideoPlane {
                data: &[100, 9, 110, 9, 120, 9, 130],
                row_stride: 4,
                pixel_stride: 2,
            },
        ],
    )
    .unwrap();
    assert_eq!(f.chroma_layout, ChromaLayout::Uv);
    assert_eq!(f.uv, [10, 100, 20, 110, 30, 120, 40, 130]);
}
