use motion_core::*;
use serde_json::json;

fn audio_project(fps: u32, frames: u32) -> Project {
    let mut p = Project::new(64, 64, fps, frames).unwrap();
    p.audio_assets.push(AudioAsset {
        id: 1,
        path: "assets/audio.wav".into(),
        bytes: 64,
        mime: "audio/wav".into(),
        track: 0,
        sample_rate: 48000,
        channels: 1,
        sample_frames: 480000,
        duration_us: 10000000,
    });
    let mut layer = Layer::solid(1, "audio", [0.; 2], [0.; 3], [1.; 4]);
    layer.content = Content::Audio {
        audio: AudioClip::new(1),
    };
    p.layers.push(layer);
    p
}
fn edit(e: &mut Engine, composition: &str, action: serde_json::Value) -> Vec<EditResult> {
    e.apply_batch(
        parse_commands(
            &json!({"op":"composition","composition":composition,"action":action}).to_string(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn voices(p: &Project) -> Vec<(u64, u64, i64)> {
    p.audio_voices()
        .unwrap()
        .iter()
        .map(|v| (v.begin_sample, v.end_sample, v.offset_sample))
        .collect()
}
#[test]
fn arbitrary_integer_fps_retains_every_sample_and_two_level_precompose_timing() {
    for fps in [24, 25, 29, 59, 90, 144, 239] {
        let mut p = audio_project(fps, fps);
        p.layers[0].timeline = Some(LayerTimeline {
            in_frame: 1,
            out_frame: fps,
            offset_frame: 1,
        });
        let expected = vec![(48000 / u64::from(fps), 48000, 48000 / i64::from(fps))];
        assert_eq!(voices(&p), expected, "fps {fps}");
        let mut e = Engine::new(p).unwrap();
        for _ in 0..2 {
            let object = e.project().layers[0].id;
            edit(
                &mut e,
                MAIN_COMPOSITION,
                json!({"kind":"precompose","objects":[object],"name":"nested"}),
            );
            assert_eq!(voices(e.project()), expected, "nested fps {fps}");
        }
    }
}
#[test]
fn settings_and_mixed_fps_source_offsets_use_the_main_composition_range() {
    let mut e = Engine::new(audio_project(59, 118)).unwrap();
    e.apply(Command::Delete { object: 1 }).unwrap();
    let result = edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"create","settings":{"name":"144 fps","width":64,"height":64,"fps":144,"frames":144}}),
    );
    let EditResult::Composition { result } = &result[0] else {
        panic!()
    };
    let id = result["composition"].as_str().unwrap().to_owned();
    e.activate_composition(&id).unwrap();
    let audio = audio_project(144, 144).layers.remove(0);
    e.apply(Command::Add { layer: audio }).unwrap();
    e.activate_composition(MAIN_COMPOSITION).unwrap();
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"reference","target":id,"at_frame":3}),
    );
    let object = e.project().layers[0].id;
    edit(
        &mut e,
        MAIN_COMPOSITION,
        json!({"kind":"set_clip","object":object,"source_start_frame":1,"volume":1,"muted":false}),
    );
    assert_eq!(voices(e.project()), vec![(2440, 50107, 2107)]);
    edit(
        &mut e,
        &id,
        json!({"kind":"settings","settings":{"name":"239 fps","width":64,"height":64,"fps":239,"frames":239,"timing":"preserve_seconds","shorten":"reject"}}),
    );
    assert_eq!(e.project().composition(&id).unwrap().fps, 239);
}
