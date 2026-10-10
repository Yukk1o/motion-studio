use motion_core::{parse_commands, Command, Content, Engine, Layer, Project};
use motion_render::{PlaneCompositor, Scene};

fn flat_project() -> Project {
    let mut p = Project::new(256, 256, 30, 180).unwrap();
    p.layers.push(Layer::solid(
        1,
        "flat",
        [128.0; 2],
        [128.0, 128.0, 250.0],
        [1.0; 4],
    ));
    p
}
#[test]
fn flat_layers_ignore_camera_and_inactive_spatial_values_until_explicit_activation() {
    let mut p = flat_project();
    assert!(!p.layers[0].three_d);
    p.layers[0].transform.rotation.value = [50.0, 70.0, 10.0];
    let mut s = Scene::new(&p);
    s.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let baseline = s.layers[0].view_projection * s.layers[0].model;
    p.camera.created = true;
    p.camera.position.value = [400.0, 400.0, -200.0];
    p.camera.target.value = [128.0, 128.0, 0.0];
    p.layers[0].transform.position.value[2] = -500.0;
    p.layers[0].transform.rotation.value[0] = 120.0;
    s.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    assert_eq!(baseline, s.layers[0].view_projection * s.layers[0].model);
    let mut e = Engine::new(p).unwrap();
    let tracks = e.project().layers[0].transform.clone();
    e.apply(
        parse_commands(r#"{"op":"set_layer_3d","object":1,"enabled":true}"#)
            .unwrap()
            .remove(0),
    )
    .unwrap();
    assert_eq!(tracks, e.project().layers[0].transform);
    s.sample(e.project(), 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    assert!(!baseline.abs_diff_eq(s.layers[0].view_projection * s.layers[0].model, 1e-4));
    e.undo().unwrap();
    assert!(!e.project().layers[0].three_d);
    e.redo().unwrap();
    assert!(e.project().layers[0].three_d);
    let rev = e.revision();
    e.apply(Command::SetLayer3d {
        object: 1,
        enabled: true,
    })
    .unwrap();
    assert_eq!(rev, e.revision());
    e.apply(Command::SetLayer3d {
        object: 1,
        enabled: false,
    })
    .unwrap();
    assert_eq!(tracks, e.project().layers[0].transform);
}
#[test]
fn explicit_switch_is_atomic_persisted_and_retained_by_clip_operations() {
    let mut e = Engine::new(flat_project()).unwrap();
    e.apply(Command::SetLayer3d {
        object: 1,
        enabled: true,
    })
    .unwrap();
    e.apply(Command::SplitLayerClip {
        object: 1,
        frame: 60,
    })
    .unwrap();
    assert!(e.project().layers.iter().all(|l| l.three_d));
    e.apply(Command::Flags {
        object: 2,
        visible: true,
        locked: true,
    })
    .unwrap();
    let before = e.snapshot();
    let revision = e.revision();
    assert!(e
        .apply_batch(vec![
            Command::SetLayer3d {
                object: 1,
                enabled: false
            },
            Command::SetLayer3d {
                object: 2,
                enabled: false
            }
        ])
        .is_err());
    assert_eq!(e.project(), &before);
    assert_eq!(revision, e.revision());
    assert!(e
        .apply(Command::SetLayer3d {
            object: 0,
            enabled: true
        })
        .is_err());
    let tmp = tempfile::tempdir().unwrap();
    motion_core::storage::save(tmp.path(), e.project()).unwrap();
    assert_eq!(motion_core::storage::load(tmp.path()).unwrap(), before);
}
#[test]
fn old_files_preserve_spatial_rendering_in_memory_without_changing_new_layer_defaults() {
    let mut p = Project::demo();
    p.version = 2;
    let mut raw = serde_json::to_value(&p).unwrap();
    for l in raw["layers"].as_array_mut().unwrap() {
        l.as_object_mut().unwrap().remove("three_d");
    }
    let tmp = tempfile::tempdir().unwrap();
    let bytes = serde_json::to_vec(&raw).unwrap();
    std::fs::write(tmp.path().join("project.json"), &bytes).unwrap();
    let migrated = motion_core::storage::load(tmp.path()).unwrap();
    assert_eq!(migrated.version, 8);
    assert!(migrated.layers.iter().all(|l| l.three_d));
    assert_eq!(
        bytes,
        std::fs::read(tmp.path().join("project.json")).unwrap()
    );
    for f in [0.0, 20.5, 99.75, 179.5] {
        let mut a = Scene::new(&p);
        let mut b = Scene::new(&migrated);
        a.sample(&p, f, None, &motion_core::ExpressionEvaluator).unwrap();
        b.sample(&migrated, f, None, &motion_core::ExpressionEvaluator).unwrap();
        for (a, b) in a.layers.iter().zip(&b.layers) {
            assert_eq!(a.view_projection * a.model, b.view_projection * b.model);
        }
    }
    assert!(!Layer::solid(10, "new", [10.0; 2], [0.0; 3], [1.0; 4]).three_d);
}
#[test]
fn intersections_are_split_and_2d_layers_form_composition_boundaries() {
    let mut p = flat_project();
    p.layers[0].three_d = true;
    p.layers[0].transform.position.value[2] = 0.0;
    p.layers[0].transform.rotation.value[1] = 45.0;
    let mut b = p.layers[0].clone();
    b.id = 2;
    b.transform.rotation.value[1] = -45.0;
    p.layers.push(b);
    let mut scene = Scene::new(&p);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    let mut compositor = PlaneCompositor::new();
    compositor.prepare(&scene).unwrap();
    assert_eq!(compositor.batches.len(), 3);
    assert_eq!(compositor.vertices.len(), 18);
    // Splitting interpolates source UV instead of stretching each fragment.
    assert!(compositor
        .vertices
        .iter()
        .any(|v| (v.uv[0] - 0.5).abs() < 1e-5));
    assert_eq!(scene.hit_candidates([110.0, 128.0]).unwrap()[0].id, 2);
    assert_eq!(scene.hit_candidates([146.0, 128.0]).unwrap()[0].id, 1);
    let mut separator = Layer::solid(3, "2d", [10.0; 2], [50.0, 50.0, 900.0], [1.0; 4]);
    separator.content = Content::Solid { color: [1.0; 4] };
    p.layers.insert(1, separator);
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    compositor.prepare(&scene).unwrap();
    assert_eq!(compositor.batches.len(), 3);
    assert_eq!(
        compositor
            .batches
            .iter()
            .map(|b| scene.layers[b.layer].id)
            .collect::<Vec<_>>(),
        vec![1, 3, 2]
    );
    p.layers[0].transform.scale.value = [0.0; 3];
    scene.sample(&p, 0.0, None, &motion_core::ExpressionEvaluator).unwrap();
    compositor.prepare(&scene).unwrap();
    assert_eq!(compositor.batches.len(), 2);
}
