use aem_core::{Project, Scene};
use aem_render::Renderer;
#[test]
fn an_actual_device_destruction_is_reported_and_a_new_device_can_render() {
    let mut renderer = pollster::block_on(Renderer::headless()).unwrap();
    let p = Project::demo();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 72.0, None).unwrap();
    let target = renderer.capture_target(64, 64).unwrap();
    renderer.draw(&scene, &target.view, 64, 64).unwrap();
    renderer.device.destroy();
    renderer.device.poll(wgpu::Maintain::Wait);
    assert!(renderer.gpu_error().is_some());
    assert!(renderer.draw(&scene, &target.view, 64, 64).is_err());
    drop(target);
    drop(renderer);
    let mut recreated = pollster::block_on(Renderer::headless()).unwrap();
    let target = recreated.capture_target(64, 64).unwrap();
    assert!(recreated
        .capture(&scene, &target)
        .unwrap()
        .0
        .iter()
        .any(|v| *v != 0));
    assert!(recreated.gpu_error().is_none());
}
