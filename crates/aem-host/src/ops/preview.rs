//! Preview, observer and Surface binding operations.
use crate::session::Result;
use aem_render::{FrameRecorder, GpuTimer, PreviewMode};
use serde_json::json;

pub fn configure_memory(id: i64, total_mem: i64, guarded: bool) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        if total_mem < 0 {
            return Err("invalid device physical memory".into());
        }
        let budget = aem_render::resource_policy::scratch_budget(total_mem as u64, guarded);
        s.effects.set_scratch_budget(budget)?;
        if let Some(g) = &mut s.graphics {
            g.renderer
                .set_scratch_budget(budget)
                .map_err(|e| e.to_string())?;
        }
        if let Some(renderer) = &mut s.editor_renderer {
            renderer
                .set_scratch_budget(budget)
                .map_err(|e| e.to_string())?;
        }
        Ok(json!({"scratchBudgetBytes":budget,"policyVersion":1}))
    })
}

pub fn seek(id: i64, frame: f64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
            return Err("invalid frame".into());
        }
        s.frame = frame;
        s.sample()?;
        Ok(s.snapshot())
    })
}

pub fn observe(id: i64, enabled: bool, az: f64, el: f64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        s.observing = enabled;
        s.view_revision += 1;
        if s.observer.view == aem_core::ObservationView::Free {
            s.observer.orbit(az as f32, el as f32).map_err(|e| e.to_string())?;
        } else {
            s.observer
                .pan(
                    az as f32 * 4.0,
                    el as f32 * 4.0,
                    s.engine.project().width,
                    s.engine.project().height,
                )
                .map_err(|e| e.to_string())?;
        }
        s.sample()?;
        Ok(s.snapshot())
    })
}

pub fn navigate(
    id: i64,
    dx: f64,
    dy: f64,
    zoom: f64,
    multi: bool,
    width: i32,
    height: i32,
) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        if !s.observing || width <= 0 || height <= 0 || !dx.is_finite() || !dy.is_finite() {
            return Err("invalid observation navigation".into());
        }
        s.observer
            .zoom(zoom as f32)
            .map_err(|e| e.to_string())?;
        if multi || s.observer.view != aem_core::ObservationView::Free {
            s.sample()?;
            let target = s.observer.camera.target.value;
            let delta = s
                .scene
                .screen_translation(
                    target,
                    [dx as f32, dy as f32],
                    [width as u32, height as u32],
                )
                .map_err(|e| e.to_string())?;
            s.observer.camera.target.value = std::array::from_fn(|i| target[i] - delta[i]);
        } else {
            s.observer
                .orbit(dx as f32 * 0.18, dy as f32 * 0.18)
                .map_err(|e| e.to_string())?;
        }
        s.view_revision += 1;
        s.sample()?;
        Ok(s.snapshot())
    })
}

/// Composition view, free observation, top view and side view.
pub const VIEW_COMPOSITION: i32 = 0;
pub const VIEW_FREE: i32 = 1;
pub const VIEW_TOP: i32 = 2;
pub const VIEW_SIDE: i32 = 3;

pub fn view(id: i64, kind: i32) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        s.observing = kind != 0;
        s.view_revision += 1;
        s.observer.view = match kind {
            VIEW_COMPOSITION | VIEW_FREE => aem_core::ObservationView::Free,
            VIEW_TOP => aem_core::ObservationView::Top,
            VIEW_SIDE => aem_core::ObservationView::Side,
            _ => return Err("invalid observation view".into()),
        };
        s.sample()?;
        Ok(s.snapshot())
    })
}

pub fn preview_info(id: i64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| Ok(s.preview_info()))
}

/// Preview quality modes, matching the Android combo box order.
pub fn preview_mode(id: i64, mode: i32, thermal: i32) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        let mode = PreviewMode::from_id(mode).ok_or("invalid preview mode")?;
        let previous_tier = s.preview.tier();
        if s.preview.mode != mode {
            s.preview.set_mode(mode);
        }
        s.preview.set_thermal(thermal);
        if previous_tier != s.preview.tier() {
            s.view_revision += 1;
        }
        Ok(s.preview_info())
    })
}

pub fn start_profiling(id: i64, limit: i32) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        if !(1..=65_536).contains(&limit) {
            return Err("invalid frame recording limit".into());
        }
        let g = s.graphics.as_mut().ok_or("no preview surface")?;
        g.renderer.device.poll(wgpu::Maintain::Wait);
        g.timer = GpuTimer::new(&g.renderer.device, &g.renderer.queue);
        s.recorder = Some(FrameRecorder::new(s.presented + 1, limit as usize));
        Ok(s.preview_info())
    })
}

/// Stop recording and write `exports/preview-performance.json` in the project.
pub fn stop_profiling(id: i64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        if let Some(g) = &mut s.graphics {
            g.renderer.device.poll(wgpu::Maintain::Wait);
            if let Some(timer) = &mut g.timer {
                for t in timer.collect().into_iter().flatten() {
                    if let Some(r) = &mut s.recorder {
                        r.timing(t);
                    }
                }
            }
        }
        let r = s
            .recorder
            .as_ref()
            .ok_or("frame recording is not active")?;
        let metadata = json!({"preview":s.preview_info(),
            "projectWidth":s.engine.project().width,"projectHeight":s.engine.project().height,
            "projectFps":s.engine.project().fps,"projectFrames":s.engine.project().frames,
            "layerCount":s.engine.project().layers.len(),"surfaceEpoch":s.surface_epoch,
            "platform":s.platform().name(),
            "graphics":s.graphics.as_ref().map(|g|json!({"surfaceWidth":g.config.width,"surfaceHeight":g.config.height,
                "renderWidth":g.scratch.width,"renderHeight":g.scratch.height,
                "adapter":g.renderer.adapter_info.name,"driver":g.renderer.adapter_info.driver,
                "driverInfo":g.renderer.adapter_info.driver_info,"backend":format!("{:?}",g.renderer.adapter_info.backend),
                "assetTextureBytes":g.renderer.texture_bytes(),"renderTargetBytes":g.scratch.texture_bytes(),
                "previewImageReadbackBytes":0,
                "timestampReadbackBytesPerSample":if g.timer.is_some(){32}else{0},
                "timingSkipped":g.timer.as_ref().map(|t|t.skipped),
                "timingErrors":g.timer.as_ref().map(|t|t.errors)}))});
        let folder = s.root.join("exports");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join("preview-performance.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&r.report(metadata)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        s.recorder = None;
        Ok(json!({"file":path.to_string_lossy()}))
    })
}

/// Render one composition frame. Invalid timing is skipped, not failed.
pub fn render(id: i64, frame: f64) -> Result<bool> {
    crate::session::with_session(id, |s| render_inner(s, frame))
}

pub fn render_inner(s: &mut crate::session::Session, frame: f64) -> Result<bool> {
    if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
        return Ok(false);
    }
    match s.render(frame) {
        Ok(rendered) => Ok(rendered),
        Err(error) => {
            s.last_error = Some(error);
            Ok(false)
        }
    }
}