use aem_core::{composition::*, vector::{ShapeKind, VectorContent}, *};
use serde_json::json;
use std::sync::Arc;

fn node(n: usize, fps: u32) -> Composition {
    let mut camera = Camera::new(64, 64);
    camera.created = false;
    Composition { id: format!("comp-{n}"), name: format!("Library {n}"),
        width:64, height:64, fps, frames:240, background:[0.;4], camera,
        layers:vec![], expressions:vec![] }
}
fn reference(id: u64, target: &str) -> Layer {
    let mut l = Layer::solid(id, "Reference", [64.;2], [32.,32.,0.], [1.;4]);
    l.content = Content::Composition { clip:CompositionClip::new(target.into()) };
    l
}
fn error_code(error: Error) -> String {
    let Error::Composition(e) = error else { panic!("expected composition error: {error}") };
    e.code
}
fn staggered() -> Project {
    let mut p = Project::new(64,64,24,240).unwrap();
    for n in 1..=96 {
        let mut c = node(n, if n % 2 == 0 {20} else {24});
        c.layers.push(Layer::solid(1,"Color",[32.;2],[32.,32.,0.],[0.2,0.8,0.3,1.]));
        let mut l = reference(n as u64, &c.id);
        l.timeline = Some(LayerTimeline {in_frame:(n as u32-1)*2, out_frame:n as u32*2,
                                         offset_frame:(n as i32-1)*2});
        p.layers.push(l); p.compositions.push(c);
    }
    p
}

#[test]
fn large_staggered_library_saves_reopens_and_samples_only_current_ranges() {
    let p = staggered(); p.validate().unwrap();
    let mut scene = Scene::new(&p);
    for frame in [0.,1.5,3.,87.,190.,0.,87.] {
        scene.sample(&p,frame,None).unwrap();
        assert_eq!(scene.nested.len(),1);
    }
    scene.sample(&p,220.,None).unwrap();
    assert!(scene.nested.is_empty());
    let folder = tempfile::tempdir().unwrap();
    aem_core::storage::save(folder.path(),&p).unwrap();
    assert_eq!(aem_core::storage::load(folder.path()).unwrap(),p);
    let archive = folder.path().join("owned.aem");
    aem_core::storage::export_package(folder.path(),&p,&archive).unwrap();
    assert_eq!(aem_core::storage::import_package(&archive,&folder.path().join("opened")).unwrap(),p);
    let list = p.composition_list(); assert_eq!(list.len(),97);
    let first = list.iter().find(|v|v["id"]=="comp-1").unwrap();
    assert_eq!(first["references"],json!([{"composition":"comp-main","object":1}]));
    assert_eq!(p.reachable_compositions().unwrap().len(),97);
}

#[test]
fn document_and_live_frame_budgets_are_distinct_and_live_failure_is_structured() {
    let mut p = staggered();
    for l in &mut p.layers { l.timeline = None; }
    p.validate().unwrap();
    let mut scene = Scene::new(&p);
    assert_eq!(error_code(scene.sample(&p,0.,None).unwrap_err()),"render_resource_limit");
    for l in &mut p.layers { l.visible=false; }
    scene.sample(&p,0.,None).unwrap(); assert!(scene.nested.is_empty());
    for l in p.layers.iter_mut().take(MAX_RENDER_COMPOSITION_INSTANCES-1) { l.visible=true; }
    scene.sample(&p,0.,None).unwrap();
    assert_eq!(scene.nested.len(),MAX_RENDER_COMPOSITION_INSTANCES-1);
}

#[test]
fn memoized_graph_rejects_hidden_cycles_and_exponential_source_expansion() {
    let mut p = Project::new(64,64,24,240).unwrap();
    for n in 1..=10 {
        let mut c = node(n,24);
        if n<10 { c.layers=vec![reference(1,&format!("comp-{}",n+1)),reference(2,&format!("comp-{}",n+1))]; }
        p.compositions.push(c);
    }
    p.layers.push(reference(1,"comp-1")); p.validate().unwrap(); // 1024 static instances including root.
    p.compositions[9].layers.push(reference(1,"comp-1"));
    assert_eq!(error_code(p.validate().unwrap_err()),"cycle");
    p.compositions[9].layers.clear();
    p.layers.push(reference(2,"comp-1"));
    for l in &mut p.layers { l.visible=false; }
    assert_eq!(error_code(p.validate().unwrap_err()),"resource_limit");
}

#[test]
fn node_limit_is_advertised_and_failed_create_is_atomic() {
    let mut p = Project::new(64,64,24,240).unwrap();
    p.compositions=(1..MAX_COMPOSITIONS).map(|i|node(i,24)).collect();
    let mut engine = Engine::new(p).unwrap();
    let before=engine.snapshot(); let revision=engine.revision();
    let action=CompositionAction::Create { settings:CompositionSettings {
        name:"Extra".into(),width:64,height:64,fps:20,frames:240,
        timing:"preserve_seconds".into(),shorten:"reject".into() } };
    assert_eq!(error_code(engine.apply(Command::Composition {action}).unwrap_err()),"resource_limit");
    assert_eq!(engine.snapshot(),before); assert_eq!(engine.revision(),revision);
}

