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
}
impl Scene {
    pub fn new(project: &Project) -> Self {
        Self {
            camera: project.camera.pose(0.0, project.width, project.height),
            layers: Vec::with_capacity(crate::MAX_LAYERS),
            background: project.background,
            width: project.width,
            height: project.height,
        }
    }
    pub fn sample(
        &mut self,
        project: &Project,
        frame: f64,
        observer: Option<&Observer>,
    ) -> Result<()> {
        ensure(
            frame.is_finite() && frame >= 0.0 && frame <= f64::from(project.frames - 1),
            "invalid sample time",
        )?;
        self.camera = observer.map_or_else(
            || project.camera.pose(frame, project.width, project.height),
            |o| o.camera.pose(0.0, project.width, project.height),
        );
        self.width = project.width;
        self.height = project.height;
        self.background = project.background;
        self.layers.clear();
        let forward = (self.camera.target - self.camera.eye).normalize();
        for (order, layer) in project.layers.iter().enumerate() {
            if !layer.visible {
                continue;
            }
            let opacity = layer.transform.opacity.sample(frame);
            if opacity <= 0.0 {
                continue;
            }
            let center = to_world(
                layer.transform.position.sample(frame),
                project.width,
                project.height,
            );
            let (color, asset) = match &layer.content {
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
                model: model_matrix(layer, frame, project.width, project.height),
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
}

pub fn model_matrix(layer: &Layer, frame: f64, width: u32, height: u32) -> Mat4 {
    let t = &layer.transform;
    let rotation = t.rotation.sample(frame);
    let quaternion = Quat::from_euler(
        EulerRot::XYZ,
        rotation[0].to_radians(),
        -rotation[1].to_radians(),
        -rotation[2].to_radians(),
    );
    let scale = Vec3::from_array(t.scale.sample(frame)) / 100.0;
    let offset = Vec3::new(
        (0.5 - t.anchor[0]) * layer.size[0],
        (t.anchor[1] - 0.5) * layer.size[1],
        0.0,
    );
    Mat4::from_scale_rotation_translation(
        scale,
        quaternion,
        to_world(t.position.sample(frame), width, height),
    ) * Mat4::from_translation(offset)
}
