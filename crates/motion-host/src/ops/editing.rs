//! Commands, gestures and temporal curve operations.
use crate::session::{Result, Session};
use motion_core::{Command, Engine};
use serde_json::{json, Value};

impl Session {
    /// Apply a batch of commands with asset validation and one-step undo.
    pub fn apply_commands(&mut self, text: &str) -> Result<Value> {
        let commands = motion_core::parse_commands(text).map_err(|e| e.to_string())?;
        self.apply_command_batch(commands)
    }
    pub fn apply_command_batch(&mut self, commands: Vec<Command>) -> Result<Value> {
        let resources = commands.iter().any(Command::changes_resources);
        if resources {
            let mut check =
                Engine::new(self.engine.snapshot()).map_err(|e| e.to_string())?;
            check
                .apply_batch(commands.clone())
                .map_err(|e| e.to_string())?;
            motion_core::storage::validate_assets(&self.root, check.project())
                .map_err(|e| e.to_string())?;
        }
        if self.editor.as_ref().is_some_and(|e| e.gesture) {
            return Err("finish the plugin editor gesture before ordinary edits".into());
        }
        let results = self
            .engine
            .apply_batch(commands)
            .map_err(|e| e.to_string())?;
        if resources {
            self.effects.alpha_images.clear();
            self.editor_renderer = None;
            self.editor_target = None;
            if let Some(g) = &mut self.graphics {
                if let Err(error) = g.renderer.configure_assets(self.engine.project(), &self.root)
                {
                    self.engine.undo().map_err(|e| e.to_string())?;
                    g.renderer.clear_assets();
                    let _ = g.renderer.configure_assets(self.engine.project(), &self.root);
                    return Err(error.to_string());
                }
            }
        }
        // The edit has committed. A frame-specific expression failure is reported
        // in renderError without pretending the stored edit failed or losing it.
        if let Err(error) = self.sample() {
            self.last_error = Some(error);
        }
        let mut snapshot = self.snapshot();
        if let Some(result) = results.last() {
            snapshot["edit_result"] = serde_json::to_value(result).map_err(|e| e.to_string())?;
            snapshot["edit_results"] =
                serde_json::to_value(&results).map_err(|e| e.to_string())?;
        }
        Ok(snapshot)
    }
}

pub fn command(id: i64, text: &str) -> Result<Value> {
    crate::session::with_session(id, |s| s.apply_commands(text))
}

pub fn drag(
    id: i64,
    object: i64,
    dx: f64,
    dy: f64,
    width: i32,
    height: i32,
) -> Result<Value> {
    crate::session::with_session(id, |s| {
        if width <= 0 || height <= 0 || !dx.is_finite() || !dy.is_finite() {
            return Err("invalid preview drag".into());
        }
        s.sample()?;
        let layer = s
            .engine
            .project()
            .layers
            .iter()
            .find(|l| l.id == object as u64)
            .ok_or("drag layer does not exist")?;
        if !layer.active(s.frame, s.engine.project().frames) {
            return Err("drag layer is outside its clip".into());
        }
        let position = layer.transform.position.sample(layer.local_frame(s.frame));
        let separated = layer.transform.position.axes.is_some();
        let world_position = s
            .scene
            .node_position(object as u64)
            .ok_or("object transform is unavailable")?;
        let offset = if layer.three_d {
            s.scene
                .screen_translation(
                    world_position,
                    [dx as f32, dy as f32],
                    [width as u32, height as u32],
                )
                .map_err(|e| e.to_string())?
        } else {
            let p = s.engine.project();
            let scale = (width as f32 / p.width as f32).min(height as f32 / p.height as f32);
            [dx as f32 / scale, dy as f32 / scale, 0.0]
        };
        let offset =
            motion_core::scene_prefix_delta(s.engine.project(), object as u64, s.frame, offset)
                .map_err(|e| e.to_string())?;
        let frame = s.frame.floor() as u32;
        let commands = if separated {
            [motion_core::Axis::X, motion_core::Axis::Y, motion_core::Axis::Z]
                .into_iter()
                .enumerate()
                .filter(|(i, _)| offset[*i].abs() > 1e-6)
                .map(|(i, axis)| Command::SetComponent {
                    object: object as u64,
                    property: motion_core::Property::Position,
                    axis,
                    frame,
                    value: position[i] + offset[i],
                })
                .collect()
        } else {
            vec![Command::SetVector {
                object: object as u64,
                property: motion_core::Property::Position,
                frame,
                value: std::array::from_fn(|i| position[i] + offset[i]),
            }]
        };
        s.engine
            .apply_batch(commands)
            .map_err(|e| e.to_string())?;
        s.sample()?;
        Ok(s.snapshot())
    })
}

/// Undo/redo and gesture bracketing. Operations are stable across hosts.
pub const HISTORY_UNDO: i32 = 0;
pub const HISTORY_REDO: i32 = 1;
pub const HISTORY_BEGIN: i32 = 2;
pub const HISTORY_COMMIT: i32 = 3;
pub const HISTORY_CANCEL: i32 = 4;

pub fn history(id: i64, op: i32) -> Result<Value> {
    crate::session::with_session(id, |s| {
        let active = s.engine.project().composition_id.clone();
        match op {
            HISTORY_UNDO => {
                s.engine.undo().map_err(|e| e.to_string())?;
            }
            HISTORY_REDO => {
                s.engine.redo().map_err(|e| e.to_string())?;
            }
            HISTORY_BEGIN => s.engine.begin_gesture().map_err(|e| e.to_string())?,
            HISTORY_COMMIT => s.engine.end_gesture(true).map_err(|e| e.to_string())?,
            HISTORY_CANCEL => s.engine.end_gesture(false).map_err(|e| e.to_string())?,
            _ => return Err("invalid history operation".into()),
        }
        if !s.engine.gesture_active() {
            let target = if s.engine.project().composition_ids().contains(&active) {
                active.as_str()
            } else {
                motion_core::MAIN_COMPOSITION
            };
            s.engine
                .activate_composition(target)
                .map_err(|e| e.to_string())?;
        }
        s.frame = s.frame.min(f64::from(s.engine.project().frames - 1));
        s.audio_mixer = None;
        s.video_frames.clear();
        if matches!(op, HISTORY_UNDO | HISTORY_REDO | HISTORY_CANCEL) {
            s.effects.alpha_images.clear();
            s.editor_renderer = None;
            s.editor_target = None;
        }
        if let Some(g) = &mut s.graphics {
            g.renderer
                .configure_assets(s.engine.project(), &s.root)
                .map_err(|e| e.to_string())?;
        }
        if let Err(error) = s.sample() {
            s.last_error = Some(error);
        }
        Ok(s.snapshot())
    })
}

/// Sample an easing into graph points without touching a session.
pub fn curve_graph(text: &str) -> Result<Value> {
    let easing: motion_core::Easing = serde_json::from_str(text).map_err(|e| e.to_string())?;
    easing.validate().map_err(|e| e.to_string())?;
    let points: Vec<_> = (0..=160).map(|i| easing.sample(i as f64 / 160.0)).collect();
    Ok(json!({"points": points,
        "definitionScale": easing.curve.map_or(1.0, |c| c.definition_scale())}))
}