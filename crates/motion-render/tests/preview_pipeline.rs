use motion_core::{Layer, Project, Scene};
use motion_render::{GpuTimer, Presenter, Renderer};
#[test]
fn full_resolution_composition_fits_the_surface_and_yields_real_pass_timings() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let mut r = pollster::block_on(Renderer::new_profiled(
        &instance,
        None,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        true,
    ))
    .unwrap();
    let mut p = Project::new(128, 256, 30, 180).unwrap();
    p.background = [0.04, 0.05, 0.08, 1.0];
    p.layers.push(Layer::solid(
        1,
        "composition",
        [128.0, 256.0],
        [64.0, 128.0, 0.0],
        [0.5, 0.8, 0.2, 1.0],
    ));
    let mut scene = Scene::new(&p);
    scene.sample(&p, 72.0, None).unwrap();
    let source = r.render_target(128, 256).unwrap();
    assert_eq!(source.texture_bytes(), 128 * 256 * 4);
    let output = r.capture_target(256, 256).unwrap();
    let presenter = Presenter::new(&r, &source.view, r.target_format);
    let mut timer = GpuTimer::new(&r.device, &r.queue)
        .expect("The validation GPU must support timestamp queries");
    let slot = timer.begin(17).unwrap();
    let mut encoder = r.device.create_command_encoder(&Default::default());
    r.encode(
        &scene,
        &source.view,
        128,
        256,
        &mut encoder,
        Some(timer.writes(slot, 0)),
    )
    .unwrap();
    presenter.encode(
        &mut encoder,
        &output.view,
        Some(timer.writes(slot, 1)),
        Some([64.0, 0.0, 128.0, 256.0]),
        p.background,
    );
    timer.resolve(slot, &mut encoder);
    r.queue.submit(Some(encoder.finish()));
    timer.map(slot);
    r.device.poll(wgpu::Maintain::Wait);
    let timing = timer.collect().into_iter().flatten().next().unwrap();
    assert_eq!(timing.sequence, 17);
    assert!(timing.composition_us > 0.0 && timing.presentation_us > 0.0 && timing.total_us > 0.0);
    let pixels = r.read_target(&output).unwrap();
    for (x, expected) in [
        (10, [10, 13, 20]),
        (128, [128, 204, 51]),
        (240, [10, 13, 20]),
    ] {
        let at = (128 * 256 + x) * 4;
        for c in 0..3 {
            assert!(
                (pixels[at + c] as i16 - expected[c]).abs() <= 1,
                "Letterbox/color encoding mismatch at {x}"
            );
        }
    }
    let slots: Vec<_> = (0..4).map(|seq| timer.begin(seq).unwrap()).collect();
    assert!(timer.begin(99).is_none());
    assert_eq!(timer.skipped, 1);
    assert_eq!(slots.len(), 4);
    timer.cancel_unsubmitted(slots[0]);
    assert_eq!(timer.begin(100), Some(slots[0]));
    assert!(r.gpu_error().is_none());
}
