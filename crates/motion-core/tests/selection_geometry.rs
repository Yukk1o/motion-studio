use motion_core::{EffectInstance, Layer, Project};
use motion_render::Scene;
use motion_effects::{builtin, Registry};

fn fixture() -> (Project, Registry) {
    let package = builtin::package().unwrap();
    let definition = package
        .manifest
        .effects
        .iter()
        .find(|e| e.id == "shake")
        .unwrap();
    let mut p = Project::new(320, 240, 30, 120).unwrap();
    p.camera.created = false;
    let mut layer = Layer::solid(1, "Shake", [60., 40.], [160., 120., 0.], [1.; 4]);
    layer.three_d = false;
    let mut effect = EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        definition,
        layer.size,
    );
    effect.params.get_mut("translation").unwrap().track.value = [120., 60., 0., 0.];
    effect.params.get_mut("rotation").unwrap().track.value = [0.; 4];
    effect.params.get_mut("zoom").unwrap().track.value = [0.; 4];
    layer.effects.push(effect);
    p.layers.push(layer);
    p.rebuild_plugin_dependencies();
    (p, Registry::new_with_builtins().unwrap())
}

#[test]
fn selection_and_picking_move_with_shake_without_changing_stored_transforms() {
    let (p, registry) = fixture();
    let stored = p.clone();
    let mut scene = Scene::new(&p);
    scene.sample(&p, 15., None, &motion_core::ExpressionEvaluator).unwrap();
    let layer = &scene.layers[0];
    let polygon = motion_render::selection_geometry::polygon(layer, &scene, &registry).unwrap();
    let center = [
        polygon.iter().map(|v| v[0]).sum::<f32>() / 4.,
        polygon.iter().map(|v| v[1]).sum::<f32>() / 4.,
    ];
    assert!((center[0] - 160.).abs() > 1. || (center[1] - 120.).abs() > 1.);
    let mut picking = scene.clone();
    picking.layers[0] = motion_render::selection_geometry::picking_layer(layer, &scene, &registry);
    assert_eq!(picking.hit_candidates(center).unwrap()[0].id, 1);
    assert!(scene.hit_candidates(center).unwrap().is_empty());
    assert_eq!(p, stored);
    assert_eq!(scene.project_node(1).unwrap()[..2], [160., 120.]);
    let first = polygon;
    scene.sample(&p, 70., None, &motion_core::ExpressionEvaluator).unwrap();
    scene.sample(&p, 15., None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(
        first,
        motion_render::selection_geometry::polygon(&scene.layers[0], &scene, &registry).unwrap()
    );
}

#[test]
fn partial_opacity_contains_original_and_moved_output_and_zero_is_identity() {
    let (mut p, registry) = fixture();
    p.layers[0].effects[0]
        .params
        .get_mut("effect_opacity")
        .unwrap()
        .track
        .value = [50., 0., 0., 0.];
    let mut scene = Scene::new(&p);
    scene.sample(&p, 15., None, &motion_core::ExpressionEvaluator).unwrap();
    let polygon =
        motion_render::selection_geometry::polygon(&scene.layers[0], &scene, &registry).unwrap();
    let min = polygon.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min);
    let max = polygon
        .iter()
        .map(|v| v[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(min <= 130.001 && max >= 189.999 && max - min > 60.);
    p.layers[0].effects[0]
        .params
        .get_mut("effect_opacity")
        .unwrap()
        .track
        .value = [0.; 4];
    scene.sample(&p, 15., None, &motion_core::ExpressionEvaluator).unwrap();
    let polygon =
        motion_render::selection_geometry::polygon(&scene.layers[0], &scene, &registry).unwrap();
    assert!((polygon[0][0] - 130.).abs() < 0.001);
    assert!((polygon[0][1] - 100.).abs() < 0.001);
}
