use aem_core::{
    plugin_editor::{EditorRequest, PluginEditorSession},
    EffectInstance, Engine, Layer, LayerTimeline, Project,
};
use aem_effects::{builtin, Registry};
fn engine() -> Engine {
    let package = builtin::particle_package().unwrap();
    let mut p = Project::new(128, 128, 30, 120).unwrap();
    let mut layer = Layer::solid(1, "Particles", [128., 128.], [64., 64., 0.], [0.; 4]);
    layer.timeline = Some(LayerTimeline {
        in_frame: 30,
        out_frame: 120,
        offset_frame: 30,
    });
    layer.effects.push(EffectInstance::new(
        1,
        &package.manifest.id,
        &package.manifest.version,
        &package.hash,
        &package.manifest.effects[0],
        [128., 128.],
    ));
    p.layers.push(layer);
    p.rebuild_plugin_dependencies();
    Engine::new(p).unwrap()
}
#[test]
fn native_page_keys_use_the_same_tracks_and_cancel_or_commit_once() {
    let mut e = engine();
    let original = e.snapshot();
    let mut session =
        PluginEditorSession::open(e.project(), &Registry::new_with_builtins().unwrap(), 1, 1)
            .unwrap();
    let state = session.state(&e, 30).unwrap();
    assert_eq!(state["timeline_offset"], 30);
    assert_eq!(state["frames"], 120);
    let revision = e.revision();
    session
        .request(&mut e, 30, EditorRequest::Begin { revision })
        .unwrap();
    for frame in [30, 60] {
        let revision = e.revision();
        session
            .request(
                &mut e,
                frame,
                EditorRequest::Key {
                    revision,
                    param: "position".into(),
                },
            )
            .unwrap();
    }
    assert_eq!(
        e.project().layers[0].effects[0].params["position"]
            .track
            .keys
            .iter()
            .map(|k| k.frame)
            .collect::<Vec<_>>(),
        vec![0, 30]
    );
    let revision = e.revision();
    session
        .request(
            &mut e,
            60,
            EditorRequest::Set {
                revision,
                param: "position".into(),
                value: [100., 0., 0., 0.],
            },
        )
        .unwrap();
    assert_eq!(
        e.project().layers[0].effects[0].params["position"].sample(15.)[0],
        50.
    );
    session.close(&mut e, false).unwrap();
    assert_eq!(e.snapshot(), original);
    let revision = e.revision();
    session
        .request(&mut e, 30, EditorRequest::Begin { revision })
        .unwrap();
    let revision = e.revision();
    session
        .request(
            &mut e,
            30,
            EditorRequest::Key {
                revision,
                param: "position".into(),
            },
        )
        .unwrap();
    let revision = e.revision();
    session
        .request(
            &mut e,
            30,
            EditorRequest::Key {
                revision,
                param: "rate".into(),
            },
        )
        .unwrap_err();
    session.close(&mut e, true).unwrap();
    let committed = e.snapshot();
    e.undo().unwrap();
    assert_eq!(e.snapshot(), original);
    e.redo().unwrap();
    assert_eq!(e.snapshot(), committed);
}
