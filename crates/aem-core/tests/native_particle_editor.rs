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
fn color_picker_restores_its_track_without_cancelling_other_page_edits() {
    let mut e = engine();
    let original = e.snapshot();
    let mut session = PluginEditorSession::open(e.project(), &Registry::new_with_builtins().unwrap(), 1, 1).unwrap();
    let revision = e.revision();
    session.request(&mut e, 30, EditorRequest::Begin { revision }).unwrap();
    let revision = e.revision();
    session.request(&mut e, 30, EditorRequest::Set { revision, param: "rate".into(), value: [120., 0., 0., 0.] }).unwrap();
    for frame in [30, 60] {
        let revision = e.revision();
        session.request(&mut e, frame, EditorRequest::Key { revision, param: "color".into() }).unwrap();
    }
    let track = e.project().layers[0].effects[0].params["color"].track.clone();
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::ColorBegin { revision, param: "color".into() }).unwrap();
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::Set { revision, param: "color".into(), value: [0.8, 0.2, 0.1, 0.5] }).unwrap();
    assert_eq!(e.project().layers[0].effects[0].params["color"].track.keys.len(), 3);
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::ColorFinish { revision, commit: false }).unwrap();
    assert_eq!(e.project().layers[0].effects[0].params["color"].track, track);
    assert_eq!(e.project().layers[0].effects[0].params["rate"].track.value[0], 120.);
    assert!(session.gesture);
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::ColorBegin { revision, param: "color".into() }).unwrap();
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::Set { revision, param: "color".into(), value: [0.1, 0.4, 0.8, 0.5] }).unwrap();
    let revision = e.revision();
    session.request(&mut e, 45, EditorRequest::ColorFinish { revision, commit: true }).unwrap();
    session.close(&mut e, true).unwrap();
    let committed = e.snapshot();
    e.undo().unwrap();
    assert_eq!(e.snapshot(), original);
    e.redo().unwrap();
    assert_eq!(e.snapshot(), committed);
}

#[test]
fn scoped_color_picker_rejects_stale_other_parameter_and_other_frame_writes() {
    let mut e = engine();
    let mut session = PluginEditorSession::open(e.project(), &Registry::new_with_builtins().unwrap(), 1, 1).unwrap();
    let revision = e.revision();
    assert!(session.request(&mut e, 30, EditorRequest::ColorBegin { revision, param: "color".into() }).is_err());
    session.request(&mut e, 30, EditorRequest::Begin { revision }).unwrap();
    assert!(session.request(&mut e, 30, EditorRequest::ColorBegin { revision, param: "size".into() }).is_err());
    session.request(&mut e, 30, EditorRequest::ColorBegin { revision, param: "color".into() }).unwrap();
    let unchanged = e.snapshot();
    for (frame, param, revision) in [(30, "size", revision), (60, "color", revision), (30, "color", revision.wrapping_sub(1))] {
        assert!(session.request(&mut e, frame, EditorRequest::Set { revision, param: param.into(), value: [1.; 4] }).is_err());
        assert_eq!(e.snapshot(), unchanged);
    }
    session.close(&mut e, false).unwrap();
    assert!(session.state(&e, 30).unwrap()["color_edit"].is_null());
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
