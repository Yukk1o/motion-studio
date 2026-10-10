use motion_core::{masks::*, vector::{PathNode, VectorPath}, Layer, Project, Track};
use motion_render::Scene;
use motion_render::{effect_plan::PlanBuilder, Renderer};

fn rectangle(id:u64,x:f32,y:f32,w:f32,h:f32)->LayerMask {
    LayerMask::new(id,VectorPath{id:1,closed:true,nodes:[[x,y],[x+w,y],[x+w,y+h],[x,y+h]].into_iter().enumerate().map(|(i,p)|PathNode{id:i as u64+1,geometry:Track::constant([p[0],p[1],0.,0.,0.,0.])}).collect()})
}
fn project()->Project {
    let mut p=Project::new(1080,1920,30,60).unwrap();p.background=[0.;4];
    p.layers.push(Layer::solid(1,"masked",[1080.,1920.],[540.,960.,0.],[1.,0.,0.,1.]));p
}
fn capture(r:&mut Renderer,p:&Project)->Vec<u8> {
    let mut scene=Scene::new(p);scene.sample(p,0.,None, &motion_core::ExpressionEvaluator).unwrap();
    let target=r.capture_target(1080,1920).unwrap();r.capture(&scene,&target).unwrap().0
}
fn alpha(p:&[u8],x:usize,y:usize)->u8 {p[(y*1080+x)*4+3]}

#[test]
fn ordinary_masks_modes_opacity_inversion_and_resources() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut p=project();
    p.layers[0].masks.push(rectangle(1,200.,300.,600.,1200.));
    let out=capture(&mut r,&p);assert_eq!(alpha(&out,400,800),255);assert_eq!(alpha(&out,100,800),0);
    let bytes=r.texture_bytes();assert_eq!(out,capture(&mut r,&p));assert_eq!(bytes,r.texture_bytes());
    p.layers[0].masks[0].inverted=true;
    let out=capture(&mut r,&p);assert_eq!(alpha(&out,400,800),0);assert_eq!(alpha(&out,100,800),255);
    p.layers[0].masks[0].inverted=false;p.layers[0].masks[0].opacity.value=50.;
    let out=capture(&mut r,&p);assert!(alpha(&out,400,800).abs_diff(128)<=1);
    let mut cut=rectangle(2,400.,600.,200.,500.);cut.mode=MaskMode::Subtract;p.layers[0].masks.push(cut);
    let out=capture(&mut r,&p);assert_eq!(alpha(&out,500,800),0);assert!(alpha(&out,300,800).abs_diff(128)<=1);
    p.layers[0].masks.clear();let out=capture(&mut r,&p);assert_eq!(alpha(&out,100,800),255);assert_eq!(r.texture_bytes(),4);
    r.clear_assets();assert_eq!(r.texture_bytes(),4);
}

#[test]
fn feather_expansion_and_masked_input_before_effect() {
    let mut r=pollster::block_on(Renderer::headless()).unwrap();let mut p=project();
    let mut mask=rectangle(1,200.,300.,600.,1200.);mask.feather.value=[40.,0.];p.layers[0].masks.push(mask);
    let out=capture(&mut r,&p);assert!(alpha(&out,198,800)>0);assert!(alpha(&out,202,800)<255);assert_eq!(alpha(&out,400,800),255);
    p.layers[0].masks[0].feather.value=[0.;2];p.layers[0].masks[0].expansion.value=20.;
    let out=capture(&mut r,&p);assert_eq!(alpha(&out,190,800),255);assert_eq!(alpha(&out,170,800),0);
    p.layers[0].masks[0].expansion.value=-20.;
    let out=capture(&mut r,&p);assert_eq!(alpha(&out,210,800),0);assert_eq!(alpha(&out,230,800),255);
    p.layers[0].masks[0].expansion.value=0.;
    let package=motion_effects::builtin::package().unwrap();let def=package.manifest.effects.iter().find(|d|d.id=="gaussian_blur").unwrap();
    let mut effect=motion_core::EffectInstance::new(1,&package.manifest.id,&package.manifest.version,&package.hash,def,[1080.,1920.]);
    effect.params.get_mut("p0001").unwrap().track.value[0]=20.;p.layers[0].effects.push(effect);p.rebuild_plugin_dependencies();
    let out=capture(&mut r,&p);assert!(alpha(&out,196,800)>0,"blur should spread masked source beyond path");assert!(alpha(&out,204,800)<255);
}

#[test]
fn portable_mask_records_keep_cached_geometry_and_utility_program() {
    let mut p=project();p.layers[0].masks.push(rectangle(1,200.,300.,600.,1200.));
    let mut s=Scene::new(&p);s.sample(&p,0.,None, &motion_core::ExpressionEvaluator).unwrap();
    let mut plan=PlanBuilder::new(motion_effects::Registry::new_with_builtins().unwrap()).unwrap();
    let frame=plan.build(&s,&[0],1080,1920,false).unwrap();assert_eq!(frame.masks.len(),1);
    let geometry=frame.masks[0].vertices.clone();let mut bytes=vec![0;frame.buffer_bytes(&s)];frame.write(&s,&mut bytes).unwrap();
    let word=|offset:usize|u32::from_ne_bytes(bytes[offset..offset+4].try_into().unwrap());
    assert_eq!(word(4),motion_render::effect_plan::PLAN_VERSION);assert_eq!(word(116),1);assert_eq!(word(124),64);assert!(word(112)>=128);
    s.sample(&p,20.,None, &motion_core::ExpressionEvaluator).unwrap();let frame=plan.build(&s,&[0],1080,1920,false).unwrap();
    assert!(std::sync::Arc::ptr_eq(&geometry,&frame.masks[0].vertices));
    assert_eq!(plan.programs[2].key,"sdk-mask-source");
}
