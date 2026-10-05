use crate::{ensure, Error, Project, Result};
use glam::Mat4;

pub fn validate(p: &Project) -> Result<()> {
    for object in std::iter::once(0).chain(p.layers.iter().map(|l| l.id)) {
        let mut current = object;
        for depth in 0..=p.layers.len() + 1 {
            let parent = if current == 0 {
                p.camera.parent.as_ref()
            } else {
                p.layers
                    .iter()
                    .find(|l| l.id == current)
                    .ok_or(Error::Missing(current))?
                    .parent
                    .as_ref()
            };
            let Some(link) = parent else { break };
            ensure(
                link.bind.iter().flatten().all(|v| v.is_finite()),
                "invalid parent bind matrix",
            )?;
            let matrix = Mat4::from_cols_array_2d(&link.bind);
            ensure(
                matrix.x_axis.w.abs() < 1e-5
                    && matrix.y_axis.w.abs() < 1e-5
                    && matrix.z_axis.w.abs() < 1e-5
                    && (matrix.w_axis.w - 1.0).abs() < 1e-5,
                "parent bind must be affine",
            )?;
            let Some(parent) = link.object else { break };
            if parent == 0 {
                ensure(p.camera.created, "parent camera does not exist")?;
            } else {
                ensure(
                    p.layers.iter().any(|l| l.id == parent),
                    "parent object does not exist",
                )?;
            }
            ensure(
                parent != object && depth <= p.layers.len(),
                "parent hierarchy contains a cycle",
            )?;
            current = parent;
        }
    }
    Ok(())
}

pub fn matrices(
    p: &Project,
    frame: f64,
    world: &mut Vec<Mat4>,
    states: &mut Vec<u8>,
) -> Result<()> {
    world.resize(p.layers.len() + 1, Mat4::IDENTITY);
    states.resize(p.layers.len() + 1, 0);
    states.fill(0);
    for i in 0..world.len() {
        node(p, frame, i, world, states)?;
    }
    Ok(())
}
fn node(
    p: &Project,
    frame: f64,
    index: usize,
    world: &mut [Mat4],
    states: &mut [u8],
) -> Result<Mat4> {
    if states[index] == 2 {
        return Ok(world[index]);
    }
    ensure(states[index] == 0, "parent hierarchy contains a cycle")?;
    states[index] = 1;
    let link = if index == p.layers.len() {
        p.camera.parent.as_ref()
    } else {
        p.layers[index].parent.as_ref()
    };
    let prefix = if let Some(link) = link {
        let parent = if let Some(object) = link.object {
            let index = if object == 0 {
                p.layers.len()
            } else {
                p.layers
                    .iter()
                    .position(|l| l.id == object)
                    .ok_or(Error::Missing(object))?
            };
            node(p, frame, index, world, states)?
        } else {
            Mat4::IDENTITY
        };
        parent * Mat4::from_cols_array_2d(&link.bind)
    } else {
        Mat4::IDENTITY
    };
    let value = if index == p.layers.len() {
        let implicit;
        let camera = if p.camera.created {
            &p.camera
        } else {
            implicit = crate::Camera::new(p.width, p.height);
            &implicit
        };
        let pose = camera.pose_parented(frame, p.width, p.height, prefix);
        // Recover the camera's orthonormal world basis from its projection/view.
        let projection = Mat4::perspective_rh(
            camera.fov.sample(frame).clamp(10.0, 120.0).to_radians(),
            p.width as f32 / p.height as f32,
            0.5,
            100_000.0,
        );
        (projection.inverse() * pose.view_projection).inverse()
    } else {
        prefix * crate::scene::pivot_matrix(&p.layers[index], frame, p.width, p.height)
    };
    ensure(value.is_finite(), "invalid parent transform")?;
    world[index] = value;
    states[index] = 2;
    Ok(value)
}

pub fn prefix(p: &Project, object: u64, frame: f64) -> Result<Mat4> {
    let mut world = Vec::with_capacity(p.layers.len() + 1);
    let mut states = Vec::with_capacity(p.layers.len() + 1);
    matrices(p, frame, &mut world, &mut states)?;
    let link = if object == 0 {
        p.camera.parent.as_ref()
    } else {
        p.layers
            .iter()
            .find(|l| l.id == object)
            .ok_or(Error::Missing(object))?
            .parent
            .as_ref()
    };
    Ok(if let Some(link) = link {
        let parent = if let Some(object) = link.object {
            let index = if object == 0 {
                p.layers.len()
            } else {
                p.layers
                    .iter()
                    .position(|l| l.id == object)
                    .ok_or(Error::Missing(object))?
            };
            world[index]
        } else {
            Mat4::IDENTITY
        };
        parent * Mat4::from_cols_array_2d(&link.bind)
    } else {
        Mat4::IDENTITY
    })
}
pub fn reparent(p: &mut Project, object: u64, parent: Option<u64>, frame: u32) -> Result<()> {
    if object == 0 {
        ensure(p.camera.created, "camera does not exist")?;
    } else {
        ensure(!p.layer_mut(object)?.locked, "object is locked")?;
    }
    let old = prefix(p, object, frame as f64)?;
    let new_link = if let Some(parent) = parent {
        ensure(parent != object, "cannot parent an object to itself")?;
        let mut world = Vec::new();
        let mut states = Vec::new();
        matrices(p, frame as f64, &mut world, &mut states)?;
        let i = if parent == 0 {
            ensure(p.camera.created, "camera does not exist")?;
            p.layers.len()
        } else {
            p.layers
                .iter()
                .position(|l| l.id == parent)
                .ok_or(Error::Missing(parent))?
        };
        ensure(
            world[i].determinant().abs() > 1e-8,
            "cannot bind to a zero-scale parent",
        )?;
        Some(crate::ParentLink {
            object: Some(parent),
            bind: (world[i].inverse() * old).to_cols_array_2d(),
        })
    } else {
        // Keep an unparented object's world offset without rewriting keyframes.
        if old.abs_diff_eq(Mat4::IDENTITY, 1e-6) {
            None
        } else {
            Some(crate::ParentLink {
                object: None,
                bind: old.to_cols_array_2d(),
            })
        }
    };
    if object == 0 {
        p.camera.parent = new_link
    } else {
        p.layer_mut(object)?.parent = new_link;
    }
    validate(p)
}
