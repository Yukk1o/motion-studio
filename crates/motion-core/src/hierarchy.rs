use crate::{ensure, Error, Project, Result};
use glam::Mat4;
pub(crate) use motion_model::hierarchy::{matrices, prefix, validate};

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
