use crate::{camera::to_world, ensure, CameraPose, Content, Layer, Observer, Project, Result};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct HitCandidate {
    pub id: u64,
    pub uv: [f32; 2],
    pub depth: f32,
    pub order: usize,
    pub three_d: bool,
}

#[derive(Clone, Debug)]
pub struct DrawLayer {
    pub id: u64,
    pub model: Mat4,
    pub size: [f32; 2],
    pub source_size: [f32; 2],
    pub source_rect: [f32; 4],
    pub color: [f32; 4],
    pub opacity: f32,
    pub asset: Option<u64>,
    pub video: Option<crate::VideoSample>,
    pub vector: Option<std::sync::Arc<crate::vector::SampledVector>>,
    pub masks: std::sync::Arc<Vec<crate::masks::SampledMask>>,
    pub adjustment: bool,
    pub composition: bool,
    pub depth: f32,
    pub order: usize,
    pub three_d: bool,
    /// 2D layers use a fixed composition projection; 3D layers use the camera.
    pub view_projection: Mat4,
}
#[derive(Clone, Debug)]
pub struct Scene {
    pub camera: CameraPose,
    pub layers: Vec<DrawLayer>,
    pub background: [f32; 4],
    pub width: u32,
    pub height: u32,
    pub effects: Vec<crate::SampledEffect>,
    pub curve_luts: Vec<crate::CurveLut>,
    pub frame: f64,
    pub fps: u32,
    pub sprite_assets: std::collections::HashMap<u64, [u32;2]>,
    pub composition_id: String,
    pub nested: Vec<NestedScene>,
    nested_spare: Vec<NestedScene>,
    node_world: Vec<Mat4>,
    node_states: Vec<u8>,
    node_ids: Vec<u64>,
    node_spatial: Vec<bool>,
    source_objects:Vec<(u64,u64)>,
    evaluated_project: Option<Project>,
    vector_cache: std::collections::HashMap<
        u64,
        (
            crate::vector::VectorContent,
            [f32; 2],
            Option<f64>,
            std::sync::Arc<crate::vector::SampledVector>,
        ),
    >,
    mask_cache: std::collections::HashMap<u64, (Vec<crate::masks::LayerMask>, Option<f64>, std::sync::Arc<Vec<crate::masks::SampledMask>>)>,
}
#[derive(Clone, Debug)]
pub struct NestedScene {
    pub layer: u64,
    pub composition: String,
    pub scene: Box<Scene>,
}
impl Scene {
    pub fn new(project: &Project) -> Self {
        Self {
            camera: project.camera.pose(0.0, project.width, project.height),
            layers: Vec::with_capacity(crate::MAX_LAYERS),
            background: project.background,
            width: project.width,
            height: project.height,
            effects: Vec::with_capacity(crate::MAX_LAYERS * aem_effects::MAX_EFFECTS_PER_LAYER),
            curve_luts: Vec::new(),
            frame: 0.0,
            fps: project.fps,
            sprite_assets: Default::default(),
            composition_id: project.composition_id.clone(),
            nested: Vec::new(),
            nested_spare: Vec::new(),
            node_world: Vec::with_capacity(crate::MAX_LAYERS + 1),
            node_states: Vec::with_capacity(crate::MAX_LAYERS + 1),
            node_ids: Vec::with_capacity(crate::MAX_LAYERS + 1),
            node_spatial: Vec::with_capacity(crate::MAX_LAYERS + 1),
            source_objects:Vec::new(),
            evaluated_project: None,
            vector_cache: Default::default(),
            mask_cache: Default::default(),
        }
    }
    pub fn sample(
        &mut self,
        project: &Project,
        frame: f64,
        observer: Option<&Observer>,
    ) -> Result<()> {
        self.sample_inner(project, project, frame, observer, 0, &mut 0)?;
        if !self.nested.is_empty() {
            let mut used: std::collections::HashSet<_> = project.layers.iter().map(|l|l.id)
                .chain(project.compositions.iter().flat_map(|c|c.layers.iter().map(|l|l.id))).collect();
            let mut next=1u64<<40;
            for node in &mut self.nested { node.scene.assign_instance_ids(&mut used,&mut next); }
        }
        Ok(())
    }
    fn sample_inner(&mut self, document:&Project, project:&Project, frame:f64, observer:Option<&Observer>, depth:usize, instances:&mut usize)->Result<()> {
        ensure(depth<crate::composition::MAX_COMPOSITION_DEPTH,"composition nesting too deep")?;
        if *instances >= crate::composition::MAX_RENDER_COMPOSITION_INSTANCES {
            return crate::composition::fail(&project.composition_id, "render_resource_limit",
                "Active composition instance limit exceeded",
                serde_json::json!({"max_render_instances":crate::composition::MAX_RENDER_COMPOSITION_INSTANCES,
                                   "requested_instances":*instances+1,"frame":frame}));
        }
        *instances += 1;
        ensure(
            frame.is_finite() && frame >= 0.0 && frame < f64::from(project.frames),
            "invalid sample time",
        )?;
        let original = project;
        let evaluated = project.evaluated_at(frame)?;
        let project = evaluated.as_ref();
        self.frame = frame;
        self.fps = project.fps;
        self.sprite_assets.clear();
        self.sprite_assets.extend(project.assets.iter().map(|a| (a.id,[a.width,a.height])));
        self.composition_id.clone_from(&project.composition_id);
        let mut previous_nested = std::mem::take(&mut self.nested);
        for node in &mut previous_nested { node.layer = self.source_object(node.layer); }
        // Alias IDs belong to the last rendered instance, not to authored tracks.
        for effect in &mut self.effects {
            effect.layer = self.source_objects.iter().find(|v| v.0 == effect.layer).map_or(effect.layer, |v| v.1);
        }
        self.nested = std::mem::take(&mut self.nested_spare);
        self.source_objects.clear();
        self.curve_luts.clear();
        let mut effect_index = 0;
        for layer in &project.layers {
            for e in &layer.effects {
                if effect_index >= self.effects.len() {
                    self.effects.push(crate::SampledEffect::new(layer.id, e));
                } else if !self.effects[effect_index].matches(layer.id, e) {
                    self.effects[effect_index] = crate::SampledEffect::new(layer.id, e);
                }
                let sampled = &mut self.effects[effect_index];
                sampled.local_frame = layer.local_frame(frame);
                sampled.enabled = e.enabled;
                sampled.seed = e.seed;
                sampled.scene = e.scene.clone();
                sampled.lut = None;
                sampled.particle_history_error = None;
                if e.enabled && e.scene.as_ref().is_some_and(|s| s.particle_space.is_some()) {
                    let raw_layer = original.layers.iter().find(|l| l.id == layer.id).unwrap();
                    let raw_effect = raw_layer.effects.iter().find(|v| v.id == e.id).unwrap();
                    match crate::particle_history::ParticleHistory::capture(original, raw_layer, raw_effect, sampled.particle_history.as_ref()) {
                        Ok(history) => sampled.particle_history = Some(history),
                        Err(error) => { sampled.particle_history = None; sampled.particle_history_error = Some(error.to_string()); }
                    }
                } else {
                    sampled.particle_history = None;
                }
                for (i, p) in e.params.values().enumerate() {
                    sampled.values[i] = p.sample(sampled.local_frame);
                    if let Some(c) = p.curve.as_ref().filter(|_| e.enabled) {
                        sampled.lut = Some(self.curve_luts.len());
                        self.curve_luts.push(c.sample(sampled.local_frame));
                    }
                }
                effect_index += 1;
            }
        }
        self.effects.truncate(effect_index);
        crate::hierarchy::matrices(project, frame, &mut self.node_world, &mut self.node_states)?;
        self.node_ids.clear();
        self.node_ids.extend(project.layers.iter().map(|l| l.id));
        self.node_ids.push(0);
        self.node_spatial.clear();
        self.node_spatial
            .extend(project.layers.iter().map(|l| l.three_d));
        self.node_spatial.push(true);
        let link = project.camera.parent.as_ref();
        let prefix = if let Some(link) = link {
            let parent = link.object.map_or(Mat4::IDENTITY, |id| {
                self.node_world[self.node_ids.iter().position(|v| *v == id).unwrap()]
            });
            parent * Mat4::from_cols_array_2d(&link.bind)
        } else {
            Mat4::IDENTITY
        };
        let implicit;
        let camera = if project.camera.created {
            &project.camera
        } else {
            implicit = crate::Camera::new(project.width, project.height);
            &implicit
        };
        self.camera = observer.map_or_else(
            || camera.pose_parented(frame, project.width, project.height, prefix),
            |o| o.pose(project.width, project.height),
        );
        self.width = project.width;
        self.height = project.height;
        self.background = project.background;
        self.layers.clear();
        self.mask_cache.retain(|id,_| project.layers.iter().any(|l| l.id == *id));
        self.vector_cache.retain(|id, _| {
            project
                .layers
                .iter()
                .any(|l| l.id == *id && matches!(l.content, Content::Vector { .. }))
        });
        let forward = (self.camera.target - self.camera.eye).normalize();
        let flat_projection = Mat4::orthographic_rh(
            -(project.width as f32) * 0.5,
            project.width as f32 * 0.5,
            -(project.height as f32) * 0.5,
            project.height as f32 * 0.5,
            -1.0,
            1.0,
        );
        for (order, layer) in project.layers.iter().enumerate() {
            if matches!(layer.content, Content::Null | Content::Audio { .. }) {
                continue;
            }
            if !layer.active(frame, project.frames) {
                continue;
            }
            let opacity = layer
                .transform
                .opacity
                .sample(layer.local_frame(frame))
                .clamp(0.0, 1.0);
            if opacity <= 0.0 {
                continue;
            }
            let center = self.node_world[order].w_axis.truncate();
            if let Content::Composition { clip } = &layer.content {
                let child=document.composition_frame_view(&clip.composition)?;
                let source=clip.source_frame(layer.local_frame(frame),project.fps,child.fps);
                if source<0.||source>=f64::from(child.frames) {continue;}
                let mut node = if let Some(index) = previous_nested.iter().position(|n| n.layer == layer.id && n.composition == clip.composition) {
                    previous_nested.swap_remove(index)
                } else {
                    NestedScene {layer:layer.id, composition:clip.composition.clone(), scene:Box::new(Scene::new(&child))}
                };
                if let Err(error)=node.scene.sample_inner(document,&child,source,None,depth+1,instances){
                    if matches!(&error, crate::Error::Composition(e) if e.code == "render_resource_limit") { return Err(error); }
                    return crate::composition::fail(&clip.composition,"sample_failed",&error.to_string(),serde_json::json!({"frame":source,"parent_composition":project.composition_id,"reference":layer.id}));
                }
                self.nested.push(node);
            }
            let video = if let Content::Video { video } = &layer.content {
                let a = project
                    .video_assets
                    .iter()
                    .find(|a| a.id == video.asset)
                    .unwrap();
                let time = video.source_time_us(layer.local_frame(frame), project.fps);
                if time < a.video_start_us as i64 || time >= a.video_end_us as i64 {
                    continue;
                }
                Some(crate::VideoSample {
                    asset: a.id,
                    source_time_us: time as u64,
                })
            } else {
                None
            };
            let (color, asset) = match &layer.content {
                Content::Null | Content::Audio { .. } => unreachable!(),
                Content::Solid { color } => (*color, None),
                Content::Video { .. } => ([1.0; 4], None),
                Content::Vector { .. } | Content::Adjustment => ([1.0; 4], None),
                Content::Composition { .. } => ([1.0;4],None),
                Content::Image { asset } => ([1.0; 4], Some(*asset)),
                Content::Text {
                    color,
                    raster_asset,
                    ..
                } => (*color, Some(*raster_asset)),
            };
            let vector = if let Content::Vector { vector } = &layer.content {
                let time = vector.animated().then_some(layer.local_frame(frame));
                let dirty = self
                    .vector_cache
                    .get(&layer.id)
                    .is_none_or(|(v, size, t, _)| v != vector || *size != layer.size || *t != time);
                if dirty {
                    self.vector_cache.insert(
                        layer.id,
                        (
                            vector.clone(),
                            layer.size,
                            time,
                            std::sync::Arc::new(
                                vector
                                    .sample(layer.local_frame(frame), layer.size)
                                    .map_err(|e| {
                                        crate::Error::Invalid(format!(
                                            "layer {} vector: {e}",
                                            layer.id
                                        ))
                                    })?,
                            ),
                        ),
                    );
                }
                Some(self.vector_cache[&layer.id].3.clone())
            } else {
                None
            };
            let mut size = layer.size;
            let mask_time = layer.masks.iter().any(|m| m.animated()).then_some(layer.local_frame(frame));
            if self.mask_cache.get(&layer.id).is_none_or(|(m,t,_)| m != &layer.masks || *t != mask_time) {
                let masks = layer.masks.iter().map(|m| m.sample(layer.local_frame(frame)))
                    .collect::<Result<Vec<_>>>().map_err(|e| crate::Error::Invalid(format!("layer {} masks at frame {frame}: {e}",layer.id)))?
                    .into_iter().flatten().collect();
                self.mask_cache.insert(layer.id,(layer.masks.clone(),mask_time,std::sync::Arc::new(masks)));
            }
            let masks = self.mask_cache[&layer.id].2.clone();
            if let Some(v) = &vector {
                let pad = v.stroke.map_or(0., |s| {
                    s.1 * 0.5
                        * if s.3 == crate::vector::LineJoin::Miter {
                            s.4
                        } else {
                            1.
                        }
                });
                for n in v.paths.iter().flat_map(|p| &p.nodes) {
                    for axis in 0..2 {
                        let extent = n[axis]
                            .abs()
                            .max((n[axis] + n[axis + 2]).abs())
                            .max((n[axis] + n[axis + 4]).abs());
                        size[axis] = size[axis].max(2. * (extent + pad + 1.));
                    }
                }
            }
            self.layers.push(DrawLayer {
                id: layer.id,
                model: self.node_world[order] * geometry_offset(layer),
                size,
                source_size: layer.size,
                source_rect: [
                    (layer.size[0] - size[0]) * 0.5,
                    (layer.size[1] - size[1]) * 0.5,
                    size[0],
                    size[1],
                ],
                color,
                opacity,
                asset,
                video,
                vector,
                masks,
                adjustment: matches!(layer.content, Content::Adjustment),
                composition: matches!(layer.content,Content::Composition {..}),
                depth: (center - self.camera.eye).dot(forward),
                order,
                three_d: layer.three_d,
                view_projection: if layer.three_d {
                    self.camera.view_projection
                } else {
                    flat_projection
                },
            });
        }
        // A visible 2D layer is a compositing boundary between spatial groups.
        let mut start = 0;
        while start < self.layers.len() {
            if !self.layers[start].three_d {
                start += 1;
                continue;
            }
            let end = self.layers[start..]
                .iter()
                .position(|l| !l.three_d)
                .map_or(self.layers.len(), |n| start + n);
            self.layers[start..end]
                .sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth).then(a.order.cmp(&b.order)));
            start = end;
        }
        previous_nested.clear();
        self.nested_spare = previous_nested;
        self.evaluated_project = match evaluated {
            std::borrow::Cow::Owned(p) => Some(p),
            std::borrow::Cow::Borrowed(_) => None,
        };
        Ok(())
    }
    fn assign_instance_ids(&mut self, used:&mut std::collections::HashSet<u64>, next:&mut u64) {
        let mut mapping=std::collections::HashMap::new();
        for id in &mut self.node_ids { if *id!=0 {
            while used.contains(next) {*next+=1;}
            let target=*next;*next+=1;used.insert(target);mapping.insert(*id,target);*id=target;
        }}
        self.source_objects.extend(mapping.iter().map(|(source,alias)|(*alias,*source)));
        for l in &mut self.layers {l.id=mapping[&l.id];}
        for e in &mut self.effects {
            e.layer=mapping[&e.layer];
            if let Some(source)=e.scene.as_mut().and_then(|s|s.source_layer.as_mut()) { *source=mapping[source]; }
        }
        for n in &mut self.nested {n.layer=mapping[&n.layer];n.scene.assign_instance_ids(used,next);}
    }
    pub fn video_layers(&self) -> Vec<&DrawLayer> {
        let mut layers:Vec<_>=self.layers.iter().filter(|l|l.video.is_some()).collect();
        for node in &self.nested {layers.extend(node.scene.video_layers());} layers
    }
    pub fn source_object(&self,id:u64)->u64 {self.source_objects.iter().find(|v|v.0==id).map_or(id,|v|v.1)}
    pub fn diagnostic(&self,message:&str)->String {
        let mut text=message.to_string();for (alias,source) in &self.source_objects{text=text.replace(&format!("layer {alias},"),&format!("layer {source},"));}
        format!("composition {}: {text}",self.composition_id)
    }
    /// Computed numeric values for UI snapshots; original project tracks stay intact.
    pub fn sampled_project<'a>(&'a self, original: &'a Project) -> &'a Project {
        self.evaluated_project.as_ref().unwrap_or(original)
    }
    pub fn node_position(&self, id: u64) -> Option<[f32; 3]> {
        self.node_ids.iter().position(|v| *v == id).map(|i| {
            crate::to_project(
                self.node_world[i].w_axis.truncate(),
                self.width,
                self.height,
            )
        })
    }
    pub fn world_matrix(&self, id: u64) -> Option<Mat4> {
        self.node_ids
            .iter()
            .position(|v| *v == id)
            .map(|i| self.node_world[i])
    }
    pub fn project_point(&self, point: [f32; 3]) -> [f32; 3] {
        let clip =
            self.camera.view_projection * to_world(point, self.width, self.height).extend(1.0);
        let ndc = clip.truncate() / clip.w;
        [
            (ndc.x * 0.5 + 0.5) * self.width as f32,
            (0.5 - ndc.y * 0.5) * self.height as f32,
            ndc.z,
        ]
    }
    pub fn project_node(&self, id: u64) -> Option<[f32; 3]> {
        let point = self.node_position(id)?;
        let spatial = self.node_is_spatial(id)?;
        Some(if spatial {
            self.project_point(point)
        } else {
            [point[0], point[1], 0.5]
        })
    }
    pub fn node_is_spatial(&self, id: u64) -> Option<bool> {
        self.node_ids
            .iter()
            .position(|v| *v == id)
            .map(|i| self.node_spatial[i])
    }
    /// Bounds picking ordered at this exact pixel, rather than by layer centre.
    /// Texture alpha is deliberately not read back; transparent bounds remain selectable.
    pub fn hit_candidates(&self, point: [f32; 2]) -> Result<Vec<HitCandidate>> {
        ensure(
            point.into_iter().all(f32::is_finite),
            "invalid hit-test point",
        )?;
        let x = point[0] / self.width as f32 * 2.0 - 1.0;
        let y = 1.0 - point[1] / self.height as f32 * 2.0;
        let mut hits = Vec::new();
        let mut start = 0;
        while start < self.layers.len() {
            let end = if self.layers[start].three_d {
                self.layers[start..]
                    .iter()
                    .position(|l| !l.three_d)
                    .map_or(self.layers.len(), |n| start + n)
            } else {
                start + 1
            };
            let group_start = hits.len();
            for l in &self.layers[start..end] {
                let mvp = (l.view_projection * l.model).as_dmat4();
                if mvp.determinant().abs() < 1e-20 {
                    continue;
                }
                let inverse = mvp.inverse();
                let a = inverse * glam::DVec4::new(f64::from(x), f64::from(y), 0.0, 1.0);
                let b = inverse * glam::DVec4::new(f64::from(x), f64::from(y), 1.0, 1.0);
                if a.w.abs() < 1e-12 || b.w.abs() < 1e-12 {
                    continue;
                }
                let a = a.truncate() / a.w;
                let b = b.truncate() / b.w;
                let direction = b - a;
                if direction.z.abs() < 1e-12 {
                    continue;
                }
                let p = a + direction * (-a.z / direction.z);
                let clip = mvp * p.extend(1.0);
                if clip.w <= 0.0 || !clip.is_finite() {
                    continue;
                }
                let depth = clip.z / clip.w;
                let uv = [
                    (p.x / f64::from(l.size[0]) + 0.5) as f32,
                    (0.5 - p.y / f64::from(l.size[1])) as f32,
                ];
                if (0.0..=1.0).contains(&depth) && uv.into_iter().all(|v| (0.0..=1.0).contains(&v))
                {
                    hits.push(HitCandidate {
                        id: l.id,
                        uv,
                        depth: depth as f32,
                        order: l.order,
                        three_d: l.three_d,
                    });
                }
            }
            // Construct back-to-front here, reverse once after all 2D boundaries.
            hits[group_start..]
                .sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth).then(a.order.cmp(&b.order)));
            start = end;
        }
        hits.reverse();
        Ok(hits)
    }
    /// Convert a preview drag to translation in the camera's view plane.
    /// Constant clip depth keeps front-facing planes at the same apparent scale.
    /// The UI's currently selected property never participates in this calculation.
    pub fn screen_translation(
        &self,
        point: [f32; 3],
        delta: [f32; 2],
        viewport: [u32; 2],
    ) -> Result<[f32; 3]> {
        ensure(
            viewport.into_iter().all(|v| v > 0)
                && point.into_iter().chain(delta).all(f32::is_finite),
            "invalid preview drag",
        )?;
        let scale =
            (viewport[0] as f64 / self.width as f64).min(viewport[1] as f64 / self.height as f64);
        let vp = self.camera.view_projection.as_dmat4();
        let world = to_world(point, self.width, self.height).as_dvec3();
        let mut clip = vp * world.extend(1.0);
        ensure(
            clip.is_finite() && clip.w > 1.0e-8,
            "drag target is behind the camera",
        )?;
        clip.x += 2.0 * delta[0] as f64 / (self.width as f64 * scale) * clip.w;
        clip.y -= 2.0 * delta[1] as f64 / (self.height as f64 * scale) * clip.w;
        let moved = vp.inverse() * clip;
        ensure(
            moved.is_finite() && moved.w.abs() > 1.0e-8,
            "invalid drag projection",
        )?;
        let offset = moved.truncate() / moved.w - world;
        let result = [offset.x as f32, -offset.y as f32, -offset.z as f32];
        ensure(
            result.into_iter().all(f32::is_finite),
            "drag exceeds numeric range",
        )?;
        Ok(result)
    }
}

