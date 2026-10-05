//! Time curves shared by interactive preview, graph display and frozen export.
//! Bezier velocity curves are integrated analytically, including their x mapping.
use crate::{ensure, Ease, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveSpace {
    #[default]
    Progress,
    Velocity,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveShape {
    Quadratic {
        control: [f64; 2],
        #[serde(default)]
        start: f64,
        #[serde(default = "one")]
        end: f64,
    },
    Cubic {
        control1: [f64; 2],
        control2: [f64; 2],
        #[serde(default)]
        start: f64,
        #[serde(default = "one")]
        end: f64,
    },
    Elastic {
        oscillations: f64,
        damping: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    #[serde(default)]
    pub space: CurveSpace,
    pub shape: CurveShape,
}

/// Clipboard payload contains timing only, never property values or key times.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Easing {
    #[serde(default)]
    pub ease: Ease,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct CurveSample {
    pub progress: f64,
    /// d(progress)/d(normalized time). Multiply by value delta / duration for units/s.
    pub velocity: f64,
}

impl Easing {
    pub fn validate(self) -> Result<()> {
        if let Some(curve) = self.curve {
            curve.validate()?;
        }
        Ok(())
    }
    pub fn sample(self, t: f64) -> CurveSample {
        if let Some(curve) = self.curve {
            return curve.sample(t);
        }
        let t = t.clamp(0.0, 1.0);
        let velocity = match self.ease {
            Ease::Linear => 1.0,
            Ease::In => 2.0 * t,
            Ease::Out => 2.0 * (1.0 - t),
            Ease::InOut => 6.0 * t * (1.0 - t),
            Ease::Hold => 0.0, // The end discontinuity has no finite derivative.
        };
        CurveSample {
            progress: f64::from(self.ease.map(t as f32)),
            velocity,
        }
    }
}

impl Curve {
    pub fn definition_scale(self) -> f64 {
        if self.space == CurveSpace::Velocity {
            if let Some((x, y)) = self.polynomials() {
                return 1.0 / integral(x, y, 1.0);
            }
        }
        1.0
    }
    pub fn validate(self) -> Result<()> {
        match self.shape {
            CurveShape::Elastic {
                oscillations,
                damping,
            } => {
                ensure(
                    oscillations.is_finite() && (0.5..=8.0).contains(&oscillations),
                    "elastic oscillations must be 0.5–8",
                )?;
                ensure(
                    damping.is_finite() && (0.5..=20.0).contains(&damping),
                    "elastic damping must be 0.5–20",
                )?;
            }
            CurveShape::Quadratic {
                control,
                start,
                end,
            } => {
                self.validate_bezier(&[control], start, end)?;
            }
            CurveShape::Cubic {
                control1,
                control2,
                start,
                end,
            } => {
                self.validate_bezier(&[control1, control2], start, end)?;
            }
        }
        Ok(())
    }
    fn validate_bezier(self, controls: &[[f64; 2]], start: f64, end: f64) -> Result<()> {
        ensure(
            controls.iter().all(|p| {
                p[0].is_finite()
                    && (0.0..=1.0).contains(&p[0])
                    && p[1].is_finite()
                    && p[1].abs() <= 8.0
            }),
            "Bezier control is outside its numeric range",
        )?;
        ensure(
            start.is_finite() && end.is_finite() && start.abs() <= 8.0 && end.abs() <= 8.0,
            "invalid curve endpoints",
        )?;
        if self.space == CurveSpace::Progress {
            ensure(
                start == 0.0 && end == 1.0,
                "progress endpoints must be zero and one",
            )?;
        } else {
            let (x, y) = self.polynomials().unwrap();
            ensure(
                integral(x, y, 1.0).abs() >= 0.05,
                "velocity curve area is too close to zero",
            )?;
        }
        Ok(())
    }
    fn polynomials(self) -> Option<([f64; 4], [f64; 4])> {
        Some(match self.shape {
            CurveShape::Quadratic {
                control: [x, y],
                start: s,
                end: e,
            } => (
                [0.0, 2.0 * x, 1.0 - 2.0 * x, 0.0],
                [s, 2.0 * (y - s), e - 2.0 * y + s, 0.0],
            ),
            CurveShape::Cubic {
                control1: [x1, y1],
                control2: [x2, y2],
                start: s,
                end: e,
            } => (
                [
                    0.0,
                    3.0 * x1,
                    3.0 * (x2 - 2.0 * x1),
                    1.0 + 3.0 * x1 - 3.0 * x2,
                ],
                [
                    s,
                    3.0 * (y1 - s),
                    3.0 * (s - 2.0 * y1 + y2),
                    e - s + 3.0 * y1 - 3.0 * y2,
                ],
            ),
            CurveShape::Elastic { .. } => return None,
        })
    }
    pub fn sample(self, t: f64) -> CurveSample {
        let t = t.clamp(0.0, 1.0);
        if let CurveShape::Elastic {
            oscillations,
            damping,
        } = self.shape
        {
            // Unit step response of a damped oscillator, normalized to end at 1.
            // In velocity space the editable definition is this response's derivative.
            let w = std::f64::consts::TAU * oscillations;
            let step =
                |t: f64| 1.0 - (-damping * t).exp() * ((w * t).cos() + damping / w * (w * t).sin());
            let norm = step(1.0);
            return CurveSample {
                progress: if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    step(t) / norm
                },
                velocity: (-damping * t).exp() * (w + damping * damping / w) * (w * t).sin() / norm,
            };
        }
        let (x, y) = self.polynomials().unwrap();
        let u = invert(x, t);
        if self.space == CurveSpace::Velocity {
            let area = integral(x, y, 1.0);
            CurveSample {
                progress: if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    integral(x, y, u) / area
                },
                velocity: polynomial(y, u) / area,
            }
        } else {
            // A vertical tangent has an unbounded mathematical speed; graph sampling
            // reports a finite one-sided approximation instead of NaN/Infinity JSON.
            let v = if derivative(x, u).abs() < 1e-10 {
                u.clamp(1e-6, 1.0 - 1e-6)
            } else {
                u
            };
            CurveSample {
                progress: if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    polynomial(y, u)
                },
                velocity: (derivative(y, v) / derivative(x, v).max(1e-12)).clamp(-1e6, 1e6),
            }
        }
    }
}

fn polynomial(c: [f64; 4], u: f64) -> f64 {
    ((c[3] * u + c[2]) * u + c[1]) * u + c[0]
}
fn derivative(c: [f64; 4], u: f64) -> f64 {
    (3.0 * c[3] * u + 2.0 * c[2]) * u + c[1]
}
fn invert(x: [f64; 4], t: f64) -> f64 {
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let (mut lo, mut hi, mut u) = (0.0, 1.0, t);
    for _ in 0..24 {
        let error = polynomial(x, u) - t;
        if error.abs() < 1e-10 {
            break;
        }
        if error > 0.0 {
            hi = u;
        } else {
            lo = u;
        }
        let next = u - error / derivative(x, u);
        u = if next.is_finite() && next > lo && next < hi {
            next
        } else {
            (lo + hi) * 0.5
        };
    }
    u
}
fn integral(x: [f64; 4], y: [f64; 4], u: f64) -> f64 {
    // Integral y(u) x'(u) du, not integral y(u) du.
    let mut product = [0.0; 6];
    for i in 0..4 {
        for j in 0..3 {
            product[i + j] += y[i] * (j + 1) as f64 * x[j + 1];
        }
    }
    let mut value = 0.0;
    for i in (0..6).rev() {
        value = value * u + product[i] / (i + 1) as f64;
    }
    value * u
}
