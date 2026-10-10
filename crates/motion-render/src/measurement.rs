use crate::GpuTiming;
use serde_json::{json, Value};
use std::time::Instant;
#[derive(Clone, Copy, Debug)]
pub struct FrameMeasurement {
    pub sequence: u64,
    pub frame: f64,
    pub elapsed_us: u64,
    pub cpu_prepare_us: u64,
    pub acquire_us: u64,
    pub submit_us: u64,
    pub present_call_us: u64,
    pub render_wall_us: u64,
    pub render_width: u32,
    pub render_height: u32,
    pub preview_fps: u32,
    pub gpu: Option<GpuTiming>,
}
pub struct FrameRecorder {
    start: Instant,
    first_sequence: u64,
    limit: usize,
    frames: Vec<FrameMeasurement>,
    pub overflow: u64,
}
impl FrameRecorder {
    pub fn new(first_sequence: u64, limit: usize) -> Self {
        let limit = limit.clamp(1, 65_536);
        Self {
            start: Instant::now(),
            first_sequence,
            limit,
            frames: Vec::with_capacity(limit),
            overflow: 0,
        }
    }
    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
    pub fn record(&mut self, frame: FrameMeasurement) {
        if self.frames.len() < self.limit {
            self.frames.push(frame)
        } else {
            self.overflow += 1;
        }
    }
    pub fn timing(&mut self, t: GpuTiming) {
        if let Some(i) = t.sequence.checked_sub(self.first_sequence) {
            if let Some(frame) = self.frames.get_mut(i as usize) {
                if frame.sequence == t.sequence {
                    frame.gpu = Some(t)
                }
            }
        }
    }
    pub fn report(&self, metadata: Value) -> Value {
        let gpu_count = self.frames.iter().filter(|f| f.gpu.is_some()).count();
        let metric = |field: fn(&FrameMeasurement) -> Option<f64>| {
            distribution(self.frames.iter().filter_map(field))
        };
        json!({"metadata":metadata,"durationSeconds":self.start.elapsed().as_secs_f64(),"frameCount":self.frames.len(),"overflowFrames":self.overflow,
            "gpuSampleCount":gpu_count,"gpuSampleCoverage":if self.frames.is_empty(){0.0}else{gpu_count as f64/self.frames.len() as f64},
            "cpuPrepareUs":metric(|f|Some(f.cpu_prepare_us as f64)),"acquireUs":metric(|f|Some(f.acquire_us as f64)),
            "submitUs":metric(|f|Some(f.submit_us as f64)),"presentCallUs":metric(|f|Some(f.present_call_us as f64)),"renderWallUs":metric(|f|Some(f.render_wall_us as f64)),
            "gpuTotalUs":metric(|f|f.gpu.map(|g|g.total_us)),"gpuCompositionUs":metric(|f|f.gpu.map(|g|g.composition_us)),"gpuPresentationUs":metric(|f|f.gpu.map(|g|g.presentation_us)),
            "presentationScope":"Native present() calls and GPU pass timestamps. Actual display deadlines require SurfaceFlinger FrameTimeline evidence.",
            "frames":self.frames.iter().map(|f|json!({"sequence":f.sequence,"frame":f.frame,"elapsedUs":f.elapsed_us,"cpuPrepareUs":f.cpu_prepare_us,
                "acquireUs":f.acquire_us,"submitUs":f.submit_us,"presentCallUs":f.present_call_us,"renderWallUs":f.render_wall_us,
                "renderWidth":f.render_width,"renderHeight":f.render_height,"previewFps":f.preview_fps,
                "gpuCompositionUs":f.gpu.map(|g|g.composition_us),"gpuPresentationUs":f.gpu.map(|g|g.presentation_us),"gpuTotalUs":f.gpu.map(|g|g.total_us)})).collect::<Vec<_>>()})
    }
}
pub fn distribution(values: impl Iterator<Item = f64>) -> Value {
    let mut values: Vec<_> = values.filter(|v| v.is_finite()).collect();
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        values[((values.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)]
    };
    json!({"count":values.len(),"p50":percentile(0.5),"p95":percentile(0.95),"p99":percentile(0.99),"max":values.last()})
}
