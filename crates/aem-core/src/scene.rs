use crate::{camera::to_world, ensure, CameraPose, Content, Layer, Observer, Project, Result};
use glam::{EulerRot, Mat4, Quat, Vec3};

#[derive(Clone, Debug)]
pub struct DrawLayer {
    pub id: u64,
    pub model: Mat4,
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub opacity: f32,
    pub asset: Option<u64>,
    pub depth: f32,
    pub order: usize,
}
#[derive(Clone, Debug)]
pub struct Scene {
    pub camera: CameraPose,
    pub layers: Vec<DrawLayer>,
    pub background: [f32; 4],
    pub width: u32,
    pub height: u32,
    node_world: Vec<Mat4>,
    node_states: Vec<u8>,
    node_ids: Vec<u64>,
}
impl Scene {
    pub fn new(project: &Project) -> Self {
        Self {
            camera: project.camera.pose(0.0, project.width, project.height),
            layers: Vec::with_capacity(crate::MAX_LAYERS),
            background: project.background,
            width: project.width,
            height: project.height,
            node_world: Vec::with_capacity(crate::MAX_LAYERS + 1),
            node_states: Vec::with_capacity(crate::MAX_LAYERS + 1),
            node_ids: Vec::with_capacity(crate::MAX_LAYERS + 1),
        }
    }
    pub fn sample(
        &mut self,
        project: &Project,
        frame: f64,
        observer: Option<&Observer>,
    ) -> Result<()> {
        ensure(
            frame.is_finite() && frame >= 0.0 && frame < f64::from(project.frames),
            "invalid sample time",
        )?;
        crate::hierarchy::matrices(project, frame, &mut self.node_world, &mut self.node_states)?;
        self.node_ids.clear();
        self.node_ids.extend(project.layers.iter().map(|l| l.id));
        self.node_ids.push(0);
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
        let forward = (self.camera.target - self.camera.eye).normalize();
        for (order, layer) in project.layers.iter().enumerate() {
            if matches!(layer.content, Content::Null) {
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
            let (color, asset) = match &layer.content {
                Content::Null => unreachable!(),
                Content::Solid { color } => (*color, None),
                Content::Image { asset } => ([1.0; 4], Some(*asset)),
                Content::Text {
                    color,
                    raster_asset,
                    ..
                } => (*color, Some(*raster_asset)),
            };
            self.layers.push(DrawLayer {
                id: layer.id,
                model: self.node_world[order] * geometry_offset(layer),
                size: layer.size,
                color,
                opacity,
                asset,
                depth: (center - self.camera.eye).dot(forward),
                order,
            });
        }
        // Painter ordering for the MVP's non-intersecting transparent planes.
        // Same depth uses the user-authored layer order; never batch across it.
        self.layers
            .sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth).then(a.order.cmp(&b.order)));
        Ok(())
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
    let rotation = t.rotation.sample(frame);
    let quaternion = Quat::from_euler(
        EulerRot::XYZ,
        rotation[0].to_radians(),
        -rotation[1].to_radians(),
        -rotation[2].to_radians(),
    );
    let scale = Vec3::from_array(t.scale.sample(frame)) / 100.0;
    Mat4::from_scale_rotation_translation(
        scale,
        quaternion,
        to_world(t.position.sample(frame), width, height),
    )
}
