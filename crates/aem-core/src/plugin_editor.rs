//! Scoped, version-pinned editor bridge. Plugin code never receives general commands.
use crate::{ensure, Command, EffectAction, Engine, Error, Project, Result};
use aem_effects::{Registry, SceneSettings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorRequest {
    State,
    Key { revision: u64, param: String },
    Transform {
        revision: u64,
        property: crate::Property,
        value: [f32; 3],
    },
    Begin {
        revision: u64,
    },
    Commit {
        revision: u64,
    },
    Cancel {
        revision: u64,
    },
    Set {
        revision: u64,
        param: String,
        value: [f32; 4],
    },
    Animate {
        revision: u64,
        param: String,
        enabled: bool,
    },
    Curve {
        revision: u64,
        param: String,
        value: crate::CurveObject,
    },
    Scene {
        revision: u64,
        settings: SceneSettings,
    },
    Seed {
        revision: u64,
        seed: u32,
    },
}
pub struct PluginEditorSession {
    pub object: u64,
    pub instance: u64,
    pub dependency: crate::PluginDependency,
    pub effect: String,
    pub gesture: bool,
}
impl PluginEditorSession {
    pub fn open(
        project: &Project,
        registry: &Registry,
        object: u64,
        instance: u64,
    ) -> Result<Self> {
        let e = project
            .layers
            .iter()
            .find(|l| l.id == object)
            .and_then(|l| l.effects.iter().find(|e| e.id == instance))
            .ok_or_else(|| Error::Invalid("editor effect instance missing".into()))?;
        let package = registry
            .resolve(&e.plugin, &e.version, &e.hash)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        ensure(
            package
                .manifest
                .effects
                .iter()
                .any(|d| d.id == e.effect && (d.editor.is_some() || d.native_editor.is_some())),
            "effect has no custom editor",
        )?;
        Ok(Self {
            object,
            instance,
            dependency: e.dependency(),
            effect: e.effect.clone(),
            gesture: false,
        })
    }
    fn check<'a>(
        &self,
        project: &'a Project,
    ) -> Result<(&'a crate::Layer, &'a crate::EffectInstance)> {
        let layer = project
            .layers
            .iter()
            .find(|l| l.id == self.object)
            .ok_or_else(|| Error::Invalid("editor layer was removed".into()))?;
        let e = layer
            .effects
            .iter()
            .find(|e| e.id == self.instance)
            .ok_or_else(|| Error::Invalid("editor effect was removed".into()))?;
        ensure(
            e.dependency() == self.dependency && e.effect == self.effect,
            "editor dependency changed; reopen the editor",
        )?;
        Ok((layer, e))
    }
    pub fn state(&self, engine: &Engine, frame: u32) -> Result<Value> {
        ensure(
            frame < engine.project().frames,
            "editor frame outside composition",
        )?;
        let (layer, e) = self.check(engine.project())?;
        let values = e
            .params
            .iter()
            .map(|(id, p)| (id.clone(), json!(p.sample(layer.local_frame(frame as f64)))))
            .collect::<serde_json::Map<_, _>>();
        Ok(
            json!({"revision":engine.revision(),"frame":frame,"object":self.object,"instance":self.instance,
            "plugin":self.dependency,"effect":self.effect,"values":values,"params":e.params,"scene":e.scene,"seed":e.seed,
            "expressions":engine.project().expressions.iter().filter(|e|matches!(&e.target,crate::ExpressionTarget::Effect {object,effect,..} if *object==self.object && *effect==self.instance)).collect::<Vec<_>>(),
            "transform":layer.transform,
            "transform_values":{"position":layer.transform.position.sample(layer.local_frame(frame as f64)),
                "rotation":layer.transform.rotation.sample(layer.local_frame(frame as f64)),
                "scale":layer.transform.scale.sample(layer.local_frame(frame as f64))},
            "dimensions":[engine.project().width,engine.project().height],"camera":engine.project().camera,
            "fps":engine.project().fps,"frames":engine.project().frames,"timeline_offset":layer.timeline.map_or(0,|t|t.offset_frame),
            "images":engine.project().assets.iter().map(|a|json!({"id":a.id,"width":a.width,"height":a.height})).collect::<Vec<_>>(),
            "locked":layer.locked,"gesture":self.gesture,
            "layers":engine.project().layers.iter().map(|l|json!({"id":l.id,"name":l.name,"particle_source":!matches!(l.content,crate::Content::Audio { .. })})).collect::<Vec<_>>()}),
        )
    }
    pub fn request(
        &mut self,
        engine: &mut Engine,
        frame: u32,
        request: EditorRequest,
    ) -> Result<Value> {
        ensure(
            frame < engine.project().frames,
            "editor frame outside composition",
        )?;
        self.check(engine.project())?;
        let revision = match &request {
            EditorRequest::State => return self.state(engine, frame),
            EditorRequest::Begin { revision }
            | EditorRequest::Commit { revision }
            | EditorRequest::Cancel { revision }
            | EditorRequest::Transform { revision, .. }
            | EditorRequest::Set { revision, .. }
            | EditorRequest::Animate { revision, .. }
            | EditorRequest::Curve { revision, .. }
            | EditorRequest::Scene { revision, .. }
            | EditorRequest::Key { revision, .. }
            | EditorRequest::Seed { revision, .. } => *revision,
        };
        ensure(
            revision == engine.revision(),
            "stale editor revision; request state and retry",
        )?;
        let (_, e) = self.check(engine.project())?;
        let effect = e.id;
        let action = match request {
            EditorRequest::State => unreachable!(),
            EditorRequest::Begin { .. } => {
                ensure(!self.gesture, "editor gesture already active")?;
                engine.begin_gesture()?;
                self.gesture = true;
                return self.state(engine, frame);
            }
            EditorRequest::Commit { .. } | EditorRequest::Cancel { .. } => {
                ensure(self.gesture, "no active editor gesture")?;
                engine.end_gesture(matches!(request, EditorRequest::Commit { .. }))?;
                self.gesture = false;
                return self.state(engine, frame);
            }
            EditorRequest::Transform {
                property, value, ..
            } => {
                ensure(
                    matches!(
                        property,
                        crate::Property::Position
                            | crate::Property::Rotation
                            | crate::Property::Scale
                    ),
                    "editor can edit only its own layer position, rotation and scale",
                )?;
                engine.apply(Command::SetVector {
                    object: self.object,
                    property,
                    frame,
                    value,
                })?;
                return self.state(engine, frame);
            }
            EditorRequest::Set { param, value, .. } => EffectAction::Set {
                effect,
                param,
                frame,
                value,
            },
            EditorRequest::Key { param, .. } => {
                let (layer, instance) = self.check(engine.project())?;
                let p=instance.params.get(&param).ok_or_else(||Error::Invalid("editor parameter missing".into()))?;
                ensure(p.animatable && p.curve.is_none(),"parameter does not support numeric keys")?;
                let local=layer.clip(engine.project().frames).edit_frame(frame)?;
                if p.track.keys.is_empty() { EffectAction::Animate {effect,param,frame,enabled:true} }
                else if p.track.keys.iter().any(|k|k.frame==local) {EffectAction::DeleteKey {effect,param,frame}}
                else {EffectAction::Set {effect,param,frame,value:p.sample(local as f64)}}
            },
            EditorRequest::Animate { param, enabled, .. } => EffectAction::Animate {
                effect,
                param,
                frame,
                enabled,
            },
            EditorRequest::Curve { param, value, .. } => EffectAction::SetCurveObject {
                effect,
                param,
                frame,
                value,
            },
            EditorRequest::Scene { settings, .. } => EffectAction::SetScene {
                effect,
                scene: settings,
            },
            EditorRequest::Seed { seed, .. } => EffectAction::Seed { effect, seed },
        };
        engine.apply(Command::Effect {
            object: self.object,
            action,
        })?;
        self.state(engine, frame)
    }
    pub fn close(&mut self, engine: &mut Engine, commit: bool) -> Result<()> {
        if self.gesture {
            engine.end_gesture(commit)?;
            self.gesture = false;
        }
        Ok(())
    }
}
