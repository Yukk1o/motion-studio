use aem_core::{masks::*, vector::{PathNode,VectorPath}, Command, Engine, Layer, Project, Scene, Track};
fn mask()->LayerMask {LayerMask::new(1,VectorPath{id:1,closed:true,nodes:[[200.,300.],[800.,300.],[800.,1500.],[200.,1500.]].into_iter().enumerate().map(|(i,p)|PathNode{id:i as u64+1,geometry:Track::constant([p[0],p[1],0.,0.,0.,0.])}).collect()})}
fn engine()->Engine {let mut p=Project::new(1080,1920,30,90).unwrap();p.layers.push(Layer::solid(1,"layer",[1080.,1920.],[540.,960.,0.],[1.;4]));Engine::new(p).unwrap()}
#[test]
fn mask_animation_tracks_use_layer_time_and_atomic_history() {
    let mut e=engine();e.apply(Command::Mask{object:1,action:MaskAction::Add{mask:mask()}}).unwrap();
    e.apply(Command::TrimLayerClip{object:1,in_frame:0,out_frame:60}).unwrap();
    e.apply(Command::MoveLayerClip{object:1,in_frame:20}).unwrap();
    let original=e.snapshot();e.begin_gesture().unwrap();
    for (frame,opacity) in [(20,0.),(40,100.)]{e.apply(Command::Mask{object:1,action:MaskAction::Set{mask:1,property:MaskProperty::Opacity,frame,value:vec![opacity],animated:Some(true)}}).unwrap();}
    e.end_gesture(true).unwrap();let edited=e.snapshot();
    let mut s=Scene::new(e.project());s.sample(e.project(),30.,None).unwrap();assert_eq!(s.layers[0].masks[0].opacity,0.5);
    let json=serde_json::to_string(e.project()).unwrap();let restored:Project=serde_json::from_str(&json).unwrap();assert_eq!(restored,edited);restored.validate().unwrap();
    e.undo().unwrap();assert_eq!(*e.project(),original);e.redo().unwrap();assert_eq!(*e.project(),edited);
    let bad=Command::Mask{object:1,action:MaskAction::Set{mask:1,property:MaskProperty::Feather,frame:30,value:vec![-1.,20.],animated:Some(false)}};
    assert!(e.apply(bad).is_err());assert_eq!(*e.project(),edited);
}
#[test]
fn disabled_open_and_none_masks_preserve_source_and_invalid_ids_fail() {
    let mut e=engine();let mut m=mask();m.path.closed=false;e.apply(Command::Mask{object:1,action:MaskAction::Add{mask:m}}).unwrap();
    let mut s=Scene::new(e.project());s.sample(e.project(),0.,None).unwrap();assert!(s.layers[0].masks.is_empty());
    let before=e.snapshot();assert!(e.apply(Command::Mask{object:1,action:MaskAction::Add{mask:mask()}}).is_err());assert_eq!(*e.project(),before);
    assert!(e.apply(Command::Mask{object:1,action:MaskAction::Reorder{masks:vec![1,1]}}).is_err());assert_eq!(*e.project(),before);
    e.apply(Command::Mask{object:1,action:MaskAction::Replace{mask:mask()}}).unwrap();
    e.apply(Command::Mask{object:1,action:MaskAction::Options{mask:1,mode:Some(MaskMode::None),inverted:Some(false),enabled:Some(true)}}).unwrap();
    s.sample(e.project(),0.,None).unwrap();assert!(s.layers[0].masks.is_empty());
}
#[test]
fn incremental_options_and_animation_keep_prior_edits_and_bound_elastic_alpha() {
    let mut e=engine();e.apply(Command::Mask{object:1,action:MaskAction::Add{mask:mask()}}).unwrap();
    let parse=|action:serde_json::Value|serde_json::from_value::<Command>(serde_json::json!({"op":"mask","object":1,"action":action})).unwrap();
    e.apply(parse(serde_json::json!({"kind":"set","mask":1,"property":"opacity","frame":0,"value":[50]}))).unwrap();
    e.apply(parse(serde_json::json!({"kind":"options","mask":1,"inverted":true}))).unwrap();
    assert_eq!(e.project().layers[0].masks[0].opacity.value,50.);assert!(e.project().layers[0].masks[0].inverted);
    e.apply(parse(serde_json::json!({"kind":"animate","mask":1,"property":"opacity","frame":0,"enabled":true}))).unwrap();
    e.apply(parse(serde_json::json!({"kind":"set","mask":1,"property":"opacity","frame":0,"value":[100]}))).unwrap();
    e.apply(parse(serde_json::json!({"kind":"set","mask":1,"property":"opacity","frame":30,"value":[0]}))).unwrap();
    assert_eq!(e.project().layers[0].masks[0].opacity.keys.len(),2);
    let easing:aem_core::Easing=serde_json::from_value(serde_json::json!({"ease":"linear","curve":{"shape":{"kind":"elastic","oscillations":2.5,"damping":6.0}}})).unwrap();
    e.apply(Command::Mask{object:1,action:MaskAction::Curve{mask:1,property:MaskProperty::Opacity,frame:0,easing}}).unwrap();
    let mut s=Scene::new(e.project());for frame in 0..30{s.sample(e.project(),f64::from(frame),None).unwrap();assert!((0. ..=1.).contains(&s.layers[0].masks[0].opacity));}
}
