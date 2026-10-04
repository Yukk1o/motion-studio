use aem_core::{
    Asset, Command, Content, Ease, Engine, ObservationView, Observer, Project, Property, Scene,
};
#[test]
fn copied_keys_preserve_values_and_easing_and_are_undoable() {
    let mut e = Engine::new(Project::demo()).unwrap();
    e.apply(Command::Animate {
        object: 2,
        property: Property::Position,
        frame: 0,
        enabled: true,
    })
    .unwrap();
    e.apply(Command::Ease {
        object: 2,
        property: Property::Position,
        frame: 0,
        ease: Ease::InOut,
    })
    .unwrap();
    let before = e.snapshot();
    e.apply(Command::CopyKey {
        object: 2,
        property: Property::Position,
        from: 0,
        to: 60,
    })
    .unwrap();
    let keys = &e.project().layers[1].transform.position.keys;
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].value, keys[1].value);
    assert_eq!(keys[1].ease, Ease::InOut);
    e.undo().unwrap();
    assert_eq!(e.project(), &before);
}
#[test]
fn orthographic_observation_views_stay_independent_and_finite() {
    let p = Project::demo();
    let e = Engine::new(p.clone()).unwrap();
    let mut observer = Observer::new(p.width, p.height);
    let mut scene = Scene::new(&p);
    for view in [ObservationView::Top, ObservationView::Side] {
        observer.view = view;
        observer.pan(100.0, 50.0, p.width, p.height).unwrap();
        scene.sample(&p, 0.0, Some(&observer)).unwrap();
        assert!(scene.camera.view_projection.is_finite());
        let c = scene.camera.view_projection * scene.camera.target.extend(1.0);
        assert!(c.x.abs() < 1e-4 && c.y.abs() < 1e-4);
    }
    assert_eq!(e.project(), &p);
    assert_eq!(e.revision(), 0);
}
#[test]
fn asset_and_content_changes_are_atomic_and_undo_restores_references() {
    let mut e = Engine::new(Project::demo()).unwrap();
    let old = e.snapshot();
    let asset = Asset {
        id: 12,
        path: "assets/test.png".into(),
        width: 2,
        height: 2,
    };
    e.apply_batch(vec![
        Command::RegisterAsset {
            asset: asset.clone(),
        },
        Command::Content {
            object: 2,
            content: Content::Image { asset: 12 },
            size: [2.0, 2.0],
        },
    ])
    .unwrap();
    assert_eq!(e.project().assets.len(), 1);
    e.undo().unwrap();
    assert_eq!(e.project(), &old);
    assert!(e
        .apply_batch(vec![
            Command::RegisterAsset { asset },
            Command::Content {
                object: 2,
                content: Content::Image { asset: 999 },
                size: [2.0, 2.0]
            }
        ])
        .is_err());
    assert_eq!(e.project(), &old);
}
