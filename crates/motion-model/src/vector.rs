//! Native paths are stored in canvas pixels, centered at (0, 0), Y down.
use crate::{ensure, Ease, Result, Track};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MAX_PATHS: usize = 64;
pub const MAX_NODES: usize = 2048;
pub mod path_ops;
pub mod groups;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimMode {
    Simultaneously,
    Individually,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrimPaths {
    pub start: Track<f32>,
    pub end: Track<f32>,
    pub offset: Track<f32>,
    pub mode: TrimMode,
}
impl Default for TrimPaths {
    fn default() -> Self {
        Self {
            start: Track::constant(0.),
            end: Track::constant(100.),
            offset: Track::constant(0.),
            mode: TrimMode::Simultaneously,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeDashes {
    /// Alternating positive dash and nonnegative gap lengths, in canvas pixels.
    pub pattern: Vec<Track<f32>>,
    pub offset: Track<f32>,
}
impl Default for StrokeDashes {
    fn default() -> Self {
        Self {
            pattern: vec![Track::constant(12.), Track::constant(8.)],
            offset: Track::constant(0.),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct SampledTrimPaths {
    pub start: f32,
    pub end: f32,
    pub offset: f32,
    pub mode: TrimMode,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SampledDashes {
    pub pattern: Vec<f32>,
    pub offset: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Circle,
    Triangle,
    Polygon,
    Pentagon,
    Hexagon,
    Octagon,
    Star,
    Ring,
    Arc,
    Sector,
    Line,
    Arrow,
    DoubleArrow,
    Plus,
    Heart,
    Teardrop,
    Trapezoid,
    Parallelogram,
    Diamond,
    Chevron,
    Gear,
    Flower,
}
pub const SHAPES: [(ShapeKind, &str, &str, &str); 25] = [
    (ShapeKind::Rectangle, "矩形", "Rectangle", "basic"),
    (
        ShapeKind::RoundedRectangle,
        "圆角矩形",
        "Rounded Rectangle",
        "basic",
    ),
    (ShapeKind::Ellipse, "椭圆", "Ellipse", "basic"),
    (ShapeKind::Circle, "圆形", "Circle", "basic"),
    (ShapeKind::Triangle, "三角形", "Triangle", "polygons"),
    (ShapeKind::Polygon, "多边形", "Polygon", "polygons"),
    (ShapeKind::Pentagon, "五边形", "Pentagon", "polygons"),
    (ShapeKind::Hexagon, "六边形", "Hexagon", "polygons"),
    (ShapeKind::Octagon, "八边形", "Octagon", "polygons"),
    (ShapeKind::Star, "星形", "Star", "symbols"),
    (ShapeKind::Ring, "圆环", "Ring", "curves"),
    (ShapeKind::Arc, "圆弧", "Arc", "curves"),
    (ShapeKind::Sector, "扇形", "Sector", "curves"),
    (ShapeKind::Line, "线段", "Line", "basic"),
    (ShapeKind::Arrow, "箭头", "Arrow", "symbols"),
    (
        ShapeKind::DoubleArrow,
        "双向箭头",
        "Double Arrow",
        "symbols",
    ),
    (ShapeKind::Plus, "十字", "Plus", "symbols"),
    (ShapeKind::Heart, "爱心", "Heart", "symbols"),
    (ShapeKind::Teardrop, "水滴", "Teardrop", "curves"),
    (ShapeKind::Trapezoid, "梯形", "Trapezoid", "polygons"),
    (
        ShapeKind::Parallelogram,
        "平行四边形",
        "Parallelogram",
        "polygons",
    ),
    (ShapeKind::Diamond, "菱形", "Diamond", "polygons"),
    (ShapeKind::Chevron, "折箭头", "Chevron", "symbols"),
    (ShapeKind::Gear, "齿轮", "Gear", "symbols"),
    (ShapeKind::Flower, "花形", "Flower", "curves"),
];
#[derive(Clone, Debug, Serialize)]
pub struct ShapeParameter {
    pub id: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub discrete: bool,
}
fn parameter(
    id: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
    discrete: bool,
) -> ShapeParameter {
    ShapeParameter {
        id,
        min,
        max,
        default,
        unit,
        discrete,
    }
}
pub fn shape_parameters(kind: ShapeKind) -> Vec<ShapeParameter> {
    use ShapeKind::*;
    let mut p = Vec::new();
    if matches!(kind, RoundedRectangle) {
        p.push(parameter("corner_ratio", 0., 1., 0.25, "ratio", false));
    }
    if matches!(kind, Polygon | Star | Gear | Flower) {
        p.push(parameter(
            "points",
            3.,
            32.,
            if kind == Gear {
                12.
            } else if kind == Polygon {
                6.
            } else {
                5.
            },
            "count",
            true,
        ));
    }
    if matches!(kind, Star | Ring | Gear | Flower) {
        p.push(parameter(
            "inner_ratio",
            0.01,
            0.99,
            if kind == Gear { 0.75 } else { 0.5 },
            "ratio",
            false,
        ));
    }
    if matches!(kind, Arrow | DoubleArrow | Plus | Chevron) {
        p.push(parameter("shaft_ratio", 0.01, 0.99, 0.3, "ratio", false));
    }
    if matches!(kind, Arc | Sector) {
        p.push(parameter(
            "start_angle",
            -3600.,
            3600.,
            -90.,
            "degrees",
            false,
        ));
        p.push(parameter(
            "sweep_angle",
            -360.,
            360.,
            270.,
            "degrees",
            false,
        ));
    }
    p.push(parameter("angle", -3600., 3600., 0., "degrees", false));
    p
}
pub fn shape_catalog() -> serde_json::Value {
    serde_json::Value::Array(SHAPES.iter().map(|(kind,zh,en,category)| serde_json::json!({"id":kind,"name":zh,"english_name":en,"category":category,"parameters":shape_parameters(*kind)})).collect())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathNode {
    pub id: u64,
    /// x, y, relative incoming tangent x/y, relative outgoing tangent x/y.
    pub geometry: Track<[f32; 6]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorPath {
    pub id: u64,
    pub closed: bool,
    pub nodes: Vec<PathNode>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorSource {
    Group { group: Box<groups::VectorGroup> },
    Paths {
        paths: Vec<VectorPath>,
    },
    Shape {
        shape: ShapeKind,
        parameters: BTreeMap<String, Track<f32>>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    NonZero,
    EvenOdd,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineCap {
    Butt,
    Round,
    Square,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: Track<[f32; 4]>,
    pub width: Track<f32>,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashes: Option<StrokeDashes>,
}
impl Stroke {
    fn dash_track_mut(&mut self, parameter: &str) -> Result<&mut Track<f32>> {
        if let Some(d) = &mut self.dashes {
            if parameter == "dash_offset" {
                return Ok(&mut d.offset);
            }
            if let Some(i) = parameter.strip_prefix("dash_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(t) = d.pattern.get_mut(i) {
                    return Ok(t);
                }
            }
        }
        Err(crate::Error::Invalid(format!("vector modifier parameter missing: {parameter}")))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorContent {
    pub source: VectorSource,
    pub fill: Option<Track<[f32; 4]>>,
    pub fill_rule: FillRule,
    pub stroke: Option<Stroke>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<TrimPaths>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SampledPath {
    pub closed: bool,
    pub nodes: Vec<[f32; 6]>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SampledVector {
    pub paths: Vec<SampledPath>,
    pub fill: Option<[f32; 4]>,
    pub fill_rule: FillRule,
    pub stroke: Option<([f32; 4], f32, LineCap, LineJoin, f32)>,
    pub trim: Option<SampledTrimPaths>,
    pub dashes: Option<SampledDashes>,
    pub batches: Option<Vec<groups::PaintBatch>>,
    pub root_opacity: f32,
    pub group_parameters: BTreeMap<u64, BTreeMap<String, serde_json::Value>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorAction {
    ConvertToGroup,
    SetGroupParameter { item: u64, parameter: String, frame: u32,
        value: groups::ParameterValue, #[serde(default)] animated: Option<bool> },
    SetTrim {
        trim: Option<TrimPaths>,
    },
    SetDashes {
        dashes: Option<StrokeDashes>,
    },
    SetModifierParameter {
        parameter: String,
        frame: u32,
        value: f32,
        #[serde(default)]
        animated: Option<bool>,
    },
    SetModifierCurve {
        parameter: String,
        frame: u32,
        easing: crate::Easing,
    },
    Replace {
        vector: VectorContent,
    },
    SetPaths {
        paths: Vec<VectorPath>,
    },
    SetNode {
        path: u64,
        node: u64,
        frame: u32,
        value: [f32; 6],
        animated: bool,
    },
    SetParameter {
        parameter: String,
        frame: u32,
        value: f32,
        animated: bool,
    },
    SetPaint {
        fill: Option<Track<[f32; 4]>>,
        fill_rule: FillRule,
        stroke: Option<Stroke>,
    },
    ConvertToPath {
        frame: u32,
    },
}
fn scalar_valid(track: &Track<f32>, min: f32, max: f32, discrete: bool) -> Result<()> {
    track.validate_local()?;
    ensure(
        track.axes.is_none(),
        "vector parameters do not support separated dimensions",
    )?;
    for v in std::iter::once(track.value).chain(track.keys.iter().map(|k| k.value)) {
        ensure(
            (min..=max).contains(&v) && (!discrete || v.fract() == 0.),
            "vector parameter outside valid range",
        )?;
    }
    ensure(
        !discrete
            || track
                .keys
                .iter()
                .all(|k| k.ease == Ease::Hold && k.curve.is_none()),
        "discrete shape parameters require hold keyframes",
    )
}
fn color_valid(track: &Track<[f32; 4]>) -> Result<()> {
    track.validate_local()?;
    for v in std::iter::once(track.value).chain(track.keys.iter().map(|k| k.value)) {
        ensure(
            v.iter().all(|x| (0.0..=1.0).contains(x)),
            "vector color outside zero to one",
        )?;
    }
    Ok(())
}
impl VectorContent {
    pub fn required_format(&self) -> u32 {
        if matches!(self.source, VectorSource::Group { .. }) { 12 }
        else if self.has_path_modifiers() { 11 } else { 6 }
    }
    pub fn has_path_modifiers(&self) -> bool {
        self.trim.is_some() || self.stroke.as_ref().is_some_and(|s| s.dashes.is_some())
    }
    fn modifier_track_mut(&mut self, parameter: &str) -> Result<&mut Track<f32>> {
        if let Some(t) = &mut self.trim {
            match parameter {
                "trim_start" => return Ok(&mut t.start),
                "trim_end" => return Ok(&mut t.end),
                "trim_offset" => return Ok(&mut t.offset),
                _ => {}
            }
        }
        if let Some(stroke) = &mut self.stroke {
            return stroke.dash_track_mut(parameter);
        }
        Err(crate::Error::Invalid(format!(
            "vector modifier parameter missing: {parameter}"
        )))
    }
    pub fn animated(&self) -> bool {
        self.trim
            .as_ref()
            .is_some_and(|t| t.start.is_animated() || t.end.is_animated() || t.offset.is_animated())
            || self
                .stroke
                .as_ref()
                .and_then(|s| s.dashes.as_ref())
                .is_some_and(|d| d.offset.is_animated() || d.pattern.iter().any(Track::is_animated))
            || self.fill.as_ref().is_some_and(|t| t.is_animated())
            || self
                .stroke
                .as_ref()
                .is_some_and(|s| s.color.is_animated() || s.width.is_animated())
            || match &self.source {
                VectorSource::Group { group } => group.animated(),
                VectorSource::Paths { paths } => paths
                    .iter()
                    .flat_map(|p| &p.nodes)
                    .any(|n| n.geometry.is_animated()),
                VectorSource::Shape { parameters, .. } => {
                    parameters.values().any(|t| t.is_animated())
                }
            }
    }
    pub fn shape(shape: ShapeKind) -> Self {
        Self {
            trim: None,
            source: VectorSource::Shape {
                shape,
                parameters: shape_parameters(shape)
                    .into_iter()
                    .map(|p| (p.id.into(), Track::constant(p.default)))
                    .collect(),
            },
            fill: if matches!(shape, ShapeKind::Line | ShapeKind::Arc) {
                None
            } else {
                Some(Track::constant([1.; 4]))
            },
            fill_rule: FillRule::EvenOdd,
            stroke: if matches!(shape, ShapeKind::Line | ShapeKind::Arc) {
                Some(Stroke {
                    color: Track::constant([1.; 4]),
                    width: Track::constant(4.),
                    cap: LineCap::Round,
                    join: LineJoin::Round,
                    miter_limit: 4.,
                    dashes: None,
                })
            } else {
                None
            },
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(t) = &self.trim {
            scalar_valid(&t.start, 0., 100., false)?;
            scalar_valid(&t.end, 0., 100., false)?;
            scalar_valid(&t.offset, -360000., 360000., false)?;
        }
        if let Some(fill) = &self.fill {
            color_valid(fill)?;
        }
        if let Some(stroke) = &self.stroke {
            if let Some(d) = &stroke.dashes {
                ensure(
                    matches!(d.pattern.len(), 2 | 4 | 6),
                    "dashes require one to three dash/gap pairs",
                )?;
                for (i, t) in d.pattern.iter().enumerate() {
                    scalar_valid(t, if i % 2 == 0 { 0.1 } else { 0. }, 32768., false)?;
                }
                scalar_valid(&d.offset, -32768., 32768., false)?;
            }
            color_valid(&stroke.color)?;
            scalar_valid(&stroke.width, 0., 4096., false)?;
            ensure(
                stroke.miter_limit.is_finite() && (1.0..=100.0).contains(&stroke.miter_limit),
                "invalid stroke miter limit",
            )?;
        }
        match &self.source {
            VectorSource::Group { group } => {
                ensure(self.fill.is_none() && self.stroke.is_none() && self.trim.is_none(), "group paints and modifiers must be stored inside the group")?;
                group.validate()?;
            }
            VectorSource::Paths { paths } => {
                ensure(paths.len() <= MAX_PATHS, "too many vector paths")?;
                let mut ids = HashSet::new();
                let mut count = 0;
                for p in paths {
                    ensure(
                        p.id != 0 && ids.insert(p.id),
                        "duplicate or zero vector path ID",
                    )?;
                    ensure(
                        !p.closed || p.nodes.len() >= 3,
                        "closed paths require at least three nodes",
                    )?;
                    let mut nodes = HashSet::new();
                    for n in &p.nodes {
                        ensure(n.id != 0 && nodes.insert(n.id), "duplicate or zero node ID")?;
                        n.geometry.validate_local()?;
                        n.geometry.validate_bound(32768.)?;
                    }
                    count += p.nodes.len();
                }
                ensure(count <= MAX_NODES, "too many vector nodes")?;
            }
            VectorSource::Shape { shape, parameters } => {
                let schema = shape_parameters(*shape);
                ensure(
                    parameters.len() == schema.len(),
                    "shape parameter schema mismatch",
                )?;
                for p in schema {
                    scalar_valid(
                        parameters.get(p.id).ok_or_else(|| {
                            crate::Error::Invalid(format!("missing shape parameter {}", p.id))
                        })?,
                        p.min,
                        p.max,
                        p.discrete,
                    )?;
                }
            }
        }
        Ok(())
    }
    pub fn sample(&self, frame: f64, size: [f32; 2]) -> Result<SampledVector> {
        if let VectorSource::Group { group } = &self.source { return group.sample(frame); }
        let paths = match &self.source {
            VectorSource::Group { .. } => unreachable!(),
            VectorSource::Paths { paths } => paths
                .iter()
                .map(|p| SampledPath {
                    closed: p.closed,
                    nodes: p.nodes.iter().map(|n| n.geometry.sample(frame)).collect(),
                })
                .collect(),
            VectorSource::Shape { shape, parameters } => {
                let mut values = BTreeMap::new();
                for p in shape_parameters(*shape) {
                    let v = parameters[p.id].sample(frame);
                    ensure(
                        (p.min..=p.max).contains(&v) && (!p.discrete || v.fract() == 0.),
                        "sampled shape parameter outside valid range",
                    )?;
                    values.insert(p.id, v);
                }
                shape_paths(*shape, size, &values)
            }
        };
        let sampled = SampledVector {
            batches: None,
            root_opacity: 1.,
            group_parameters: BTreeMap::new(),
            trim: self.trim.as_ref().map(|t| SampledTrimPaths {
                start: t.start.sample(frame),
                end: t.end.sample(frame),
                offset: t.offset.sample(frame),
                mode: t.mode,
            }),
            dashes: self
                .stroke
                .as_ref()
                .and_then(|s| s.dashes.as_ref())
                .map(|d| SampledDashes {
                    pattern: d.pattern.iter().map(|t| t.sample(frame)).collect(),
                    offset: d.offset.sample(frame),
                }),
            paths,
            fill: self.fill.as_ref().map(|f| f.sample(frame)),
            fill_rule: self.fill_rule,
            stroke: self.stroke.as_ref().map(|s| {
                (
                    s.color.sample(frame),
                    s.width.sample(frame),
                    s.cap,
                    s.join,
                    s.miter_limit,
                )
            }),
        };
        ensure(
            sampled
                .paths
                .iter()
                .flat_map(|p| &p.nodes)
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 131072.),
            "sampled path exceeds coordinate range",
        )?;
        ensure(
            sampled
                .fill
                .into_iter()
                .chain(sampled.stroke.map(|s| s.0))
                .flatten()
                .all(|v| (0.0..=1.0).contains(&v)),
            "sampled vector color outside valid range",
        )?;
        ensure(
            sampled.stroke.is_none_or(|s| (0.0..=4096.0).contains(&s.1)),
            "sampled stroke width outside valid range",
        )?;
        if let Some(t) = &sampled.trim {
            ensure(
                (0.0..=100.).contains(&t.start)
                    && (0.0..=100.).contains(&t.end)
                    && (-360000.0..=360000.).contains(&t.offset),
                "sampled trim parameter outside valid range",
            )?;
        }
        if let Some(d) = &sampled.dashes {
            ensure(
                d.pattern
                    .iter()
                    .enumerate()
                    .all(|(i, v)| (if i % 2 == 0 { 0.1 } else { 0. }..=32768.).contains(v))
                    && (-32768.0..=32768.).contains(&d.offset),
                "sampled dash parameter outside valid range",
            )?;
        }
        Ok(sampled)
    }
    pub fn edit(&mut self, action: VectorAction, size: [f32; 2], offset: i32) -> Result<()> {
        let local = |f: u32| {
            i32::try_from(i64::from(f) - i64::from(offset))
                .map_err(|_| crate::Error::Invalid("vector local frame overflow".into()))
        };
        match action {
            VectorAction::ConvertToGroup => {
                ensure(!matches!(self.source, VectorSource::Group { .. }), "already a vector group")?;
                let content = self.clone();
                *self = groups::VectorGroup::wrap(content, size);
            }
            VectorAction::SetGroupParameter { item, parameter, frame, value, animated } => {
                let VectorSource::Group { group } = &mut self.source else { return Err(crate::Error::Invalid("not a vector group".into())); };
                group.set_parameter(item, &parameter, local(frame)?, value, animated)?;
            }
            VectorAction::SetTrim { trim } => self.trim = trim,
            VectorAction::SetDashes { dashes } => {
                let stroke = self.stroke.as_mut().ok_or_else(|| {
                    crate::Error::Invalid("add a stroke before setting dashes".into())
                })?;
                stroke.dashes = dashes;
            }
            VectorAction::SetModifierParameter {
                parameter,
                frame,
                value,
                animated,
            } => {
                let t = self.modifier_track_mut(&parameter)?;
                if let Some(enabled) = animated {
                    t.set_animated(local(frame)?, enabled)?;
                }
                t.set_at(local(frame)?, value)?;
            }
            VectorAction::SetModifierCurve {
                parameter,
                frame,
                easing,
            } => {
                self.modifier_track_mut(&parameter)?
                    .set_curve(local(frame)?, easing)?;
            }
            VectorAction::Replace { vector } => *self = vector,
            VectorAction::SetPaths { paths } => self.source = VectorSource::Paths { paths },
            VectorAction::SetPaint {
                fill,
                fill_rule,
                stroke,
            } => {
                self.fill = fill;
                self.fill_rule = fill_rule;
                self.stroke = stroke;
            }
            VectorAction::SetNode {
                path,
                node,
                frame,
                value,
                animated,
            } => {
                let VectorSource::Paths { paths } = &mut self.source else {
                    return Err(crate::Error::Invalid(
                        "convert shape to path before editing nodes".into(),
                    ));
                };
                let n = paths
                    .iter_mut()
                    .find(|p| p.id == path)
                    .and_then(|p| p.nodes.iter_mut().find(|n| n.id == node))
                    .ok_or_else(|| crate::Error::Invalid("vector node missing".into()))?;
                n.geometry.set_animated(local(frame)?, animated)?;
                n.geometry.set_at(local(frame)?, value)?;
            }
            VectorAction::SetParameter {
                parameter,
                frame,
                value,
                animated,
            } => {
                let VectorSource::Shape { shape, parameters } = &mut self.source else {
                    return Err(crate::Error::Invalid("not a parametric shape".into()));
                };
                let p = shape_parameters(*shape)
                    .into_iter()
                    .find(|p| p.id == parameter)
                    .ok_or_else(|| crate::Error::Invalid("shape parameter missing".into()))?;
                let t = parameters.get_mut(&parameter).unwrap();
                t.set_animated(local(frame)?, animated)?;
                t.set_at(local(frame)?, value)?;
                if p.discrete {
                    for key in &mut t.keys {
                        key.ease = Ease::Hold;
                        key.curve = None;
                    }
                }
            }
            VectorAction::ConvertToPath { frame } => {
                ensure(
                    matches!(self.source, VectorSource::Shape { .. }),
                    "already an editable path",
                )?;
                let paths = self
                    .sample(f64::from(local(frame)?), size)?
                    .paths
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| VectorPath {
                        id: i as u64 + 1,
                        closed: p.closed,
                        nodes: p
                            .nodes
                            .into_iter()
                            .enumerate()
                            .map(|(j, g)| PathNode {
                                id: j as u64 + 1,
                                geometry: Track::constant(g),
                            })
                            .collect(),
                    })
                    .collect();
                self.source = VectorSource::Paths { paths };
            }
        }
        self.validate()
    }
}

fn node(x: f32, y: f32) -> [f32; 6] {
    [x, y, 0., 0., 0., 0.]
}
fn polygon(points: &[[f32; 2]], closed: bool) -> SampledPath {
    SampledPath {
        closed,
        nodes: points.iter().map(|p| node(p[0], p[1])).collect(),
    }
}
fn ellipse(rx: f32, ry: f32, start: f32, sweep: f32, closed: bool) -> SampledPath {
    let segments = (sweep.abs() / 90.).ceil().max(1.) as usize;
    let step = sweep.to_radians() / segments as f32;
    let mut nodes = Vec::new();
    for i in 0..=segments {
        let a = start.to_radians() + step * i as f32;
        let k = 4. / 3. * (step / 4.).tan();
        nodes.push([
            rx * a.cos(),
            ry * a.sin(),
            k * rx * a.sin(),
            -k * ry * a.cos(),
            -k * rx * a.sin(),
            k * ry * a.cos(),
        ]);
    }
    if closed {
        nodes.pop();
    } else {
        nodes[0][2] = 0.;
        nodes[0][3] = 0.;
        let n = nodes.last_mut().unwrap();
        n[4] = 0.;
        n[5] = 0.;
    }
    SampledPath { closed, nodes }
}
fn shape_paths(kind: ShapeKind, size: [f32; 2], p: &BTreeMap<&str, f32>) -> Vec<SampledPath> {
    use ShapeKind::*;
    let (mut x, mut y) = (size[0] * 0.5, size[1] * 0.5);
    if kind == Circle {
        x = x.min(y);
        y = x;
    }
    let get = |id: &str, default: f32| p.get(id).copied().unwrap_or(default);
    let mut paths = match kind {
        Ellipse | Circle => vec![ellipse(x, y, -90., 360., true)],
        Ring => {
            let r = get("inner_ratio", 0.5);
            vec![
                ellipse(x, y, -90., 360., true),
                ellipse(x * r, y * r, -90., -360., true),
            ]
        }
        Arc | Sector => {
            let sweep = get("sweep_angle", 270.);
            if sweep.abs() < 0.0001 {
                Vec::new()
            } else if sweep.abs() == 360. {
                vec![ellipse(x, y, get("start_angle", -90.), sweep, true)]
            } else {
                let mut path = ellipse(x, y, get("start_angle", -90.), sweep, false);
                if kind == Sector {
                    path.nodes.push(node(0., 0.));
                    path.closed = true;
                }
                vec![path]
            }
        }
        RoundedRectangle => {
            let r = x.min(y) * get("corner_ratio", 0.25);
            let k = 0.55228475 * r;
            vec![SampledPath {
                closed: true,
                nodes: vec![
                    [-x + r, -y, -k, 0., 0., 0.],
                    [x - r, -y, 0., 0., k, 0.],
                    [x, -y + r, 0., -k, 0., 0.],
                    [x, y - r, 0., 0., 0., k],
                    [x - r, y, k, 0., 0., 0.],
                    [-x + r, y, 0., 0., -k, 0.],
                    [-x, y - r, 0., k, 0., 0.],
                    [-x, -y + r, 0., 0., 0., -k],
                ],
            }]
        }
        Triangle | Polygon | Pentagon | Hexagon | Octagon | Star | Gear | Flower => {
            let n = match kind {
                Triangle => 3,
                Pentagon => 5,
                Hexagon => 6,
                Octagon => 8,
                _ => get("points", 5.) as usize,
            };
            let alternating = matches!(kind, Star | Gear | Flower);
            let count = if kind == Gear {
                n * 4
            } else if alternating {
                n * 2
            } else {
                n
            };
            let points = (0..count)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::TAU * i as f32 / count as f32;
                    let r = if kind == Gear {
                        if i % 4 < 2 {
                            1.
                        } else {
                            get("inner_ratio", 0.75)
                        }
                    } else if alternating && i % 2 == 1 {
                        get("inner_ratio", 0.5)
                    } else {
                        1.
                    };
                    [x * r * a.cos(), y * r * a.sin()]
                })
                .collect::<Vec<_>>();
            let mut path = polygon(&points, true);
            if kind == Flower {
                let len = path.nodes.len();
                for i in 0..len {
                    let prev = points[(i + len - 1) % len];
                    let next = points[(i + 1) % len];
                    let dx = (next[0] - prev[0]) * 0.17;
                    let dy = (next[1] - prev[1]) * 0.17;
                    path.nodes[i][2] = -dx;
                    path.nodes[i][3] = -dy;
                    path.nodes[i][4] = dx;
                    path.nodes[i][5] = dy;
                }
            }
            vec![path]
        }
        Line => vec![polygon(&[[-x * 0.9, 0.], [x * 0.9, 0.]], false)],
        Arrow => {
            let t = y * get("shaft_ratio", 0.3);
            vec![polygon(
                &[
                    [-x, -t],
                    [x * 0.3, -t],
                    [x * 0.3, -y],
                    [x, 0.],
                    [x * 0.3, y],
                    [x * 0.3, t],
                    [-x, t],
                ],
                true,
            )]
        }
        DoubleArrow => {
            let t = y * get("shaft_ratio", 0.3);
            vec![polygon(
                &[
                    [-x, 0.],
                    [-x * 0.3, -y],
                    [-x * 0.3, -t],
                    [x * 0.3, -t],
                    [x * 0.3, -y],
                    [x, 0.],
                    [x * 0.3, y],
                    [x * 0.3, t],
                    [-x * 0.3, t],
                    [-x * 0.3, y],
                ],
                true,
            )]
        }
        Plus => {
            let a = x * get("shaft_ratio", 0.3);
            let b = y * get("shaft_ratio", 0.3);
            vec![polygon(
                &[
                    [-a, -y],
                    [a, -y],
                    [a, -b],
                    [x, -b],
                    [x, b],
                    [a, b],
                    [a, y],
                    [-a, y],
                    [-a, b],
                    [-x, b],
                    [-x, -b],
                    [-a, -b],
                ],
                true,
            )]
        }
        Heart => vec![SampledPath {
            closed: true,
            nodes: vec![
                [0., -y * 0.35, -x * 0.55, -y, x * 0.55, -y],
                [x * 0.95, -y * 0.1, 0., -y * 1.0, 0., y * 0.45],
                [0., y, x * 0.3, -y * 0.3, -x * 0.3, -y * 0.3],
                [-x * 0.95, -y * 0.1, 0., y * 0.45, 0., -y * 1.0],
            ],
        }],
        Teardrop => vec![SampledPath {
            closed: true,
            nodes: vec![
                [0., -y, 0., 0., x * 0.1, y * 0.55],
                [x * 0.9, y * 0.25, 0., -y * 0.5, 0., y * 0.9],
                [-x * 0.9, y * 0.25, 0., y * 0.9, 0., -y * 0.5],
            ],
        }],
        Trapezoid => vec![polygon(
            &[[-x * 0.55, -y], [x * 0.55, -y], [x, y], [-x, y]],
            true,
        )],
        Parallelogram => vec![polygon(
            &[[-x * 0.5, -y], [x, -y], [x * 0.5, y], [-x, y]],
            true,
        )],
        Diamond => vec![polygon(&[[0., -y], [x, 0.], [0., y], [-x, 0.]], true)],
        Chevron => {
            let t = get("shaft_ratio", 0.3) * x * 2.;
            vec![polygon(
                &[
                    [-x, -y],
                    [-x + t, -y],
                    [x, 0.],
                    [-x + t, y],
                    [-x, y],
                    [x - t, 0.],
                ],
                true,
            )]
        }
        Rectangle => vec![polygon(&[[-x, -y], [x, -y], [x, y], [-x, y]], true)],
    };
    let a = get("angle", 0.).to_radians();
    let (c, s) = (a.cos(), a.sin());
    for path in &mut paths {
        for n in &mut path.nodes {
            for i in [0, 2, 4] {
                let (px, py) = (n[i], n[i + 1]);
                n[i] = px * c - py * s;
                n[i + 1] = px * s + py * c;
            }
        }
    }
    paths
}
