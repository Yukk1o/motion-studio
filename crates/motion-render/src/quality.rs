#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Auto,
    High,
    Balanced,
    Economy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreviewTier {
    #[default]
    High,
    Balanced,
    Economy,
}
impl PreviewMode {
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Auto),
            1 => Some(Self::High),
            2 => Some(Self::Balanced),
            3 => Some(Self::Economy),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::High => "high",
            Self::Balanced => "balanced",
            Self::Economy => "economy",
        }
    }
}
impl PreviewTier {
    pub fn name(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Balanced => "balanced",
            Self::Economy => "economy",
        }
    }
    pub fn fps(self) -> u32 {
        if self == Self::Economy {
            30
        } else {
            60
        }
    }
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        if self == Self::High {
            (width, height)
        } else {
            (
                ((width as u64 * 2 + 1) / 3).max(1) as u32,
                ((height as u64 * 2 + 1) / 3).max(1) as u32,
            )
        }
    }
}
#[derive(Default)]
pub struct PreviewPolicy {
    pub mode: PreviewMode,
    tier: PreviewTier,
    thermal: i32,
    frames: u32,
    slow: u32,
    healthy: u32,
    stable_windows: u32,
}
impl PreviewPolicy {
    /// Fit automatic preview work to the physical canvas. Explicit High keeps
    /// the full composition resolution; this policy is never used by export.
    pub fn render_dimensions(
        &self,
        width: u32,
        height: u32,
        surface_width: u32,
        surface_height: u32,
    ) -> (u32, u32) {
        if self.mode == PreviewMode::High || surface_width == 0 || surface_height == 0 {
            return self.tier().dimensions(width, height);
        }
        let scale = (f64::from(surface_width) / f64::from(width))
            .min(f64::from(surface_height) / f64::from(height))
            .min(1.0);
        self.tier().dimensions(
            (f64::from(width) * scale).ceil().max(1.0) as u32,
            (f64::from(height) * scale).ceil().max(1.0) as u32,
        )
    }
    pub fn set_mode(&mut self, mode: PreviewMode) {
        self.mode = mode;
        self.tier = PreviewTier::High;
        self.reset_window();
        self.stable_windows = 0;
    }
    pub fn set_thermal(&mut self, status: i32) {
        self.thermal = status.clamp(0, 6);
    }
    pub fn tier(&self) -> PreviewTier {
        match self.mode {
            PreviewMode::High => PreviewTier::High,
            PreviewMode::Balanced => PreviewTier::Balanced,
            PreviewMode::Economy => PreviewTier::Economy,
            PreviewMode::Auto => {
                if self.thermal >= 3 {
                    PreviewTier::Economy
                } else if self.thermal >= 2 && self.tier == PreviewTier::High {
                    PreviewTier::Balanced
                } else {
                    self.tier
                }
            }
        }
    }
    pub fn observe(&mut self, cpu_us: u64, gpu_us: Option<f64>) {
        if self.mode != PreviewMode::Auto {
            return;
        }
        self.frames += 1;
        if cpu_us > 4000 || gpu_us.is_some_and(|v| v > 10_000.0) {
            self.slow += 1;
        }
        if cpu_us < 2000 && gpu_us.is_none_or(|v| v < 6000.0) {
            self.healthy += 1;
        }
        if self.frames >= 60 {
            if self.slow >= 6 {
                self.tier = match self.tier {
                    PreviewTier::High => PreviewTier::Balanced,
                    _ => PreviewTier::Economy,
                };
                self.stable_windows = 0;
            } else if self.healthy == self.frames {
                self.stable_windows += 1;
                if self.stable_windows >= 10 {
                    self.tier = match self.tier {
                        PreviewTier::Economy => PreviewTier::Balanced,
                        _ => PreviewTier::High,
                    };
                    self.stable_windows = 0;
                }
            } else {
                self.stable_windows = 0;
            }
            self.reset_window();
        }
    }
    /// Ignore an ordinary FIFO/vsync wait, but include excess queue pressure on
    /// devices without GPU timestamp queries. CPU reporting remains separate.
    pub fn observe_render(&mut self, cpu_us: u64, gpu_us: Option<f64>, surface_wait_us: u64) {
        let frame_budget_us = 1_000_000 / u64::from(self.tier().fps());
        self.observe(
            cpu_us.saturating_add(surface_wait_us.saturating_sub(frame_budget_us)),
            gpu_us,
        );
    }
    fn reset_window(&mut self) {
        self.frames = 0;
        self.slow = 0;
        self.healthy = 0;
    }
}

pub(crate) fn preview_layer_scale(
    layer: &crate::DrawLayer,
    width: u32,
    height: u32,
    scale: f32,
) -> f32 {
    if layer.three_d {
        return scale;
    }
    let m = layer.view_projection * layer.model;
    let w = m.w_axis.w;
    if !m.is_finite() || w <= 0.0 || m.x_axis.w != 0.0 || m.y_axis.w != 0.0 {
        return scale;
    }
    let screen = glam::Vec2::new(width as f32, height as f32) / (2.0 * w);
    let x = glam::Vec2::new(m.x_axis.x, m.x_axis.y) * screen;
    let y = glam::Vec2::new(m.y_axis.x, m.y_axis.y) * screen;
    // Largest singular value also handles rotation, shear, and parent scaling.
    let a = x.length_squared();
    let d = y.length_squared();
    let b = x.dot(y);
    let density = ((a + d + ((a - d) * (a - d) + 4.0 * b * b).sqrt()) * 0.5).sqrt();
    if !density.is_finite() || density <= 0.0 {
        return scale;
    }
    let mut factor = 1.0;
    for candidate in [0.5, 0.25, 0.125, 0.0625] {
        // Do not oscillate between buckets due to rotation-rounding noise.
        if candidate >= density * (1.0 - 1e-5) {
            factor = candidate;
        }
    }
    scale * factor
}
