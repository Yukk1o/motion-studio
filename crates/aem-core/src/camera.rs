use crate::{ensure, Result, Track};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraMode {
    #[default]
    Position,
    Orbit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    #[serde(default = "legacy_created", skip_serializing_if = "is_created")]
    pub created: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<crate::ParentLink>,
    pub mode: CameraMode,
    pub position: Track<[f32; 3]>,
    pub target: Track<[f32; 3]>,
    pub roll: Track<f32>,
    pub fov: Track<f32>,
    pub radius: Track<f32>,
    pub azimuth: Track<f32>,
    pub elevation: Track<f32>,
}
fn legacy_created() -> bool {
    true
}
fn is_created(value: &bool) -> bool {
    *value
}

#[derive(Clone, Copy, Debug)]
pub struct CameraPose {
    pub eye: Vec3,
    pub target: Vec3,
    pub view_projection: Mat4,
}

pub fn to_world(point: [f32; 3], width: u32, height: u32) -> Vec3 {
    Vec3::new(
        point[0] - width as f32 / 2.0,
        height as f32 / 2.0 - point[1],
        -point[2],
    )
}
pub fn to_project(point: Vec3, width: u32, height: u32) -> [f32; 3] {
    [
        point.x + width as f32 / 2.0,
        height as f32 / 2.0 - point.y,
        -point.z,
    ]
}

impl Camera {
    pub fn new(width: u32, height: u32) -> Self {
        let distance = height as f32 / (2.0 * (45.0f32.to_radians() / 2.0).tan());
        let target = [width as f32 / 2.0, height as f32 / 2.0, 0.0];
        Self {
            created: true,
            parent: None,
            mode: CameraMode::Position,
            position: Track::constant([target[0], target[1], -distance]),
            target: Track::constant(target),
            roll: Track::constant(0.0),
            fov: Track::constant(45.0),
            radius: Track::constant(distance),
            azimuth: Track::constant(0.0),
            elevation: Track::constant(0.0),
        }
    }
    pub fn validate(&self, frames: u32) -> Result<()> {
        self.position.validate(frames)?;
        self.target.validate(frames)?;
        for track in [&self.position, &self.target] {
            for value in std::iter::once(track.value).chain(track.keys.iter().map(|k| k.value)) {
                ensure(
                    value.into_iter().all(|v| v.abs() <= 10_000_000.0),
                    "camera position exceeds its numeric range",
                )?;
            }
        }
        for track in [
            &self.roll,
            &self.fov,
            &self.radius,
            &self.azimuth,
            &self.elevation,
        ] {
            track.validate(frames)?;
        }
        for value in std::iter::once(self.fov.value).chain(self.fov.keys.iter().map(|k| k.value)) {
            ensure(
                (10.0..=120.0).contains(&value),
                "field of view must be 10..120 degrees",
            )?;
        }
        for value in
            std::iter::once(self.radius.value).chain(self.radius.keys.iter().map(|k| k.value))
        {
            ensure(
                (1.0..=10_000_000.0).contains(&value),
                "orbit radius is outside its numeric range",
            )?;
        }
        for value in
            std::iter::once(self.elevation.value).chain(self.elevation.keys.iter().map(|k| k.value))
        {
            ensure(
                (-89.0..=89.0).contains(&value),
                "orbit elevation must avoid the poles",
            )?;
        }
        for track in [&self.roll, &self.azimuth] {
            for value in std::iter::once(track.value).chain(track.keys.iter().map(|k| k.value)) {
                ensure(
                    value.abs() <= 1_000_000.0,
                    "camera angle exceeds its numeric range",
                )?;
            }
        }
        Ok(())
    }
    pub fn position_at(&self, frame: f64) -> [f32; 3] {
        if self.mode == CameraMode::Position {
            return self.position.sample(frame);
        }
        let target = self.target.sample(frame);
        let r = self.radius.sample(frame);
        let az = self.azimuth.sample(frame).to_radians();
        let el = self.elevation.sample(frame).to_radians();
        [
            target[0] + r * az.sin() * el.cos(),
            target[1] - r * el.sin(),
            target[2] - r * az.cos() * el.cos(),
        ]
    }
    pub fn pose(&self, frame: f64, width: u32, height: u32) -> CameraPose {
        self.pose_parented(frame, width, height, Mat4::IDENTITY)
    }
    pub fn pose_parented(&self, frame: f64, width: u32, height: u32, parent: Mat4) -> CameraPose {
        let local_eye = to_world(self.position_at(frame), width, height);
        let local_target = to_world(self.target.sample(frame), width, height);
        let eye = parent.transform_point3(local_eye);
        let mut target = parent.transform_point3(local_target);
        if eye.distance_squared(target) < 1.0e-8 {
            target = eye - Vec3::Z;
        }
        let forward = (target - eye).normalize();
        let basis_up = if forward.dot(Vec3::Y).abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let local_forward = (local_target - local_eye)
            .try_normalize()
            .unwrap_or(-Vec3::Z);
        let local_basis = if local_forward.dot(Vec3::Y).abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let transformed = parent.transform_vector3(
            Quat::from_axis_angle(local_forward, self.roll.sample(frame).to_radians())
                * local_basis,
        );
        let up = transformed
            .try_normalize()
            .filter(|up| up.dot(forward).abs() < 0.999)
            .unwrap_or(basis_up);
        let view = Mat4::look_at_rh(eye, target, up);
        // glam's non-GL RH projection has the 0..1 depth range wgpu requires.
        let projection = Mat4::perspective_rh(
            self.fov.sample(frame).to_radians(),
            width as f32 / height as f32,
            0.5,
            100_000.0,
        );
        CameraPose {
            eye,
            target,
            view_projection: projection * view,
        }
    }
    /// Bake on integer frames to preserve an existing animation when modes change.
    pub fn convert_mode(&mut self, mode: CameraMode, frames: u32) -> Result<()> {
        fn keep_static<T: crate::Tween>(track: &mut Track<T>) {
            if let Some(first) = track.keys.first() {
                let value = first.value;
                if track.keys.iter().all(|key| key.value == value) {
                    track.value = value;
                    track.keys.clear();
                }
            }
        }
        if self.mode == mode {
            return Ok(());
        }
        if mode == CameraMode::Position {
            let mut positions = Track::constant(self.position_at(0.0));
            for frame in 0..frames {
                positions.upsert(
                    frame,
                    self.position_at(f64::from(frame)),
                    crate::Ease::Linear,
                )?;
            }
            keep_static(&mut positions);
            self.position = positions;
        } else {
            let mut radius = Track::constant(1.0);
            let mut azimuth = Track::constant(0.0);
            let mut elevation = Track::constant(0.0);
            let mut previous_azimuth: Option<f32> = None;
            for frame in 0..frames {
                let pos = Vec3::from_array(self.position_at(f64::from(frame)));
                let target = Vec3::from_array(self.target.sample(f64::from(frame)));
                let delta = pos - target;
                let r = delta.length();
                ensure(r >= 1.0, "cannot orbit at a coincident target")?;
                let mut az = delta.x.atan2(-delta.z).to_degrees();
                if let Some(prev) = previous_azimuth {
                    while az - prev > 180.0 {
                        az -= 360.0;
                    }
                    while az - prev < -180.0 {
                        az += 360.0;
                    }
                }
                let el = (-delta.y / r).clamp(-1.0, 1.0).asin().to_degrees();
                ensure(el.abs() <= 89.0, "camera is too close to an orbit pole")?;
                radius.upsert(frame, r, crate::Ease::Linear)?;
                azimuth.upsert(frame, az, crate::Ease::Linear)?;
                elevation.upsert(frame, el, crate::Ease::Linear)?;
                previous_azimuth = Some(az);
            }
            keep_static(&mut radius);
            keep_static(&mut azimuth);
            keep_static(&mut elevation);
            self.radius = radius;
            self.azimuth = azimuth;
            self.elevation = elevation;
        }
        self.mode = mode;
        Ok(())
    }
}

