//! Versioned composition context and transaction API; no widget/UI ownership.
use super::parse_request;
use crate::session::{Result, Session};
use motion_core::{CompositionAction, MAIN_COMPOSITION};
use serde_json::{json, Value};

impl Session {
    pub fn composition_context_snapshot(&self) -> Value {
        let p = self.engine.project();
        let Some(context) = self.composition_contexts.get(&p.composition_id) else {
            return json!({"frame":self.frame,"selection":[],"timeline":null,"path":[p.composition_id]});
        };
        let selection: Vec<_> = context
            .selection
            .iter()
            .copied()
            .filter(|id| (*id == 0 && p.camera.created) || p.layers.iter().any(|l| l.id == *id))
            .collect();
        let valid_path = context.path.last() == Some(&p.composition_id)
            && context.path.windows(2).all(|pair| {
                let layers = if pair[0] == p.composition_id {
                    Some(&p.layers)
                } else {
                    p.compositions
                        .iter()
                        .find(|c| c.id == pair[0])
                        .map(|c| &c.layers)
                };
                layers.is_some_and(|layers| {
                    layers.iter().any(|l| {
                        matches!(&l.content, motion_core::Content::Composition { clip } if clip.composition == pair[1])
                    })
                })
            });
        let path = if valid_path {
            context.path.clone()
        } else {
            vec![p.composition_id.clone()]
        };
        json!({"frame":self.frame,"selection":selection,"timeline":context.timeline,"path":path})
    }

    fn open_composition(&mut self, id: &str, path: Option<Vec<String>>) -> Result<()> {
        if self.engine.gesture_active() || self.editor.is_some() {
            return Err(self.composition_error(
                "context_busy",
                "Finish the current gesture or plugin editor first",
                json!({}),
            ));
        }
        self.engine
            .project()
            .composition_view(id)
            .map_err(|e| e.to_string())?;
        let route = if let Some(path) = path {
            if path.is_empty()
                || path.last().map(String::as_str) != Some(id)
                || path.len() > motion_core::composition::MAX_COMPOSITION_DEPTH
            {
                return Err(self.composition_error(
                    "invalid_path",
                    "Invalid breadcrumb path",
                    json!({}),
                ));
            }
            for pair in path.windows(2) {
                let p = self
                    .engine
                    .project()
                    .composition(&pair[0])
                    .map_err(|e| e.to_string())?;
                if !p.layers.iter().any(|l| {
                    matches!(&l.content, motion_core::Content::Composition { clip } if clip.composition == pair[1])
                }) {
                    return Err(self.composition_error(
                        "invalid_path",
                        "Breadcrumb contains a missing reference",
                        json!({"path":path}),
                    ));
                }
            }
            path
        } else {
            vec![id.into()]
        };
        let old = self.engine.project().composition_id.clone();
        self.composition_contexts.entry(old).or_default().frame = self.frame;
        self.engine
            .activate_composition(id)
            .map_err(|e| e.to_string())?;
        let context = self.composition_contexts.entry(id.into()).or_default();
        context.path = route;
        self.frame = context
            .frame
            .clamp(0., f64::from(self.engine.project().frames - 1));
        context.selection.retain(|id| {
            (*id == 0 && self.engine.project().camera.created)
                || self.engine.project().layers.iter().any(|l| l.id == *id)
        });
        self.scene = motion_core::Scene::new(self.engine.project());
        self.observer = motion_core::Observer::new(
            self.engine.project().width,
            self.engine.project().height,
        );
        self.observing = false;
        self.audio_mixer = None;
        self.video_frames.clear();
        self.view_revision += 1;
        self.last_presented_frame = None;
        if let Err(error) = self.sample() {
            self.last_error = Some(error);
        }
        Ok(())
    }