pub(crate) fn geometry_offset(layer: &Layer) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        (0.5 - layer.transform.anchor[0]) * layer.size[0],
        (layer.transform.anchor[1] - 0.5) * layer.size[1],
        0.0,
    ))
}
pub(crate) fn pivot_matrix(layer: &Layer, frame: f64, width: u32, height: u32) -> Mat4 {
    let frame = layer.local_frame(frame);
    let t = &layer.transform;
    let mut rotation = t.rotation.sample(frame);
    let mut position = t.position.sample(frame);
    let mut scale = t.scale.sample(frame);
    if !layer.three_d {
        rotation[0] = 0.0;
        rotation[1] = 0.0;
        position[2] = 0.0;
        scale[2] = 100.0;
    }
    let quaternion = Quat::from_euler(
        EulerRot::XYZ,
        rotation[0].to_radians(),
        -rotation[1].to_radians(),
        -rotation[2].to_radians(),
    );
    let scale = Vec3::from_array(scale) / 100.0;
    Mat4::from_scale_rotation_translation(scale, quaternion, to_world(position, width, height))
}

/// Project parent motion into the composition plane for a flat child.
pub(crate) fn flat_matrix(m: Mat4) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(m.x_axis.x, m.x_axis.y, 0.0, 0.0),
        glam::Vec4::new(m.y_axis.x, m.y_axis.y, 0.0, 0.0),
        glam::Vec4::Z,
        glam::Vec4::new(m.w_axis.x, m.w_axis.y, 0.0, 1.0),
    )
}
