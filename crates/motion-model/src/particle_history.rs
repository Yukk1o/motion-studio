//! Immutable birth inputs. Keep only the emitter's spatial ancestry and tracks,
//! never media, GPU resources or the currently expression-evaluated project.
use crate::{
    ensure, Content, EffectInstance, EffectParam, ExpressionTarget, Layer, Project, Result,
};
use glam::Mat4;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct ParticleHistory {
    pose: Project,
    params: BTreeMap<String, EffectParam>,
    offset: i32,
    camera_parent: bool,
}
impl ParticleHistory {
    pub fn capture(
        p: &Project,
        emitter: &Layer,
        e: &EffectInstance,
        previous: Option<&Arc<Self>>,
    ) -> Result<Arc<Self>> {
        if let Some(asset) = e.scene.as_ref().and_then(|s| s.sprite_asset) {
            ensure(
                p.assets.iter().any(|a| a.id == asset),
                &format!("particle sprite image {asset} missing"),
            )?;
        }
        let mut nodes = Vec::new();
        let mut id = e
            .scene
            .as_ref()
            .and_then(|s| s.source_layer)
            .unwrap_or(emitter.id);
        let mut camera_parent = false;
        for _ in 0..=p.layers.len() + 1 {
            let link = if id == 0 {
                ensure(
                    !camera_parent && p.camera.created,
                    "invalid particle camera parent",
                )?;
                camera_parent = true;
                p.camera.parent.as_ref()
            } else {
                let node = p
                    .layers
                    .iter()
                    .find(|l| l.id == id)
                    .ok_or(crate::Error::Missing(id))?;
                ensure(
                    !nodes.iter().any(|l: &&Layer| l.id == id),
                    "particle source hierarchy contains a cycle",
                )?;
                ensure(
                    !matches!(node.content, Content::Audio { .. }),
                    "audio cannot be a particle source",
                )?;
                nodes.push(node);
                node.parent.as_ref()
            };
            let Some(parent) = link.and_then(|l| l.object) else {
                break;
            };
            id = parent;
        }
        for expression in p.expressions.iter().filter(|x| x.enabled) {
            let dependent = match &expression.target {
                ExpressionTarget::Property { object, .. } => {
                    nodes.iter().any(|l| l.id == *object) || (*object == 0 && camera_parent)
                }
                ExpressionTarget::Effect { object, effect, .. } => {
                    *object == emitter.id && *effect == e.id
                }
            };
            ensure(!dependent, "particle birth history supports keyframes; expressions on emitter ancestry or particle parameters are not supported yet")?;
        }
        let offset = emitter.timeline.map_or(0, |t| t.offset_frame);
        if let Some(old) = previous {
            if old.offset == offset
                && old.params == e.params
                && old.camera_parent == camera_parent
                && old.pose.width == p.width
                && old.pose.height == p.height
                && old.pose.fps == p.fps
                && (!camera_parent || old.pose.camera == p.camera)
                && old.pose.layers.len() == nodes.len()
                && old.pose.layers.iter().zip(&nodes).all(|(a, b)| {
                    a.id == b.id
                        && a.transform == b.transform
                        && a.parent == b.parent
                        && a.timeline == b.timeline
                        && a.three_d == b.three_d
                })
            {
                return Ok(old.clone());
            }
        }
        let mut pose = Project::new(p.width, p.height, p.fps, p.frames)?;
        if camera_parent {
            pose.camera = p.camera.clone();
        }
        for node in nodes {
            let mut skeletal = Layer::solid(node.id, "Particle pose", [1., 1.], [0.; 3], [0.; 4]);
            skeletal.content = Content::Null;
            skeletal.transform = node.transform.clone();
            skeletal.parent = node.parent.clone();
            skeletal.timeline = node.timeline;
            skeletal.three_d = node.three_d;
            pose.layers.push(skeletal);
        }
        Ok(Arc::new(Self {
            pose,
            params: e.params.clone(),
            offset,
            camera_parent,
        }))
    }
    pub fn parameter(&self, id: &str, seconds: f64) -> Result<[f32; 4]> {
        self.params
            .get(id)
            .map(|p| p.sample(seconds * self.pose.fps as f64))
            .ok_or_else(|| crate::Error::Invalid(format!("particle parameter {id} missing")))
    }
    pub fn matrix_at(
        &self,
        seconds: f64,
        world: &mut Vec<Mat4>,
        states: &mut Vec<u8>,
    ) -> Result<Mat4> {
        crate::hierarchy::matrix_for(
            &self.pose,
            seconds * self.pose.fps as f64 + self.offset as f64,
            0,
            world,
            states,
        )
    }
}
