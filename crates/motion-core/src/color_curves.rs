//! Color transfer functions; separate from temporal keyframe easing curves.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorInterpolation {
    #[default]
    Linear,
    NaturalCubic,
}
pub fn legacy_modes() -> [ColorInterpolation; 5] {
    [ColorInterpolation::Linear; 5]
}
pub fn no_overrides(v: &[Option<Vec<f32>>; 5]) -> bool {
    v.iter().all(Option::is_none)
}

pub(crate) struct Transfer<'a> {
    points: &'a [[f32; 2]],
    mode: ColorInterpolation,
    lookup: Option<&'a [f32]>,
    coefficients: Vec<[f64; 3]>,
}
impl<'a> Transfer<'a> {
    pub fn new(
        points: &'a [[f32; 2]],
        mode: ColorInterpolation,
        lookup: Option<&'a [f32]>,
    ) -> Self {
        let n = points.len();
        let mut coefficients = vec![];
        if mode == ColorInterpolation::NaturalCubic && lookup.is_none() {
            let h: Vec<_> = points
                .windows(2)
                .map(|p| f64::from(p[1][0]) - f64::from(p[0][0]))
                .collect();
            let mut z = vec![0.; n];
            let mut mu = vec![0.; n];
            let mut c = vec![0.; n];
            for i in 1..n - 1 {
                let alpha = 3. * (f64::from(points[i + 1][1]) - f64::from(points[i][1])) / h[i]
                    - 3. * (f64::from(points[i][1]) - f64::from(points[i - 1][1])) / h[i - 1];
                let l = 2. * (f64::from(points[i + 1][0]) - f64::from(points[i - 1][0]))
                    - h[i - 1] * mu[i - 1];
                mu[i] = h[i] / l;
                z[i] = (alpha - h[i - 1] * z[i - 1]) / l;
            }
            coefficients = vec![[0.; 3]; n - 1];
            for j in (0..n - 1).rev() {
                c[j] = z[j] - mu[j] * c[j + 1];
                coefficients[j] = [
                    (f64::from(points[j + 1][1]) - f64::from(points[j][1])) / h[j]
                        - h[j] * (c[j + 1] + 2. * c[j]) / 3.,
                    c[j],
                    (c[j + 1] - c[j]) / (3. * h[j]),
                ];
            }
        }
        Self {
            points,
            mode,
            lookup,
            coefficients,
        }
    }
    pub fn at(&self, x: f32) -> f32 {
        if let Some(lut) = self.lookup {
            let position = x.clamp(0., 1.) * 255.;
            let nearest = position.round().clamp(0., 255.) as usize;
            if x == nearest as f32 / 255. {
                return lut[nearest];
            }
            let lower = position.floor().clamp(0., 255.) as usize;
            let upper = (lower + 1).min(255);
            return (lut[lower] + (lut[upper] - lut[lower]) * (position - lower as f32))
                .clamp(0., 1.);
        }
        let i = self
            .points
            .partition_point(|p| p[0] <= x)
            .clamp(1, self.points.len() - 1)
            - 1;
        let a = self.points[i];
        let b = self.points[i + 1];
        if x == a[0] {
            return a[1];
        }
        if x == b[0] {
            return b[1];
        }
        if self.mode == ColorInterpolation::Linear {
            return a[1] + (b[1] - a[1]) * ((x - a[0]) / (b[0] - a[0])).clamp(0., 1.);
        }
        let dx = f64::from(x.clamp(a[0], b[0])) - f64::from(a[0]);
        let c = self.coefficients[i];
        (f64::from(a[1]) + dx * (c[0] + dx * (c[1] + dx * c[2]))).clamp(0., 1.) as f32
    }
    pub fn segments(&self) -> Vec<[[f64; 2]; 4]> {
        self.points
            .windows(2)
            .enumerate()
            .map(|(i, p)| {
                let a = p[0].map(f64::from);
                let b = p[1].map(f64::from);
                let h = b[0] - a[0];
                let (start, end) =
                    if self.mode == ColorInterpolation::NaturalCubic && self.lookup.is_none() {
                        let c = self.coefficients[i];
                        (c[0], c[0] + 2. * c[1] * h + 3. * c[2] * h * h)
                    } else {
                        let slope = (b[1] - a[1]) / h;
                        (slope, slope)
                    };
                [
                    a,
                    [a[0] + h / 3., a[1] + start * h / 3.],
                    [b[0] - h / 3., b[1] - end * h / 3.],
                    b,
                ]
            })
            .collect()
    }
}