    pub fn composition_request(&mut self, v: Value) -> Result<Value> {
        if v["version"].as_u64() != Some(1) {
            return Err(self.composition_error(
                "unsupported_version",
                "Composition API requires version 1",
                json!({}),
            ));
        }
        let id = v["composition"].as_str().ok_or("missing composition ID")?;
        self.engine
            .project()
            .composition_view(id)
            .map_err(|e| e.to_string())?;
        let op = v["op"].as_str().ok_or("missing composition operation")?;
        match op {
            "list" => {
                return Ok(json!({"version":1,"compositions":self.engine.project().composition_list(),"revision":self.engine.revision()}))
            }
            "info" => {
                return Ok(json!({"composition":self.engine.project().composition_list().into_iter().find(|c|c["id"]==id),"revision":self.engine.revision()}))
            }
            "reference_candidates" => {
                let results = self
                    .engine
                    .project()
                    .composition_reference_candidates(id)
                    .map_err(|e| e.to_string())?;
                return Ok(json!({"compositions":results}));
            }
            "delete_check" => {
                return Ok(json!({"references":self.engine.project().composition_references(id),
                    "can_delete":id!=MAIN_COMPOSITION&&id!=self.engine.project().composition_id&&self.engine.project().composition_references(id).is_empty()}))
            }
            "settings_preview" => {
                let settings =
                    serde_json::from_value(v["settings"].clone()).map_err(|e| e.to_string())?;
                let mut test =
                    motion_core::Engine::new(self.engine.snapshot()).map_err(|e| e.to_string())?;
                let command = motion_core::Command::InComposition {
                    composition: id.into(),
                    command: Box::new(motion_core::Command::Composition {
                        action: CompositionAction::Settings { settings },
                    }),
                };
                return match test.apply_batch(vec![command]) {
                    Ok(result) => Ok(json!({"valid":true,"expected_revision":self.engine.revision(),"results":result})),
                    Err(e) => Ok(json!({"valid":false,"expected_revision":self.engine.revision(),"error":e.to_string(),
                        "error_detail":e.to_string().strip_prefix("composition_error:").and_then(|s|serde_json::from_str::<Value>(s).ok())})),
                };
            }
            "open" => {
                let path = v
                    .get("path")
                    .map(|p| serde_json::from_value(p.clone()))
                    .transpose()
                    .map_err(|e| e.to_string())?;
                self.open_composition(id, path)?;
            }
            _ => {
                if id != self.engine.project().composition_id {
                    return Err(self.composition_error(
                        "context_mismatch",
                        "Open the requested composition before this operation",
                        json!({"requested":id}),
                    ));
                }
                match op {
                    "state" => {}
                    "assert_context" => {
                        return Ok(json!({"composition":id,"revision":self.engine.revision()}))
                    }
                    "position_path" => {
                        let target = serde_json::from_value(v["target"].clone()).map_err(|e| e.to_string())?;
                        self.sample()?;
                        let mut result = motion_core::position_path::sample(self.engine.project(), &self.scene, &target, self.frame)
                            .map_err(|e|e.to_string())?;
                        result["revision"] = json!(self.engine.revision());
                        return Ok(result);
                    }
                    "context" => {
                        let selection: Vec<u64> =
                            serde_json::from_value(v["selection"].clone()).map_err(|e| e.to_string())?;
                        if selection.iter().any(|id| {
                            !(*id == 0 && self.engine.project().camera.created)
                                && !self.engine.project().layers.iter().any(|l| l.id == *id)
                        }) {
                            return Err(self.composition_error(
                                "cross_composition_selection",
                                "Selection contains an unknown layer",
                                json!({}),
                            ));
                        }
                        let c = self.composition_contexts.entry(id.into()).or_default();
                        c.selection = selection;
                        c.timeline = v["timeline"].clone();
                    }
                    "seek" => {
                        let frame = v["frame"].as_f64().ok_or("missing frame")?;
                        if !frame.is_finite()
                            || frame < 0.
                            || frame >= f64::from(self.engine.project().frames)
                        {
                            return Err(self.composition_error(
                                "invalid_range",
                                "Seek outside composition",
                                json!({"frame":frame}),
                            ));
                        }
                        self.frame = frame;
                        self.sample()?;
                    }
                    "action" | "settings_apply" => {
                        if self.editor.is_some() || self.engine.gesture_active() {
                            return Err(self.composition_error(
                                "context_busy",
                                "Finish the current gesture or plugin editor first",
                                json!({}),
                            ));
                        }
                        let action = if op == "settings_apply" {
                            if v["expected_revision"].as_u64() != Some(self.engine.revision()) {
                                return Err(self.composition_error(
                                    "stale_revision",
                                    "Recompute the settings impact before applying",
                                    json!({"revision":self.engine.revision()}),
                                ));
                            }
                            CompositionAction::Settings {
                                settings: serde_json::from_value(v["settings"].clone())
                                    .map_err(|e| e.to_string())?,
                            }
                        } else {
                            serde_json::from_value(v["action"].clone()).map_err(|e| e.to_string())?
                        };
                        let results = self
                            .engine
                            .apply_batch(vec![motion_core::Command::Composition { action }])
                            .map_err(|e| e.to_string())?;
                        if let Some(motion_core::EditResult::Composition { result }) = results.last()
                        {
                            if let Some(selection) = result["selection"].as_array() {
                                self.composition_contexts
                                    .entry(id.into())
                                    .or_default()
                                    .selection =
                                    selection.iter().filter_map(Value::as_u64).collect();
                            }
                        }
                        self.frame = self.frame.min(f64::from(self.engine.project().frames - 1));
                        self.audio_mixer = None;
                        self.video_frames.clear();
                        self.view_revision += 1;
                        if let Err(error) = self.sample() {
                            self.last_error = Some(error);
                        }
                        let mut state = self.snapshot();
                        state["edit_results"] = json!(results);
                        return Ok(state);
                    }
                    _ => {
                        return Err(self.composition_error(
                            "unsupported_operation",
                            "Unknown composition operation",
                            json!({"op":op}),
                        ))
                    }
                }
            }
        }
        Ok(self.snapshot())
    }
}

/// Serialize the versioned composition bundle used by frozen export readers.
pub fn frame_bundle(
    id: i64,
    composition: &str,
    frame: f64,
    out: &mut Vec<u8>,
) -> Result<usize> {
    crate::session::with_session(id, |s| {
        if composition != s.engine.project().composition_id {
            return Err("composition context mismatch".into());
        }
        s.frame = frame;
        s.sample()?;
        let p = s.engine.project();
        let assets = std::iter::once(0)
            .chain(p.assets.iter().map(|a| a.id))
            .collect::<Vec<_>>();
        s.effects
            .synchronize_scene_alpha(&s.scene, p, &s.root)?;
        motion_render::composition_plan::build(
            &mut s.effects,
            &s.scene,
            p,
            &assets,
            out,
        )?;
        Ok(out.len())
    })
}

pub fn render(id: i64, composition: &str, frame: f64) -> Result<bool> {
    crate::session::with_session(id, |s| {
        let result = if composition != s.engine.project().composition_id {
            Err(s.composition_error(
                "context_mismatch",
                "Open the requested composition before rendering",
                json!({}),
            ))
        } else {
            s.render(frame)
        };
        if let Err(e) = &result {
            s.last_error = Some(e.clone());
        }
        result
    })
}

pub fn request(id: i64, text: &str) -> Result<Value> {
    let value = parse_request(text, 256 * 1024, "composition request")?;
    crate::session::with_session(id, |s| {
        s.composition_request(value).map_err(|e| {
            if e.starts_with("composition_error:") {
                e
            } else {
                s.composition_error("invalid_request", &e, json!({}))
            }
        })
    })
}