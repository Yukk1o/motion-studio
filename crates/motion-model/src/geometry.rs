use crate::{to_world, Layer};
use glam::{EulerRot, Mat4, Quat, Vec3};

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
pub fn flat_matrix(m: Mat4) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(m.x_axis.x, m.x_axis.y, 0.0, 0.0),
        glam::Vec4::new(m.y_axis.x, m.y_axis.y, 0.0, 0.0),
        glam::Vec4::Z,
        glam::Vec4::new(m.w_axis.x, m.w_axis.y, 0.0, 1.0),
    )
}
