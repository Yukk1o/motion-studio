//! Projected matte extraction shares the effects-stage snapshots used by image inputs.
use super::*;

fn coverage_order(frame: &crate::effect_plan::EffectFramePlan) -> Result<Vec<(usize, bool)>> {
    fn visit(
        frame: &crate::effect_plan::EffectFramePlan,
        index: usize,
        luma: bool,
        active: &mut std::collections::HashSet<usize>,
        done: &mut std::collections::HashSet<(usize, bool)>,
        out: &mut Vec<(usize, bool)>,
    ) -> Result<()> {
        if done.contains(&(index, luma)) {
            return Ok(());
        }
        if !active.insert(index) {
            return Err(RenderError::Invalid("track matte dependency cycle".into()));
        }
        let c = frame.compositing[index];
        if c.matte >= 0 {
            visit(frame, c.matte as usize, c.mode >= 3, active, done, out)?;
        }
        active.remove(&index);
        done.insert((index, luma));
        out.push((index, luma));
        Ok(())
    }
    let mut active = Default::default();
    let mut done = Default::default();
    let mut out = Vec::new();
    for c in &frame.compositing {
        if c.matte >= 0 {
            visit(
                frame,
                c.matte as usize,
                c.mode >= 3,
                &mut active,
                &mut done,
                &mut out,
            )?;
        }
    }
    Ok(out)
}

impl Renderer {
    pub(super) fn encode_track_mattes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
    ) -> Result<usize> {
        let Some(gpu) = &self.compositing_gpu else {
            return Ok(0);
        };
        let frame = &self.effect_gpu.builder.frame;
        let mut draws = 0;
        for (index, luma) in coverage_order(frame)? {
            let d = &frame.draws[index];
            let c = frame.compositing[index];
            let target = &gpu.mattes[&(d.layer, luma)];
            let parent = if c.matte >= 0 {
                &gpu.mattes[&(frame.draws[c.matte as usize].layer, c.mode >= 3)].composite
            } else {
                &gpu.zero.composite
            };
            let flags = u32::from(c.mode != 0) * 2 + u32::from(c.mode == 2 || c.mode == 4) * 4;
            let u = DrawUniform {
                mvp: std::array::from_fn(|col| std::array::from_fn(|row| d.words[col * 4 + row])),
                color: [1.; 4],
                extent_opacity: [0., 0., d.words[22], flags as f32],
                uv_scale: [1., 1., size[0] as f32, size[1] as f32],
            };
            self.queue.write_buffer(
                &gpu.extract_buffer,
                (index * gpu.extract_stride) as u64,
                bytemuck::bytes_of(&u),
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("projected track matte"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(if luma { &gpu.luma } else { &gpu.alpha });
            pass.set_bind_group(
                0,
                &gpu.extract_group,
                &[(index * gpu.extract_stride) as u32],
            );
            pass.set_bind_group(
                1,
                &self.images[&TextureKey::EffectInput(d.layer)].bind_group,
                &[],
            );
            pass.set_bind_group(2, &gpu.zero.composite, &[]);
            pass.set_bind_group(3, parent, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            for batch in frame.batches.iter().filter(|b| b.layer == index) {
                pass.draw(batch.vertices.clone(), 0..1);
                draws += 1;
            }
        }
        Ok(draws)
    }
}
