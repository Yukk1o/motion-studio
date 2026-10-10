use motion_core::{vector::{groups::*,*}, Content, Layer, Project, Track};
use motion_render::{Renderer,Scene};
fn group()->VectorGroup {
    let v=VectorGroup::wrap(VectorContent::shape(ShapeKind::Rectangle),[20.;2]);
    let VectorSource::Group{group}=v.source else{unreachable!()};*group
}
fn project(g:VectorGroup)->Project {
    let mut p=Project::new(64,64,30,60).unwrap();p.version=12;p.background=[0.;4];p.camera.created=false;
    let mut l=Layer::solid(1,"Group",[20.;2],[32.,32.,0.],[1.;4]);
    l.content=Content::Vector{vector:VectorContent{source:VectorSource::Group{group:Box::new(g)},fill:None,stroke:None,trim:None,fill_rule:FillRule::NonZero}};
    p.layers.push(l);p
}
fn capture(r:&mut Renderer,p:&Project,f:f64)->Vec<u8> {
    let mut s=Scene::new(p);s.sample(p,f,None,&motion_core::ExpressionEvaluator).unwrap();
    let target=r.capture_target(64,64).unwrap();r.capture(&s,&target).unwrap().0
}
fn pixel(bytes:&[u8],x:usize,y:usize)->&[u8]{&bytes[(y*64+x)*4..(y*64+x)*4+4]}
fn fill(id:u64,color:[f32;4])->GroupItem {GroupItem::Fill{id,name:"Fill".into(),color:Track::constant(color),fill_rule:FillRule::NonZero,composite:Composite::Below}}
#[test]
fn root_opacity_applies_once_after_overlapping_paints_and_edits_refresh_cache() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut g=group();
    g.items.push(fill(3,[1.,0.,0.,1.]));g.items.push(fill(4,[0.,1.,0.,1.]));g.transform.opacity.value=50.;
    let mut p=project(g);let first=capture(&mut r,&p,0.);assert!(pixel(&first,32,32)[3].abs_diff(128)<=1);
    let Content::Vector{vector}=&mut p.layers[0].content else{unreachable!()};let VectorSource::Group{group}=&mut vector.source else{unreachable!()};group.transform.opacity.value=25.;
    let quarter=capture(&mut r,&p,0.);assert!(pixel(&quarter,32,32)[3].abs_diff(64)<=1);assert_ne!(first,quarter);
    let count=r.texture_bytes();assert_eq!(quarter,capture(&mut r,&p,0.));assert_eq!(count,r.texture_bytes());
}
#[test]
fn nested_opacity_isolates_the_child_and_repeater_copies_keep_independent_alpha() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut child=group();child.id=5;
    if let GroupItem::Geometry{id,..}=&mut child.items[0]{*id=6;}
    child.items.push(fill(7,[1.,0.,0.,1.]));child.items.push(fill(8,[0.,0.,1.,1.]));child.transform.opacity.value=50.;
    let mut root=VectorGroup{id:1,name:"Root".into(),transform:Default::default(),items:vec![GroupItem::Group{group:Box::new(child)}]};
    root.transform.opacity.value=50.;let p=project(root.clone());let first=capture(&mut r,&p,0.);
    assert!(pixel(&first,32,32)[3].abs_diff(64)<=1);
    root.transform.opacity.value=100.;root.items.push(GroupItem::Repeater{id:9,name:"Copies".into(),repeater:Repeater{copies:Track::constant(2.),position:Track::constant([10.,0.]),..Default::default()}});
    let p=project(root);let repeated=capture(&mut r,&p,0.);
    assert!(pixel(&repeated,36,32)[3].abs_diff(191)<=2);assert!(pixel(&repeated,24,32)[3].abs_diff(128)<=1);
    let bytes=r.texture_bytes();r.clear_assets();assert!(r.texture_bytes()<bytes);
}
#[test]
fn two_large_repeaters_rasterize_visible_roi_and_follow_layer_motion() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut g=group();
    for(id,position)in[(3,[1064.,0.]),(4,[0.,844.])] {g.items.push(GroupItem::Repeater{id,name:"Grid".into(),repeater:Repeater{copies:Track::constant(23.),position:Track::constant(position),..Default::default()}});}
    let mut p=project(g);let first=capture(&mut r,&p,0.);assert_eq!(pixel(&first,32,32),[255,255,255,255]);
    let mut s=Scene::new(&p);s.sample(&p,0.,None,&motion_core::ExpressionEvaluator).unwrap();assert!(s.layers[0].size[0]<=66.&&s.layers[0].size[1]<=66.);
    p.layers[0].transform.position.value[0]-=1064.;p.layers[0].transform.position.value[1]-=844.;
    assert_eq!(first,capture(&mut r,&p,0.));
}
#[test]
fn independent_paints_and_group_animation_are_seek_order_independent() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut g=group();g.transform.position.set_animated(0,true).unwrap();g.transform.position.set_at(10,[16.,-8.]).unwrap();
    g.transform.opacity.set_animated(0,true).unwrap();g.transform.opacity.set_at(10,50.).unwrap();
    let p=project(g);let a=capture(&mut r,&p,5.);
    for f in [10.,0.,7.,5.] {let b=capture(&mut r,&p,f);if f==5.{assert_eq!(a,b);}}
}
