use super::*;
use crate::vector_mesh::{RasterCommand, VectorMesh};
use std::collections::HashSet;

struct Target { multisample: wgpu::Texture, resolved: FxTexture }
pub(super) struct GroupGpu {
    targets: HashMap<(u32,u32,usize),Target>,
    uniform: wgpu::BindGroupLayout,
    composite: wgpu::RenderPipeline,
    encode: wgpu::RenderPipeline,
}
impl GroupGpu {
    pub fn new(device:&wgpu::Device,image:&wgpu::BindGroupLayout) -> Self {
        let uniform=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:Some("vector group opacity"), entries:&[wgpu::BindGroupLayoutEntry {
                binding:0,visibility:wgpu::ShaderStages::FRAGMENT,
                ty:wgpu::BindingType::Buffer {ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:wgpu::BufferSize::new(16)},count:None,
            }],
        });
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("vector group copy"),source:wgpu::ShaderSource::Wgsl(include_str!("../group_copy.wgsl").into())});
        let layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:None,bind_group_layouts:&[image,&uniform],push_constant_ranges:&[]});
        let pipeline=|samples,format,blend| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:Some("vector group composite"),layout:Some(&layout),
            vertex:wgpu::VertexState {module:&shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[]},
            fragment:Some(wgpu::FragmentState {module:&shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState {format,blend,write_mask:wgpu::ColorWrites::ALL})]}),
            primitive:Default::default(),depth_stencil:None,multisample:wgpu::MultisampleState {count:samples,..Default::default()},multiview:None,cache:None,
        });
        Self {targets:HashMap::new(),composite:pipeline(4,wgpu::TextureFormat::Rgba8Unorm,Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)),
            encode:pipeline(1,wgpu::TextureFormat::Rgba8UnormSrgb,None),uniform}
    }
    pub fn grouped(mesh:&VectorMesh) -> bool { !mesh.commands.is_empty() || mesh.root_opacity!=1. }
    fn needed(meshes:&[VectorMesh]) -> HashSet<(u32,u32,usize)> {
        meshes.iter().filter(|m|Self::grouped(m)).flat_map(|m|(0..=m.scope_depth()).map(move|d|(m.width,m.height,d))).collect()
    }
    pub fn retain(&mut self,meshes:&[VectorMesh]) { let needed=Self::needed(meshes);self.targets.retain(|k,_|needed.contains(k)); }
    pub fn bytes(&self) -> u64 {self.targets.keys().map(|(w,h,_)|u64::from(*w)*u64::from(*h)*20).sum()}
    pub fn release(&mut self) {self.targets.clear();}
    pub fn required_bytes(meshes:&[VectorMesh]) -> u64 {Self::needed(meshes).iter().map(|(w,h,_)|u64::from(*w)*u64::from(*h)*20).sum()}
    fn opacity(&self,device:&wgpu::Device,alpha:f32) -> wgpu::BindGroup {
        let buffer=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("vector group opacity"),contents:bytemuck::cast_slice(&[alpha,0.,0.,0.]),usage:wgpu::BufferUsages::UNIFORM});
        device.create_bind_group(&wgpu::BindGroupDescriptor {label:None,layout:&self.uniform,entries:&[wgpu::BindGroupEntry {binding:0,resource:buffer.as_entire_binding()}]})
    }
    fn pass<'a>(encoder:&'a mut wgpu::CommandEncoder,target:&'a Target,clear:bool) -> wgpu::RenderPass<'a> {
        // Store samples between paints; resolve at the end of each pass so the
        // enclosing group can sample a completed child at its closing command.
        let view=target.multisample.create_view(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label:Some("ordered vector group paint"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                view:&view,resolve_target:Some(&target.resolved.view),
                ops:wgpu::Operations {load:if clear {wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)}else{wgpu::LoadOp::Load},store:wgpu::StoreOp::Store},
            })],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,
        })
    }
    pub fn draw(&mut self,device:&wgpu::Device,encoder:&mut wgpu::CommandEncoder,image:&wgpu::BindGroupLayout,sampler:&wgpu::Sampler,
        mesh:&VectorMesh,output:&wgpu::TextureView,paint:&wgpu::RenderPipeline,clear:&wgpu::RenderPipeline,clear_vertices:&wgpu::Buffer) -> Result<(),RenderError> {
        for depth in 0..=mesh.scope_depth() {
            self.targets.entry((mesh.width,mesh.height,depth)).or_insert_with(||Target {
                multisample:device.create_texture(&wgpu::TextureDescriptor {label:Some("pooled vector group MSAA"),
                    size:wgpu::Extent3d {width:mesh.width,height:mesh.height,depth_or_array_layers:1},mip_level_count:1,sample_count:4,
                    dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[]}),
                resolved:crate::effect_gpu::texture(device,image,sampler,mesh.width,mesh.height,wgpu::TextureFormat::Rgba8Unorm),
            });
        }
        let vertices=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("ordered vector group triangles"),
            contents:if mesh.vertices.is_empty(){&[0u8;24]}else{bytemuck::cast_slice(&mesh.vertices)},usage:wgpu::BufferUsages::VERTEX});
        let target=|depth|&self.targets[&(mesh.width,mesh.height,depth)];
        {let mut pass=Self::pass(encoder,target(0),true);pass.set_pipeline(clear);pass.set_vertex_buffer(0,clear_vertices.slice(..));pass.draw(0..3,0..1);}
        let mut depth=0;
        for RasterCommand{kind,start,end,opacity} in mesh.commands.iter().copied() {
            match kind {
                0=>{let mut pass=Self::pass(encoder,target(depth),false);pass.set_pipeline(paint);pass.set_vertex_buffer(0,vertices.slice(..));pass.draw(start..end,0..1);}
                1=>{depth+=1;let mut pass=Self::pass(encoder,target(depth),true);pass.set_pipeline(clear);pass.set_vertex_buffer(0,clear_vertices.slice(..));pass.draw(0..3,0..1);}
                2=>{if depth==0{return Err(RenderError::Invalid("unbalanced vector group raster commands".into()));}
                    let group=self.opacity(device,opacity);let mut pass=Self::pass(encoder,target(depth-1),false);
                    pass.set_pipeline(&self.composite);pass.set_bind_group(0,&target(depth).resolved.composite,&[]);pass.set_bind_group(1,&group,&[]);pass.draw(0..3,0..1);depth-=1;}
                _=>return Err(RenderError::Invalid("invalid vector group raster command".into())),
            }
        }
        if depth!=0{return Err(RenderError::Invalid("unclosed vector group raster scope".into()));}
        let group=self.opacity(device,mesh.root_opacity);
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:Some("encode vector root once"),
            color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:output,resolve_target:None,
                ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],
            depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None});
        pass.set_pipeline(&self.encode);pass.set_bind_group(0,&target(0).resolved.composite,&[]);pass.set_bind_group(1,&group,&[]);pass.draw(0..3,0..1);
        Ok(())
    }
}
