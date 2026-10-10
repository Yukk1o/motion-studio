//! Ordered, editable shape contents. IDs are unique within a vector layer.
use super::*;
use glam::Mat3;

mod sampling;
pub const MAX_DEPTH: usize = 8;
pub const MAX_ITEMS: usize = 512;
pub const MAX_EXPANDED_NODES: usize = 65536;
pub const MAX_PAINT_BATCHES: usize = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Composite {
    #[default]
    Below,
    Above,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupTransform {
    pub position: Track<[f32; 2]>,
    pub anchor: Track<[f32; 2]>,
    pub scale: Track<[f32; 2]>,
    pub rotation: Track<f32>,
    pub skew: Track<f32>,
    pub skew_axis: Track<f32>,
    pub opacity: Track<f32>,
}
impl Default for GroupTransform {
    fn default() -> Self {
        Self {
            position: Track::constant([0.; 2]),
            anchor: Track::constant([0.; 2]),
            scale: Track::constant([100.; 2]),
            rotation: Track::constant(0.),
            skew: Track::constant(0.),
            skew_axis: Track::constant(0.),
            opacity: Track::constant(100.),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repeater {
    pub copies: Track<f32>,
    pub offset: Track<f32>,
    pub position: Track<[f32; 2]>,
    pub anchor: Track<[f32; 2]>,
    pub scale: Track<[f32; 2]>,
    pub rotation: Track<f32>,
    pub start_opacity: Track<f32>,
    pub end_opacity: Track<f32>,
    pub composite: Composite,
}
impl Default for Repeater {
    fn default() -> Self {
        Self {
            copies: Track::constant(3.),
            offset: Track::constant(0.),
            position: Track::constant([100., 0.]),
            anchor: Track::constant([0.; 2]),
            scale: Track::constant([100.; 2]),
            rotation: Track::constant(0.),
            start_opacity: Track::constant(100.),
            end_opacity: Track::constant(100.),
            composite: Composite::Below,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorGroup {
    pub id: u64,
    pub name: String,
    pub transform: GroupTransform,
    pub items: Vec<GroupItem>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroupItem {
    Group {
        group: Box<VectorGroup>,
    },
    Geometry {
        id: u64,
        name: String,
        vector: Box<VectorContent>,
        size: Track<[f32; 2]>,
        position: Track<[f32; 2]>,
    },
    Fill {
        id: u64,
        name: String,
        color: Track<[f32; 4]>,
        fill_rule: FillRule,
        composite: Composite,
    },
    Stroke {
        id: u64,
        name: String,
        stroke: Stroke,
        composite: Composite,
    },
    Trim {
        id: u64,
        name: String,
        trim: TrimPaths,
    },
    Repeater {
        id: u64,
        name: String,
        repeater: Repeater,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct PaintBatch {
    pub vector: Box<SampledVector>,
    /// Paint coordinates to centered layer pixels. Applied after stroke tessellation.
    pub transform: [f32; 9],
    pub scopes: Vec<PaintScope>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PaintScope {
    pub id: u32,
    pub opacity: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    Scalar(f32),
    Vector([f32; 2]),
    Color([f32; 4]),
}

fn vector_valid(t: &Track<[f32; 2]>, min: f32, max: f32) -> Result<()> {
    t.validate_local()?;
    ensure(
        t.axes.is_none(),
        "group tracks do not support separated dimensions",
    )?;
    ensure(
        std::iter::once(t.value)
            .chain(t.keys.iter().map(|k| k.value))
            .flatten()
            .all(|v| (min..=max).contains(&v)),
        "group vector parameter outside valid range",
    )
}
impl GroupTransform {
    fn validate(&self) -> Result<()> {
        vector_valid(&self.position, -32768., 32768.)?;
        vector_valid(&self.anchor, -32768., 32768.)?;
        vector_valid(&self.scale, -10000., 10000.)?;
        scalar_valid(&self.rotation, -360000., 360000., false)?;
        scalar_valid(&self.skew, -89., 89., false)?;
        scalar_valid(&self.skew_axis, -360000., 360000., false)?;
        scalar_valid(&self.opacity, 0., 100., false)
    }
    fn animated(&self) -> bool {
        self.position.is_animated()
            || self.anchor.is_animated()
            || self.scale.is_animated()
            || [&self.rotation, &self.skew, &self.skew_axis, &self.opacity]
                .iter()
                .any(|t| t.is_animated())
    }
}
impl Repeater {
    fn validate(&self) -> Result<()> {
        scalar_valid(&self.copies, 0., 1024., false)?;
        scalar_valid(&self.offset, -1024., 1024., false)?;
        vector_valid(&self.position, -32768., 32768.)?;
        vector_valid(&self.anchor, -32768., 32768.)?;
        vector_valid(&self.scale, -1000., 1000.)?;
        scalar_valid(&self.rotation, -360000., 360000., false)?;
        scalar_valid(&self.start_opacity, 0., 100., false)?;
        scalar_valid(&self.end_opacity, 0., 100., false)
    }
    fn animated(&self) -> bool {
        self.position.is_animated()
            || self.anchor.is_animated()
            || self.scale.is_animated()
            || [
                &self.copies,
                &self.offset,
                &self.rotation,
                &self.start_opacity,
                &self.end_opacity,
            ]
            .iter()
            .any(|t| t.is_animated())
    }
}
impl GroupItem {
    pub fn id(&self) -> u64 {
        match self {
            Self::Group { group } => group.id,
            Self::Geometry { id, .. }
            | Self::Fill { id, .. }
            | Self::Stroke { id, .. }
            | Self::Trim { id, .. }
            | Self::Repeater { id, .. } => *id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Group { group } => &group.name,
            Self::Geometry { name, .. }
            | Self::Fill { name, .. }
            | Self::Stroke { name, .. }
            | Self::Trim { name, .. }
            | Self::Repeater { name, .. } => name,
        }
    }
    fn animated(&self) -> bool {
        match self {
            Self::Group { group } => group.animated(),
            Self::Geometry {
                vector,
                size,
                position,
                ..
            } => vector.animated() || size.is_animated() || position.is_animated(),
            Self::Fill { color, .. } => color.is_animated(),
            Self::Stroke { stroke, .. } => {
                stroke.color.is_animated()
                    || stroke.width.is_animated()
                    || stroke.dashes.as_ref().is_some_and(|d| {
                        d.offset.is_animated() || d.pattern.iter().any(Track::is_animated)
                    })
            }
            Self::Trim { trim, .. } => {
                trim.start.is_animated() || trim.end.is_animated() || trim.offset.is_animated()
            }
            Self::Repeater { repeater, .. } => repeater.animated(),
        }
    }
}
impl VectorGroup {
    pub fn wrap(vector: VectorContent, size: [f32; 2]) -> VectorContent {
        VectorContent {
            source: VectorSource::Group {
                group: Box::new(Self {
                    id: 1,
                    name: "Group 1".into(),
                    transform: GroupTransform::default(),
                    items: vec![GroupItem::Geometry {
                        id: 2,
                        name: "Geometry".into(),
                        vector: Box::new(vector),
                        size: Track::constant(size),
                        position: Track::constant([0.; 2]),
                    }],
                }),
            },
            fill: None,
            stroke: None,
            trim: None,
            fill_rule: FillRule::NonZero,
        }
    }
    pub fn animated(&self) -> bool {
        self.transform.animated() || self.items.iter().any(GroupItem::animated)
    }
    pub fn validate(&self) -> Result<()> {
        fn visit(
            g: &VectorGroup,
            depth: usize,
            ids: &mut HashSet<u64>,
            nodes: &mut usize,
        ) -> Result<()> {
            ensure(depth <= MAX_DEPTH, "vector group nesting limit exceeded")?;
            ensure(
                g.id != 0 && ids.insert(g.id),
                "duplicate or zero vector item ID",
            )?;
            ensure(g.name.len() <= 1024, "vector item name too long")?;
            g.transform.validate()?;
            for item in &g.items {
                if let GroupItem::Group { group } = item {
                    visit(group, depth + 1, ids, nodes)?;
                    continue;
                }
                ensure(
                    item.id() != 0 && ids.insert(item.id()),
                    "duplicate or zero vector item ID",
                )?;
                ensure(item.name().len() <= 1024, "vector item name too long")?;
                match item {
                    GroupItem::Geometry {
                        vector,
                        size,
                        position,
                        ..
                    } => {
                        ensure(
                            !matches!(vector.source, VectorSource::Group { .. }),
                            "nested contents must use a group item",
                        )?;
                        vector.validate()?;
                        vector_valid(size, 0., 32768.)?;
                        vector_valid(position, -32768., 32768.)?;
                        if let VectorSource::Paths { paths } = &vector.source {
                            *nodes += paths.iter().map(|p| p.nodes.len()).sum::<usize>();
                        }
                    }
                    GroupItem::Fill { color, .. } => color_valid(color)?,
                    GroupItem::Stroke { stroke, .. } => VectorContent {
                        source: VectorSource::Paths { paths: vec![] },
                        fill: None,
                        fill_rule: FillRule::NonZero,
                        trim: None,
                        stroke: Some(stroke.clone()),
                    }
                    .validate()?,
                    GroupItem::Trim { trim, .. } => {
                        scalar_valid(&trim.start, 0., 100., false)?;
                        scalar_valid(&trim.end, 0., 100., false)?;
                        scalar_valid(&trim.offset, -360000., 360000., false)?;
                    }
                    GroupItem::Repeater { repeater, .. } => repeater.validate()?,
                    GroupItem::Group { .. } => unreachable!(),
                }
                ensure(
                    ids.len() <= MAX_ITEMS && *nodes <= MAX_NODES,
                    "vector group content limit exceeded",
                )?;
            }
            ensure(ids.len() <= MAX_ITEMS, "vector group item limit exceeded")
        }
        visit(self, 1, &mut HashSet::new(), &mut 0)
    }
    pub fn sample(&self, frame: f64) -> Result<SampledVector> {
        sampling::sample(self, frame)
    }
    pub fn set_parameter(
        &mut self,
        item: u64,
        name: &str,
        frame: i32,
        value: ParameterValue,
        animated: Option<bool>,
    ) -> Result<()> {
        fn discrete(g: &VectorGroup, id: u64, name: &str) -> bool {
            for item in &g.items {
                if let GroupItem::Group { group } = item {
                    if discrete(group, id, name) {
                        return true;
                    }
                } else if item.id() == id {
                    if let GroupItem::Geometry { vector, .. } = item {
                        if let VectorSource::Shape { shape, .. } = &vector.source {
                            return shape_parameters(*shape)
                                .iter()
                                .any(|p| p.discrete && name == format!("shape:{}", p.id));
                        }
                    }
                }
            }
            false
        }
        let hold = discrete(self, item, name);
        enum Channel<'a> {
            Scalar(&'a mut Track<f32>),
            Vector(&'a mut Track<[f32; 2]>),
            Color(&'a mut Track<[f32; 4]>),
        }
        fn transform<'a>(t: &'a mut GroupTransform, name: &str) -> Option<Channel<'a>> {
            Some(match name {
                "position" => Channel::Vector(&mut t.position),
                "anchor" => Channel::Vector(&mut t.anchor),
                "scale" => Channel::Vector(&mut t.scale),
                "rotation" => Channel::Scalar(&mut t.rotation),
                "skew" => Channel::Scalar(&mut t.skew),
                "skew_axis" => Channel::Scalar(&mut t.skew_axis),
                "opacity" => Channel::Scalar(&mut t.opacity),
                _ => return None,
            })
        }
        fn find<'a>(g: &'a mut VectorGroup, id: u64, name: &str) -> Option<Channel<'a>> {
            if g.id == id {
                return transform(&mut g.transform, name);
            }
            for item in &mut g.items {
                if let GroupItem::Group { group } = item {
                    if let Some(c) = find(group, id, name) {
                        return Some(c);
                    }
                    continue;
                }
                if item.id() != id {
                    continue;
                }
                return match item {
                    GroupItem::Geometry {
                        size,
                        position,
                        vector,
                        ..
                    } => match name {
                        "size" => Some(Channel::Vector(size)),
                        "position" => Some(Channel::Vector(position)),
                        "fill" => vector.fill.as_mut().map(Channel::Color),
                        "stroke_color" => {
                            vector.stroke.as_mut().map(|s| Channel::Color(&mut s.color))
                        }
                        "stroke_width" => vector
                            .stroke
                            .as_mut()
                            .map(|s| Channel::Scalar(&mut s.width)),
                        _ if name.starts_with("shape:") => {
                            if let VectorSource::Shape { parameters, .. } = &mut vector.source {
                                parameters
                                    .get_mut(name.trim_start_matches("shape:"))
                                    .map(Channel::Scalar)
                            } else {
                                None
                            }
                        }
                        _ => vector.modifier_track_mut(name).ok().map(Channel::Scalar),
                    },
                    GroupItem::Fill { color, .. } => {
                        (name == "color").then_some(Channel::Color(color))
                    }
                    GroupItem::Stroke { stroke, .. } => match name {
                        "color" => Some(Channel::Color(&mut stroke.color)),
                        "width" => Some(Channel::Scalar(&mut stroke.width)),
                        _ => None,
                    },
                    GroupItem::Trim { trim, .. } => match name {
                        "start" => Some(Channel::Scalar(&mut trim.start)),
                        "end" => Some(Channel::Scalar(&mut trim.end)),
                        "offset" => Some(Channel::Scalar(&mut trim.offset)),
                        _ => None,
                    },
                    GroupItem::Repeater { repeater: r, .. } => match name {
                        "copies" => Some(Channel::Scalar(&mut r.copies)),
                        "offset" => Some(Channel::Scalar(&mut r.offset)),
                        "rotation" => Some(Channel::Scalar(&mut r.rotation)),
                        "start_opacity" => Some(Channel::Scalar(&mut r.start_opacity)),
                        "end_opacity" => Some(Channel::Scalar(&mut r.end_opacity)),
                        "position" => Some(Channel::Vector(&mut r.position)),
                        "anchor" => Some(Channel::Vector(&mut r.anchor)),
                        "scale" => Some(Channel::Vector(&mut r.scale)),
                        _ => None,
                    },
                    GroupItem::Group { .. } => unreachable!(),
                };
            }
            None
        }
        let channel = find(self, item, name).ok_or_else(|| {
            crate::Error::Invalid(format!("vector group {item} parameter missing: {name}"))
        })?;
        match (channel, value) {
            (Channel::Scalar(t), ParameterValue::Scalar(v)) => {
                if let Some(a) = animated {
                    t.set_animated(frame, a)?;
                }
                t.set_at(frame, v)?;
                if hold {
                    for key in &mut t.keys {
                        key.ease = Ease::Hold;
                        key.curve = None;
                    }
                }
            }
            (Channel::Vector(t), ParameterValue::Vector(v)) => {
                if let Some(a) = animated {
                    t.set_animated(frame, a)?;
                }
                t.set_at(frame, v)?;
            }
            (Channel::Color(t), ParameterValue::Color(v)) => {
                if let Some(a) = animated {
                    t.set_animated(frame, a)?;
                }
                t.set_at(frame, v)?;
            }
            _ => {
                return Err(crate::Error::Invalid(
                    "vector group parameter dimensions mismatch".into(),
                ))
            }
        }
        self.validate()
    }
}
