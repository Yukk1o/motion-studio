//! Length-based path operations. Cubics are split with de Casteljau, never
//! replaced by polylines; the adaptive length table only locates cut parameters.
use super::{SampledDashes, SampledPath, SampledTrimPaths, TrimMode};
use crate::{ensure, Result};
use glam::DVec2 as V;

pub const MAX_OUTPUT_NODES: usize = 16384;
const MAX_MEASURE_SAMPLES: usize = 65536;
const EPS: f64 = 1e-8;
type Cubic = [V; 4];

fn split(c: Cubic, t: f64) -> (Cubic, Cubic) {
    let a = c[0].lerp(c[1], t);
    let b = c[1].lerp(c[2], t);
    let d = c[2].lerp(c[3], t);
    let e = a.lerp(b, t);
    let f = b.lerp(d, t);
    let p = e.lerp(f, t);
    ([c[0], a, e, p], [p, f, d, c[3]])
}
fn subcurve(c: Cubic, start: f64, end: f64) -> Cubic {
    if start <= 0. && end >= 1. {
        return c;
    }
    let (left, _) = split(c, end);
    if start <= 0. {
        left
    } else {
        split(left, start / end).1
    }
}
struct Segment {
    curve: Cubic,
    line: bool,
    start: f64,
    length: f64,
    table: Vec<(f64, f64)>,
}
struct Measured {
    path: SampledPath,
    segments: Vec<Segment>,
    length: f64,
}
fn table(
    c: Cubic,
    lo: f64,
    hi: f64,
    depth: u8,
    points: &mut Vec<(f64, V)>,
    budget: &mut usize,
) -> Result<()> {
    // Include parameterization error, so collinear nonuniform cubics are also
    // measured correctly (a flatness-only table would cut them at the wrong t).
    let error = c[1].distance(c[0].lerp(c[3], 1. / 3.)) + c[2].distance(c[0].lerp(c[3], 2. / 3.));
    if error <= 0.01 || depth == 12 {
        *budget += 1;
        ensure(
            *budget <= MAX_MEASURE_SAMPLES,
            "vector arc-length table limit exceeded",
        )?;
        points.push((hi, c[3]));
    } else {
        let (a, b) = split(c, 0.5);
        let mid = (lo + hi) * 0.5;
        table(a, lo, mid, depth + 1, points, budget)?;
        table(b, mid, hi, depth + 1, points, budget)?;
    }
    Ok(())
}
fn measure(paths: &[SampledPath]) -> Result<Vec<Measured>> {
    let mut budget = 0;
    paths
        .iter()
        .map(|p| {
            let mut length = 0.;
            let mut segments = Vec::new();
            if p.nodes.len() >= 2 {
                for i in 0..p.nodes.len() - 1 + usize::from(p.closed) {
                    let a = p.nodes[i % p.nodes.len()];
                    let b = p.nodes[(i + 1) % p.nodes.len()];
                    let first = V::new(a[0] as f64, a[1] as f64);
                    let last = V::new(b[0] as f64, b[1] as f64);
                    let line = a[4] == 0. && a[5] == 0. && b[2] == 0. && b[3] == 0.;
                    let curve = if line {
                        [
                            first,
                            first.lerp(last, 1. / 3.),
                            first.lerp(last, 2. / 3.),
                            last,
                        ]
                    } else {
                        [
                            first,
                            first + V::new(a[4] as f64, a[5] as f64),
                            last + V::new(b[2] as f64, b[3] as f64),
                            last,
                        ]
                    };
                    let mut points = vec![(0., first)];
                    table(curve, 0., 1., 0, &mut points, &mut budget)?;
                    let mut distances = vec![(0., 0.)];
                    let mut d = 0.;
                    for pair in points.windows(2) {
                        d += pair[0].1.distance(pair[1].1);
                        distances.push((pair[1].0, d));
                    }
                    if d > EPS {
                        segments.push(Segment {
                            curve,
                            line,
                            start: length,
                            length: d,
                            table: distances,
                        });
                        length += d;
                    }
                }
            }
            Ok(Measured {
                path: p.clone(),
                segments,
                length,
            })
        })
        .collect()
}
fn parameter(s: &Segment, distance: f64) -> f64 {
    let d = distance.clamp(0., s.length);
    let i = s
        .table
        .partition_point(|(_, v)| *v < d)
        .clamp(1, s.table.len() - 1);
    let (t0, d0) = s.table[i - 1];
    let (t1, d1) = s.table[i];
    t0 + (t1 - t0) * ((d - d0) / (d1 - d0).max(EPS))
}
fn cut(p: &Measured, start: f64, end: f64) -> Option<SampledPath> {
    if end - start <= EPS || p.length <= EPS {
        return None;
    }
    if start <= EPS && end >= p.length - EPS {
        return Some(p.path.clone());
    }
    let mut nodes: Vec<[f32; 6]> = Vec::new();
    for s in &p.segments {
        let lo = start.max(s.start) - s.start;
        let hi = end.min(s.start + s.length) - s.start;
        if hi - lo <= EPS {
            continue;
        }
        let c = subcurve(s.curve, parameter(s, lo), parameter(s, hi));
        if nodes.is_empty() {
            nodes.push([c[0].x as f32, c[0].y as f32, 0., 0., 0., 0.]);
        }
        let n = nodes.last_mut().unwrap();
        if !s.line {
            n[4] = (c[1].x - c[0].x) as f32;
            n[5] = (c[1].y - c[0].y) as f32;
        }
        let incoming = if s.line { V::ZERO } else { c[2] - c[3] };
        nodes.push([
            c[3].x as f32,
            c[3].y as f32,
            incoming.x as f32,
            incoming.y as f32,
            0.,
            0.,
        ]);
    }
    (nodes.len() >= 2).then_some(SampledPath {
        closed: false,
        nodes,
    })
}
fn append(out: &mut Vec<SampledPath>, path: SampledPath, nodes: &mut usize) -> Result<()> {
    *nodes += path.nodes.len();
    ensure(
        *nodes <= MAX_OUTPUT_NODES,
        "vector path-operation output node limit exceeded",
    )?;
    out.push(path);
    Ok(())
}
fn join(mut a: SampledPath, b: SampledPath) -> SampledPath {
    let last = a.nodes.last_mut().unwrap();
    last[4] = b.nodes[0][4];
    last[5] = b.nodes[0][5];
    a.nodes.extend_from_slice(&b.nodes[1..]);
    a
}
fn intervals(start: f32, end: f32, offset: f32) -> Vec<(f64, f64)> {
    let lo = f64::from(start.min(end)) / 100.;
    let width = f64::from((end - start).abs()) / 100.;
    if width <= EPS {
        return vec![];
    }
    if width >= 1. - EPS {
        return vec![(0., 1.)];
    }
    let a = (lo + f64::from(offset) / 360.).rem_euclid(1.);
    let b = a + width;
    if b <= 1. {
        vec![(a, b)]
    } else {
        vec![(a, 1.), (0., b - 1.)]
    }
}
/// Simultaneous: same fractional interval on every contour. Individual:
/// one length-weighted interval over the stored contour order.
pub fn trim(paths: &[SampledPath], t: &SampledTrimPaths) -> Result<Vec<SampledPath>> {
    Ok(trim_by_path(paths, t)?.into_iter().flatten().collect())
}
/// Preserve contour ownership for ordered groups and their independent paints.
pub fn trim_by_path(paths: &[SampledPath], t: &SampledTrimPaths) -> Result<Vec<Vec<SampledPath>>> {
    ensure(
        (0.0..=100.).contains(&t.start)
            && (0.0..=100.).contains(&t.end)
            && (-360000.0..=360000.).contains(&t.offset),
        "invalid sampled trim parameters",
    )?;
    let ranges = intervals(t.start, t.end, t.offset);
    if ranges == [(0., 1.)] {
        return Ok(paths.iter().cloned().map(|p| vec![p]).collect());
    }
    if ranges.is_empty() {
        return Ok(vec![vec![]; paths.len()]);
    }
    let measured = measure(paths)?;
    let total: f64 = measured.iter().map(|p| p.length).sum();
    let mut out = Vec::new();
    let mut nodes = 0;
    let mut cursor = 0.;
    for p in &measured {
        let (base, length) = if t.mode == TrimMode::Simultaneously {
            (0., p.length)
        } else {
            (cursor, total)
        };
        let mut pieces = Vec::new();
        for &(a, b) in &ranges {
            if let Some(piece) = cut(
                p,
                (a * length - base).max(0.),
                (b * length - base).min(p.length),
            ) {
                pieces.push(piece);
            }
        }
        // A wrapped interval on a closed contour is a single stroke across
        // its original seam, with no artificial caps at the first vertex.
        if p.path.closed && pieces.len() == 2 && t.mode == TrimMode::Simultaneously {
            let b = pieces.pop().unwrap();
            let a = pieces.pop().unwrap();
            pieces.push(join(a, b));
        }
        nodes += pieces.iter().map(|p| p.nodes.len()).sum::<usize>();
        ensure(nodes <= MAX_OUTPUT_NODES, "vector path-operation output node limit exceeded")?;
        out.push(pieces);
        cursor += p.length;
    }
    Ok(out)
}
/// The dash phase resets per contour. A positive offset advances the pattern.
pub fn dash(paths: &[SampledPath], d: &SampledDashes) -> Result<Vec<SampledPath>> {
    ensure(
        matches!(d.pattern.len(), 2 | 4 | 6)
            && (-32768.0..=32768.).contains(&d.offset)
            && d.pattern
                .iter()
                .enumerate()
                .all(|(i, v)| (if i % 2 == 0 { 0.1 } else { 0. }..=32768.).contains(v)),
        "invalid sampled dash pattern",
    )?;
    if d.pattern.iter().skip(1).step_by(2).all(|v| *v == 0.) {
        return Ok(paths.to_vec());
    }
    let cycle: f64 = d.pattern.iter().map(|v| f64::from(*v)).sum();
    ensure(
        cycle > 0. && cycle.is_finite(),
        "invalid sampled dash cycle",
    )?;
    let measured = measure(paths)?;
    let mut out = Vec::new();
    let mut nodes = 0;
    let mut steps = 0;
    for p in &measured {
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        let mut cursor = -f64::from(d.offset).rem_euclid(cycle);
        while cursor < p.length {
            for (i, &length) in d.pattern.iter().enumerate() {
                steps += 1;
                ensure(
                    steps <= MAX_OUTPUT_NODES * 8,
                    "vector dash iteration limit exceeded",
                )?;
                let end = cursor + f64::from(length);
                if i % 2 == 0 && end > 0. && cursor < p.length {
                    let lo = cursor.max(0.);
                    let hi = end.min(p.length);
                    if let Some((_, previous_end)) =
                        ranges.last_mut().filter(|(_, e)| (lo - *e).abs() <= EPS)
                    {
                        *previous_end = hi;
                    } else {
                        ranges.push((lo, hi));
                    }
                    ensure(
                        ranges.len() <= MAX_OUTPUT_NODES / 2,
                        "vector dash segment limit exceeded",
                    )?;
                }
                cursor = end;
            }
        }
        let wrapped = p.path.closed
            && ranges.len() > 1
            && ranges[0].0 <= EPS
            && ranges.last().unwrap().1 >= p.length - EPS;
        if wrapped {
            let first = ranges.remove(0);
            let last = ranges.pop().unwrap();
            if let (Some(a), Some(b)) = (cut(p, last.0, last.1), cut(p, first.0, first.1)) {
                append(&mut out, join(a, b), &mut nodes)?;
            }
        }
        for (a, b) in ranges {
            if let Some(piece) = cut(p, a, b) {
                append(&mut out, piece, &mut nodes)?;
            }
        }
    }
    Ok(out)
}