#[test]
fn deep_active_views_keep_static_vector_caches_and_restore_instance_aliases() {
    let mut p = Project::new(64,64,24,240).unwrap();
    p.layers.push(reference(1,"comp-1"));
    for n in 1..=9 {
        let mut c=node(n,if n%2==0 {20} else {24});
        if n<9 {c.layers.push(reference(1,&format!("comp-{}",n+1)));}
        else {
            let mut l=Layer::solid(1,"Star",[32.;2],[32.,32.,0.],[1.;4]);
            l.content=Content::Vector {vector:VectorContent::shape(ShapeKind::Star)};
            c.layers.push(l);
        }
        p.compositions.push(c);
    }
    p.validate().unwrap();
    fn leaf(s:&Scene)->&Scene { if s.nested.is_empty() {s} else {leaf(&s.nested[0].scene)} }
    let mut s=Scene::new(&p); s.sample(&p,0.,None).unwrap();
    let cached=leaf(&s).layers[0].vector.as_ref().unwrap().clone();
    for frame in [2.,1.,0.,14.5,2.] {
        s.sample(&p,frame,None).unwrap();
        assert!(Arc::ptr_eq(&cached,leaf(&s).layers[0].vector.as_ref().unwrap()));
        assert_eq!(leaf(&s).source_object(leaf(&s).layers[0].id),1);
        assert_eq!(s.nested[0].layer,1);
    }
    p.compositions[8].layers[0].content=Content::Vector {vector:VectorContent::shape(ShapeKind::Triangle)};
    s.sample(&p,2.,None).unwrap();
    assert!(!Arc::ptr_eq(&cached,leaf(&s).layers[0].vector.as_ref().unwrap()));
}

#[test]
fn depth_limit_counts_root_and_is_checked_even_for_unused_nodes() {
    let mut p = Project::new(64,64,24,240).unwrap();
    for n in 1..=MAX_COMPOSITION_DEPTH {
        let mut c=node(n,24);
        if n<MAX_COMPOSITION_DEPTH {c.layers.push(reference(1,&format!("comp-{}",n+1)));}
        p.compositions.push(c);
    }
    p.validate().unwrap(); // Library path has exactly the maximum number of nodes.
    p.layers.push(reference(1,"comp-1"));
    assert_eq!(error_code(p.validate().unwrap_err()),"resource_limit");
}

#[test]
fn deep_mixed_rate_audio_preserves_absolute_samples_without_copying_the_document() {
    let mut p=Project::new(64,64,24,240).unwrap();
    p.audio_assets.push(AudioAsset {id:1,path:"assets/owned.wav".into(),mime:"audio/wav".into(),
        bytes:64,track:0,sample_rate:48000,channels:1,sample_frames:480000,duration_us:10000000});
    p.layers.push(reference(1,"comp-1"));
    for n in 1..=9 {
        let mut c=node(n,if n%2==0 {20} else {24});
        if n<9 {c.layers.push(reference(1,&format!("comp-{}",n+1)));}
        else {
            let mut audio=Layer::solid(1,"Audio",[0.;2],[0.;3],[1.;4]);
            audio.content=Content::Audio {audio:AudioClip::new(1)};
            audio.timeline=Some(LayerTimeline {in_frame:1,out_frame:240,offset_frame:1});
            c.layers.push(audio);
        }
        p.compositions.push(c);
    }
    p.validate().unwrap();
    let voices=p.audio_voices().unwrap(); assert_eq!(voices.len(),1);
    assert_eq!((voices[0].begin_sample,voices[0].end_sample,voices[0].offset_sample),(2000,480000,2000));
}

#[test]
fn borrowed_reference_candidates_match_atomic_edits_in_every_context() {
    let mut p=Project::new(64,64,24,240).unwrap();
    p.layers.push(reference(1,"comp-1"));
    for n in 1..=12 { let mut c=node(n,24); if n<9 {c.layers.push(reference(1,&format!("comp-{}",n+1)));} p.compositions.push(c); }
    p.validate().unwrap();
    for source in [MAIN_COMPOSITION,"comp-1","comp-8","comp-12"] {
        let candidates=p.composition_reference_candidates(source).unwrap();
        for target in p.composition_ids() {
            let mut engine=Engine::new(p.clone()).unwrap();
            let valid=engine.apply(Command::InComposition {composition:source.into(),command:Box::new(
                Command::Composition {action:CompositionAction::Reference {target:target.clone(),at_frame:0}})}).is_ok();
            assert_eq!(candidates.contains(&target),valid,"{source} -> {target}");
        }
    }
}
