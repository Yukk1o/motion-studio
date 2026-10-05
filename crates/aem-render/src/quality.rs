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
    fn reset_window(&mut self) {
        self.frames = 0;
        self.slow = 0;
        self.healthy = 0;
    }
}