/// Observer state is deliberately absent from Project and undo/save history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationView {
    Free,
    Top,
    Side,
}
#[derive(Clone, Debug)]
pub struct Observer {
    pub camera: Camera,
    pub view: ObservationView,
}
impl Observer {
    pub fn new(width: u32, height: u32) -> Self {
        let mut camera = Camera::new(width, height);
        camera.mode = CameraMode::Orbit;
        Self {
            camera,
            view: ObservationView::Free,
        }
    }
    pub fn pose(&self, width: u32, height: u32) -> CameraPose {
        if self.view == ObservationView::Free {
            return self.camera.pose(0.0, width, height);
        }
        let target = to_world(self.camera.target.value, width, height);
        let distance = self.camera.radius.value;
        let (eye, up) = if self.view == ObservationView::Top {
            (target + Vec3::Y * distance, Vec3::Z)
        } else {
            (target + Vec3::X * distance, Vec3::Y)
        };
        let half_height = distance * (self.camera.fov.value.to_radians() / 2.0).tan();
        let half_width = half_height * width as f32 / height as f32;
        CameraPose {
            eye,
            target,
            view_projection: Mat4::orthographic_rh(
                -half_width,
                half_width,
                -half_height,
                half_height,
                0.5,
                100_000.0,
            ) * Mat4::look_at_rh(eye, target, up),
        }
    }
    pub fn pan(&mut self, x: f32, y: f32, width: u32, height: u32) -> Result<()> {
        ensure(
            x.is_finite() && y.is_finite() && x.abs() <= 1_000_000.0 && y.abs() <= 1_000_000.0,
            "invalid observation pan",
        )?;
        let p = self.pose(width, height);
        let forward = (p.target - p.eye).normalize();
        let base_up = if self.view == ObservationView::Top {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let right = forward.cross(base_up).normalize();
        let up = right.cross(forward).normalize();
        self.camera.target.value = to_project(p.target + right * x + up * y, width, height);
        Ok(())
    }
    pub fn orbit(&mut self, azimuth_delta: f32, elevation_delta: f32) -> Result<()> {
        ensure(
            azimuth_delta.is_finite()
                && elevation_delta.is_finite()
                && azimuth_delta.abs() <= 1_000_000.0
                && elevation_delta.abs() <= 1_000_000.0,
            "invalid observation gesture",
        )?;
        self.camera.azimuth.value = (self.camera.azimuth.value + azimuth_delta).rem_euclid(360.0);
        self.camera.elevation.value =
            (self.camera.elevation.value + elevation_delta).clamp(-89.0, 89.0);
        Ok(())
    }
    pub fn zoom(&mut self, ratio: f32) -> Result<()> {
        ensure(ratio.is_finite() && ratio > 0.0, "invalid observation zoom")?;
        self.camera.radius.value = (self.camera.radius.value / ratio).clamp(1.0, 10_000_000.0);
        Ok(())
    }
}
