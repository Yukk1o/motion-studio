use super::*;
use glam::Vec2;
use serde_json::json;

#[derive(Clone)]
struct Chunk {
    paths: Vec<SampledPath>,
    opacity: f32,
}
#[derive(Clone)]
struct Paint {
    chunks: Vec<usize>,
    vector: SampledVector,
    basis: Mat3,
    scopes: Vec<PaintScope>,
}
#[derive(Default)]
struct State {
    chunks: Vec<Chunk>,
    paints: Vec<Paint>,
    parameters: BTreeMap<u64, BTreeMap<String, serde_json::Value>>,
}
fn scalar(t: &Track<f32>, f: f64, lo: f32, hi: f32) -> Result<f32> {
    let v = t.sample(f);
    ensure(
        (lo..=hi).contains(&v),
        "sampled group scalar outside valid range",
    )?;
    Ok(v)
}
fn vector(t: &Track<[f32; 2]>, f: f64, lo: f32, hi: f32) -> Result<Vec2> {
    let v = Vec2::from_array(t.sample(f));
    ensure(
        v.to_array().iter().all(|v| (lo..=hi).contains(v)),
        "sampled group vector outside valid range",
    )?;
    Ok(v)
}
fn color(t: &Track<[f32; 4]>, f: f64) -> Result<[f32; 4]> {
    let v = t.sample(f);
    ensure(
        v.iter().all(|v| (0.0..=1.).contains(v)),
        "sampled group color outside valid range",
    )?;
    Ok(v)
}
fn paths_transform(paths: &mut [SampledPath], matrix: Mat3) -> Result<()> {
    for n in paths.iter_mut().flat_map(|p| &mut p.nodes) {
        let point = matrix.transform_point2(Vec2::new(n[0], n[1]));
        let a = matrix.transform_vector2(Vec2::new(n[2], n[3]));
        let b = matrix.transform_vector2(Vec2::new(n[4], n[5]));
        *n = [point.x, point.y, a.x, a.y, b.x, b.y];
        ensure(
            n.iter().all(|v| v.is_finite() && v.abs() <= 131072.),
            "group output exceeds coordinate range",
        )?;
    }
    Ok(())
}
fn matrix(
    position: Vec2,
    anchor: Vec2,
    scale: Vec2,
    rotation: f32,
    skew: f32,
    skew_axis: f32,
) -> Mat3 {
    let shear = Mat3::from_cols(
        Vec2::X.extend(0.),
        Vec2::new(skew.to_radians().tan(), 1.).extend(0.),
        glam::Vec3::Z,
    );
    Mat3::from_translation(position)
        * Mat3::from_angle(rotation.to_radians())
        * Mat3::from_angle(skew_axis.to_radians())
        * shear
        * Mat3::from_angle(-skew_axis.to_radians())
        * Mat3::from_scale(scale / 100.)
        * Mat3::from_translation(-anchor)
}
fn empty_paint() -> SampledVector {
    SampledVector {
        paths: vec![],
        fill: None,
        fill_rule: FillRule::NonZero,
        stroke: None,
        trim: None,
        dashes: None,
        batches: None,
        root_opacity: 1.,
        group_parameters: BTreeMap::new(),
    }
}
fn closed_fill() -> SampledTrimPaths {
    SampledTrimPaths {
        start: 0.,
        end: 100.,
        offset: 0.,
        mode: TrimMode::Simultaneously,
    }
}
impl State {
    fn check_budget(&self) -> Result<()> {
        ensure(
            self.paints.len() <= MAX_PAINT_BATCHES && self.chunks.len() <= 4096,
            "vector group paint or instance limit exceeded",
        )?;
        ensure(
            self.chunks
                .iter()
                .flat_map(|c| &c.paths)
                .map(|p| p.nodes.len())
                .sum::<usize>()
                <= MAX_EXPANDED_NODES,
            "vector group expanded node limit exceeded",
        )
    }
    fn transform(&mut self, matrix: Mat3, opacity: f32) -> Result<()> {
        for c in &mut self.chunks {
            paths_transform(&mut c.paths, matrix)?;
            c.opacity *= opacity;
        }
        for p in &mut self.paints {
            p.basis = matrix * p.basis;
        }
        Ok(())
    }
    fn append(&mut self, mut child: State) -> Result<()> {
        let offset = self.chunks.len();
        for p in &mut child.paints {
            for id in &mut p.chunks {
                *id += offset;
            }
        }
        self.chunks.extend(child.chunks);
        self.paints.extend(child.paints);
        self.parameters.extend(child.parameters);
        self.check_budget()
    }
    fn paint(&mut self, vector: SampledVector, composite: Composite) -> Result<()> {
        let paint = Paint {
            chunks: (0..self.chunks.len()).collect(),
            vector,
            basis: Mat3::IDENTITY,
            scopes: vec![],
        };
        if composite == Composite::Below {
            self.paints.insert(0, paint);
        } else {
            self.paints.push(paint);
        }
        self.check_budget()
    }
    fn trim(&mut self, trim: SampledTrimPaths) -> Result<()> {
        let counts: Vec<_> = self.chunks.iter().map(|c| c.paths.len()).collect();
        let paths: Vec<_> = self
            .chunks
            .iter()
            .flat_map(|c| c.paths.iter().cloned())
            .collect();
        let mut cut = path_ops::trim_by_path(&paths, &trim)?.into_iter();
        for (chunk, count) in self.chunks.iter_mut().zip(counts) {
            chunk.paths = cut.by_ref().take(count).flatten().collect();
        }
        for paint in &mut self.paints {
            if paint.vector.fill.is_some() {
                paint.vector.trim = Some(closed_fill());
            }
        }
        self.check_budget()
    }
    fn repeat(
        &mut self,
        r: &Repeater,
        f: f64,
        serial: &mut u32,
    ) -> Result<BTreeMap<String, serde_json::Value>> {
        let copies = scalar(&r.copies, f, 0., 1024.)?;
        let offset = scalar(&r.offset, f, -1024., 1024.)?;
        let position = vector(&r.position, f, -32768., 32768.)?;
        let anchor = vector(&r.anchor, f, -32768., 32768.)?;
        let scale = vector(&r.scale, f, -1000., 1000.)? / 100.;
        let rotation = scalar(&r.rotation, f, -360000., 360000.)?;
        let start = scalar(&r.start_opacity, f, 0., 100.)? / 100.;
        let end = scalar(&r.end_opacity, f, 0., 100.)? / 100.;
        let count = copies.ceil() as usize;
        ensure(
            self.chunks.len().saturating_mul(count) <= 4096
                && self.paints.len().saturating_mul(count) <= MAX_PAINT_BATCHES,
            "vector repeater exceeds instance or paint limit",
        )?;
        ensure(
            self.chunks
                .iter()
                .flat_map(|c| &c.paths)
                .map(|p| p.nodes.len())
                .sum::<usize>()
                .saturating_mul(count)
                <= MAX_EXPANDED_NODES,
            "vector repeater exceeds expanded node limit",
        )?;
        let original_chunks = std::mem::take(&mut self.chunks);
        let original_paints = std::mem::take(&mut self.paints);
        for i in 0..count {
            let n = i as f32 + offset;
            let s = Vec2::new(scale.x.powf(n), scale.y.powf(n));
            ensure(s.is_finite(), "repeater scale cannot be raised to this offset; use integer offset with negative scale")?;
            let matrix = Mat3::from_translation(position * n + anchor)
                * Mat3::from_angle((rotation * n).to_radians())
                * Mat3::from_scale(s)
                * Mat3::from_translation(-anchor);
            let progress = if count <= 1 {
                0.
            } else {
                i as f32 / (count - 1) as f32
            };
            let alpha = (start + (end - start) * progress) * (copies - i as f32).min(1.);
            let mut copy = State {
                chunks: original_chunks.clone(),
                paints: original_paints.clone(),
                parameters: BTreeMap::new(),
            };
            let mut scope_ids = BTreeMap::new();
            for paint in &mut copy.paints {
                for scope in &mut paint.scopes {
                    scope.id = *scope_ids.entry(scope.id).or_insert_with(|| {
                        *serial += 1;
                        *serial
                    });
                }
            }
            copy.transform(matrix, alpha)?;
            if r.composite == Composite::Below {
                let prior = std::mem::take(&mut self.paints);
                self.append(copy)?;
                self.paints.extend(prior);
            } else {
                self.append(copy)?;
            }
        }
        self.check_budget()?;
        Ok(BTreeMap::from([
            ("copies".into(), json!(copies)),
            ("offset".into(), json!(offset)),
            ("position".into(), json!(position.to_array())),
            ("anchor".into(), json!(anchor.to_array())),
            ("scale".into(), json!((scale * 100.).to_array())),
            ("rotation".into(), json!(rotation)),
            ("start_opacity".into(), json!(start * 100.)),
            ("end_opacity".into(), json!(end * 100.)),
        ]))
    }
}
fn sample_group(g: &VectorGroup, f: f64, serial: &mut u32) -> Result<State> {
    let mut state = State::default();
    for item in &g.items {
        let values = match item {
            GroupItem::Group { group } => {
                let prior = std::mem::take(&mut state.paints);
                state.append(sample_group(group, f, serial)?)?;
                state.paints.extend(prior);
                continue;
            }
            GroupItem::Geometry {
                id,
                vector: v,
                size,
                position,
                ..
            } => {
                let size = vector(size, f, 0., 32768.)?;
                let position = vector(position, f, -32768., 32768.)?;
                let mut sampled = v.sample(f, size.to_array())?;
                let mut paths = sampled.paths.clone();
                if let Some(t) = &sampled.trim {
                    paths = path_ops::trim(&paths, t)?;
                    sampled.trim = Some(closed_fill());
                }
                paths_transform(&mut paths, Mat3::from_translation(position))?;
                let index = state.chunks.len();
                state.chunks.push(Chunk { paths, opacity: 1. });
                sampled.paths.clear();
                if sampled.fill.is_some() || sampled.stroke.is_some() {
                    state.paints.insert(
                        0,
                        Paint {
                            chunks: vec![index],
                            vector: sampled.clone(),
                            basis: Mat3::from_translation(position),
                            scopes: vec![],
                        },
                    );
                }
                let mut values = BTreeMap::from([
                    ("size".into(), json!(size.to_array())),
                    ("position".into(), json!(position.to_array())),
                ]);
                if let Some(fill) = sampled.fill {
                    values.insert("fill".into(), json!(fill));
                }
                if let Some(stroke) = sampled.stroke {
                    values.insert("stroke_color".into(), json!(stroke.0));
                    values.insert("stroke_width".into(), json!(stroke.1));
                }
                if let VectorSource::Shape { parameters, .. } = &v.source {
                    for (name, t) in parameters {
                        values.insert(format!("shape:{name}"), json!(t.sample(f)));
                    }
                }
                if let VectorSource::Paths { paths } = &v.source {
                    for path in paths {
                        for node in &path.nodes {
                            values.insert(format!("node:{}:{}", path.id, node.id), json!(node.geometry.sample(f)));
                        }
                    }
                }
                if let Some(t) = &v.trim {
                    values.insert("trim_start".into(), json!(t.start.sample(f)));
                    values.insert("trim_end".into(), json!(t.end.sample(f)));
                    values.insert("trim_offset".into(), json!(t.offset.sample(f)));
                }
                if let Some(d) = &sampled.dashes {
                    values.insert("dash_offset".into(), json!(d.offset));
                    for (i, v) in d.pattern.iter().enumerate() {
                        values.insert(format!("dash_{i}"), json!(v));
                    }
                }
                state
                    .check_budget()
                    .map_err(|e| crate::Error::Invalid(format!("item {id}: {e}")))?;
                values
            }
            GroupItem::Fill {
                color: c,
                fill_rule,
                composite,
                ..
            } => {
                let c = color(c, f)?;
                let mut v = empty_paint();
                v.fill = Some(c);
                v.fill_rule = *fill_rule;
                v.trim = Some(closed_fill());
                state.paint(v, *composite)?;
                BTreeMap::from([("color".into(), json!(c))])
            }
            GroupItem::Stroke {
                stroke: s,
                composite,
                ..
            } => {
                let c = color(&s.color, f)?;
                let width = scalar(&s.width, f, 0., 4096.)?;
                let mut v = empty_paint();
                v.stroke = Some((c, width, s.cap, s.join, s.miter_limit));
                let mut values = BTreeMap::from([("color".into(), json!(c)), ("width".into(), json!(width))]);
                if let Some(d) = &s.dashes {
                    v.dashes = Some(SampledDashes {
                        pattern: d
                            .pattern
                            .iter()
                            .enumerate()
                            .map(|(i, t)| scalar(t, f, if i % 2 == 0 { 0.1 } else { 0. }, 32768.))
                            .collect::<Result<_>>()?,
                        offset: scalar(&d.offset, f, -32768., 32768.)?,
                    });
                    let sampled = v.dashes.as_ref().unwrap();
                    values.insert("dash_offset".into(), json!(sampled.offset));
                    for (i, value) in sampled.pattern.iter().enumerate() {
                        values.insert(format!("dash_{i}"), json!(value));
                    }
                }
                state.paint(v, *composite)?;
                values
            }
            GroupItem::Trim { trim: t, .. } => {
                let start = scalar(&t.start, f, 0., 100.)?;
                let end = scalar(&t.end, f, 0., 100.)?;
                let offset = scalar(&t.offset, f, -360000., 360000.)?;
                state.trim(SampledTrimPaths {
                    start,
                    end,
                    offset,
                    mode: t.mode,
                })?;
                BTreeMap::from([
                    ("start".into(), json!(start)),
                    ("end".into(), json!(end)),
                    ("offset".into(), json!(offset)),
                ])
            }
            GroupItem::Repeater { repeater, .. } => state.repeat(repeater, f, serial)?,
        };
        state.parameters.insert(item.id(), values);
    }
    let t = &g.transform;
    let position = vector(&t.position, f, -32768., 32768.)?;
    let anchor = vector(&t.anchor, f, -32768., 32768.)?;
    let scale = vector(&t.scale, f, -10000., 10000.)?;
    let rotation = scalar(&t.rotation, f, -360000., 360000.)?;
    let skew = scalar(&t.skew, f, -89., 89.)?;
    let axis = scalar(&t.skew_axis, f, -360000., 360000.)?;
    let opacity = scalar(&t.opacity, f, 0., 100.)?;
    state.transform(matrix(position, anchor, scale, rotation, skew, axis), 1.)?;
    if opacity < 100. {
        *serial += 1;
        for paint in &mut state.paints {
            paint.scopes.insert(
                0,
                PaintScope {
                    id: *serial,
                    opacity: opacity / 100.,
                },
            );
        }
    }
    state.parameters.insert(
        g.id,
        BTreeMap::from([
            ("position".into(), json!(position.to_array())),
            ("anchor".into(), json!(anchor.to_array())),
            ("scale".into(), json!(scale.to_array())),
            ("rotation".into(), json!(rotation)),
            ("skew".into(), json!(skew)),
            ("skew_axis".into(), json!(axis)),
            ("opacity".into(), json!(opacity)),
        ]),
    );
    Ok(state)
}
pub(super) fn sample(group: &VectorGroup, frame: f64) -> Result<SampledVector> {
    let mut state = sample_group(group, frame, &mut 0)?;
    let root_opacity = scalar(&group.transform.opacity, frame, 0., 100.)? / 100.;
    if root_opacity < 1. {
        for paint in &mut state.paints {
            paint.scopes.remove(0);
        }
    }
    let mut batches = Vec::new();
    for paint in state.paints {
        if paint.basis.determinant().abs() < 1e-12 {
            continue;
        }
        let inverse = paint.basis.inverse();
        // Equal-opacity contours share one compound paint, preserving holes.
        let mut opacity_groups: BTreeMap<u32, Vec<SampledPath>> = BTreeMap::new();
        for id in paint.chunks {
            let chunk = &state.chunks[id];
            if chunk.opacity <= 0. {
                continue;
            }
            opacity_groups
                .entry(chunk.opacity.to_bits())
                .or_default()
                .extend(chunk.paths.iter().cloned());
        }
        for (alpha, mut paths) in opacity_groups {
            paths_transform(&mut paths, inverse)?;
            let mut vector = paint.vector.clone();
            vector.paths = paths;
            let alpha = f32::from_bits(alpha);
            if let Some(fill) = &mut vector.fill {
                fill[3] *= alpha;
            }
            if let Some(stroke) = &mut vector.stroke {
                stroke.0[3] *= alpha;
            }
            batches.push(PaintBatch {
                vector: Box::new(vector),
                transform: paint.basis.to_cols_array(),
                scopes: paint.scopes.clone(),
            });
            ensure(
                batches.len() <= MAX_PAINT_BATCHES,
                "vector group paint batch limit exceeded",
            )?;
        }
    }
    let mut vector = empty_paint();
    vector.batches = Some(batches);
    vector.root_opacity = root_opacity;
    vector.group_parameters = state.parameters;
    Ok(vector)
}
