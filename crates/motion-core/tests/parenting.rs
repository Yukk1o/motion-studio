use motion_core::{Asset, Command, Content, Engine, Layer, Project, Property};
use motion_render::Scene;
fn scene(p: &Project, frame: f64) -> Scene {
    let mut s = Scene::new(p);
    s.sample(p, frame, None, &motion_core::ExpressionEvaluator).unwrap();
    s
}
fn close(a: glam::Mat4, b: glam::Mat4) {
    assert!(
        a.abs_diff_eq(b, 0.005),
        "world transform changed: {a:?} / {b:?}"
    );
}
#[test]
fn cameras_are_created_explicitly_and_legacy_projects_keep_their_camera() {
    let p = Project::new(1080, 1920, 30, 180).unwrap();
    assert!(!p.camera.created);
    let mut e = Engine::new(p).unwrap();
    assert!(e.apply(Command::SetVector {
        object: 0, property: Property::Position, frame: 0, value: [0.0; 3]
    }).is_err());
    e.apply(Command::CreateCamera).unwrap();
    assert!(e.project().camera.created);
    assert!(e.project().camera.position.keys.is_empty());
    e.undo().unwrap();
    assert!(!e.project().camera.created);
    e.redo().unwrap();
    assert!(e.project().camera.created);
    let mut legacy = serde_json::to_value(Project::demo()).unwrap();
    legacy["camera"].as_object_mut().unwrap().remove("created");
    let loaded: Project = serde_json::from_value(legacy).unwrap();
    assert!(loaded.camera.created);
}

#[test]
fn deleting_a_parent_preserves_children_and_roundtrips_the_binding() {
    let mut e = Engine::new(Project::demo()).unwrap();
    for child in [0, 2] {
        e.apply(Command::Parent { object: child, parent: Some(1), frame: 21 }).unwrap();
    }
    e.apply(Command::SetVector {
        object: 1, property: Property::Position, frame: 21, value: [640.0, 800.0, 500.0]
    }).unwrap();
    let before = scene(e.project(), 21.0);
    let json = serde_json::to_string(e.project()).unwrap();
    let restored: Project = serde_json::from_str(&json).unwrap();
    restored.validate().unwrap();
    close(before.world_matrix(2).unwrap(), scene(&restored, 21.0).world_matrix(2).unwrap());
    e.apply(Command::Remove { object: 1, frame: 21 }).unwrap();
    for child in [0, 2] {
        close(before.world_matrix(child).unwrap(), scene(e.project(), 21.0).world_matrix(child).unwrap());
    }
    e.undo().unwrap();
    assert_eq!(e.project().camera.parent.as_ref().unwrap().object, Some(1));
}
#[test]
fn every_entity_type_can_parent_other_entities_without_rewriting_animation() {
    let mut p = Project::demo();
    let mut null = Layer::solid(4, "空对象", [100.0; 2], [500.0, 900.0, 0.0], [0.0; 4]);
    null.content = Content::Null;
    p.layers.push(null);
    p.assets.push(Asset {
        id: 1,
        path: "assets/image.png".into(),
        width: 64,
        height: 64,
    });
    p.layers[1].content = Content::Image { asset: 1 };
    p.layers[2].content = Content::Text {
        text: "标题".into(),
        font: "sans-serif".into(),
        color: [1.0; 4],
        raster_asset: 1,
    };
    let mut e = Engine::new(p).unwrap();
    for (child, parent) in [(2, 1), (3, 2), (4, 3), (0, 4)] {
        let before = scene(e.project(), 21.0);
        e.apply(Command::Parent {
            object: child,
            parent: Some(parent),
            frame: 21,
        })
        .unwrap();
        close(
            before.world_matrix(child).unwrap(),
            scene(e.project(), 21.0).world_matrix(child).unwrap(),
        );
    }
    let before = e.project().clone();
    let camera = scene(&before, 21.0).camera;
    e.apply(Command::SetVector {
        object: 1,
        property: Property::Position,
        frame: 21,
        value: [620.0, 990.0, 500.0],
    })
    .unwrap();
    let after = scene(e.project(), 21.0);
    assert!((after.camera.eye - camera.eye).abs_diff_eq(glam::Vec3::new(80.0, -30.0, 0.0), 0.005));
    assert_eq!(
        after.layers.len(),
        3,
        "Null controls must not enter rendered/exported layers"
    );
    assert!(e.project().camera.position.keys.is_empty());
    assert!(e
        .project()
        .layers
        .iter()
        .all(|l| l.transform.position.keys.is_empty()));
    let held = after.world_matrix(0).unwrap();
    e.apply(Command::Parent {
        object: 0,
        parent: None,
        frame: 21,
    })
    .unwrap();
    close(held, scene(e.project(), 21.0).world_matrix(0).unwrap());
    e.undo().unwrap();
    assert_eq!(e.project().camera.parent.as_ref().unwrap().object, Some(4));
}
#[test]
fn camera_parenting_and_cycles_are_atomic() {
    let mut e = Engine::new(Project::demo()).unwrap();
    e.apply(Command::Parent {
        object: 2,
        parent: Some(0),
        frame: 0,
    })
    .unwrap();
    let snapshot = e.project().clone();
    assert!(e
        .apply(Command::Parent {
            object: 0,
            parent: Some(2),
            frame: 0
        })
        .is_err());
    assert_eq!(e.project(), &snapshot);
    assert!(e
        .apply(Command::Parent {
            object: 2,
            parent: Some(2),
            frame: 0
        })
        .is_err());
    assert_eq!(e.project(), &snapshot);
    assert!(e
        .apply(Command::Parent {
            object: 2,
            parent: Some(999),
            frame: 0
        })
        .is_err());
    assert_eq!(e.project(), &snapshot);
    e.apply(Command::SetVector {
        object: 0,
        property: Property::Position,
        frame: 0,
        value: [600.0, 960.0, -2300.0],
    })
    .unwrap();
    assert_ne!(
        scene(e.project(), 0.0).world_matrix(2),
        scene(&snapshot, 0.0).world_matrix(2)
    );
}
