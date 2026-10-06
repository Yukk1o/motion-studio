//! Exact back-to-front ordering of flat 3D layers, including intersections.
//! BSP splitting preserves texture coordinates and premultiplied alpha order.
//! No per-pixel lists, extra fullscreen passes, or unbounded geometry growth.
use crate::{ensure, ProjectionKind, Result, Scene, MAX_LAYERS};
use glam::{DVec2, DVec3};
use std::ops::Range;

const MAX_NODES: usize = 8192;
const MAX_ARENA: usize = 131_072;
const MAX_OUTPUT: usize = 65_536;
const EPSILON: f64 = 1.0e-5;

#[derive(Clone, Copy, Debug)]
pub struct PlaneVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct PlaneBatch {
    pub layer: usize,
    pub vertices: Range<u32>,
}
#[derive(Clone, Copy)]
struct Vertex {
    position: DVec3,
    uv: DVec2,
}
#[derive(Clone, Copy)]
struct Plane {
    normal: DVec3,
    distance: f64,
}
impl Plane {
    fn distance(self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.distance
    }
}
#[derive(Clone)]
struct Polygon {
    layer: usize,
    vertices: Range<usize>,
}
struct Node {
    polygon: Polygon,
    plane: Plane,
    front: Option<usize>,
    back: Option<usize>,
    next: Option<usize>,
}

pub struct PlaneCompositor {
    pub vertices: Vec<PlaneVertex>,
    pub batches: Vec<PlaneBatch>,
    arena: Vec<Vertex>,
    nodes: Vec<Node>,
    tasks: Vec<(Polygon, usize)>,
    visits: Vec<(usize, bool)>,
    coplanar: Vec<usize>,
    planes: [Option<Plane>; MAX_LAYERS],
}
impl Default for PlaneCompositor {
    fn default() -> Self {
        Self::new()
    }
}
impl PlaneCompositor {
    pub fn new() -> Self {
        Self {
            vertices: Vec::with_capacity(MAX_LAYERS * 12),
            batches: Vec::with_capacity(MAX_LAYERS * 2),
            arena: Vec::with_capacity(MAX_LAYERS * 16),
            nodes: Vec::with_capacity(MAX_LAYERS * 4),
            tasks: Vec::with_capacity(MAX_LAYERS * 4),
            visits: Vec::with_capacity(MAX_LAYERS * 4),
            coplanar: Vec::with_capacity(MAX_LAYERS),
            planes: [None; MAX_LAYERS],
        }
    }
    pub fn prepare(&mut self, scene: &Scene) -> Result<()> {
        self.prepare_with_sizes(scene, &[])
    }
    /// Effects can expand a plane's local bounds while retaining its projection.
    pub fn prepare_with_sizes(&mut self, scene: &Scene, sizes: &[[f32; 2]]) -> Result<()> {
        self.prepare_with_sizes_and_overlays(scene, sizes, &[])
    }
    /// Screen-space generators form flat compositing boundaries; their sprites
    /// already include camera and emitter transforms, so do not transform twice.
    pub fn prepare_with_sizes_and_overlays(&mut self, scene: &Scene, sizes: &[[f32; 2]], overlays: &[bool]) -> Result<()> {
        ensure(sizes.is_empty() || sizes.len() == scene.layers.len(), "invalid compositor sizes")?;
        ensure(overlays.is_empty() || overlays.len() == scene.layers.len(), "invalid compositor overlays")?;
        let overlay = |index: usize| overlays.get(index).copied().unwrap_or(false);
        let spatial = |index: usize| scene.layers[index].three_d && !overlay(index);
        ensure(
            scene.layers.len() <= MAX_LAYERS,
            "too many compositor layers",
        )?;
        self.vertices.clear();
        self.batches.clear();
        self.arena.clear();
        self.nodes.clear();
        self.tasks.clear();
        self.visits.clear();
        let mut start = 0;
        while start < scene.layers.len() {
            if !spatial(start) {
                let p = self.quad(scene, start, sizes, overlay(start))?;
                self.emit(p)?;
                start += 1;
                continue;
            }
            let end = (start..scene.layers.len()).find(|&index| !spatial(index)).unwrap_or(scene.layers.len());
            let mut root = None;
            for layer in start..end {
                let p = self.quad(scene, layer, sizes, false)?;
                if self.planes[layer].is_none() {
                    continue;
                }
                if let Some(r) = root {
                    self.insert(p, r)?;
                } else {
                    root = Some(self.node(p)?);
                }
            }
            if let Some(root) = root {
                self.traverse(root, scene)?;
            }
            start = end;
        }
        Ok(())
    }
    fn quad(&mut self, scene: &Scene, layer: usize, sizes: &[[f32; 2]], overlay: bool) -> Result<Polygon> {
        ensure(
            self.arena.len() + 4 <= MAX_ARENA,
            "intersection geometry budget exceeded",
        )?;
        let l = &scene.layers[layer];
        let size = sizes.get(layer).copied().unwrap_or(l.size);
        let model = if overlay { glam::DMat4::IDENTITY } else { l.model.as_dmat4() };
        let start = self.arena.len();
        for [x, y] in [[-0.5, 0.5], [-0.5, -0.5], [0.5, -0.5], [0.5, 0.5]] {
            let position = model.transform_point3(DVec3::new(
                x * f64::from(size[0]),
                y * f64::from(size[1]),
                0.0,
            ));
            ensure(position.is_finite(), "invalid plane geometry")?;
            self.arena.push(Vertex {
                position,
                uv: DVec2::new(x + 0.5, 0.5 - y),
            });
        }
        let a = self.arena[start].position;
        let normal = (self.arena[start + 1].position - a).cross(self.arena[start + 3].position - a);
        self.planes[layer] = if normal.length_squared() > 1e-20 {
            let normal = normal.normalize();
            Some(Plane {
                normal,
                distance: normal.dot(a),
            })
        } else {
            None
        };
        Ok(Polygon {
            layer,
            vertices: start..start + 4,
        })
    }
    fn node(&mut self, polygon: Polygon) -> Result<usize> {
        ensure(
            self.nodes.len() < MAX_NODES,
            "intersection fragment budget exceeded",
        )?;
        let index = self.nodes.len();
        let plane = self.planes[polygon.layer].expect("nondegenerate spatial plane");
        self.nodes.push(Node {
            polygon,
            plane,
            front: None,
            back: None,
            next: None,
        });
        Ok(index)
    }
    fn insert(&mut self, polygon: Polygon, root: usize) -> Result<()> {
        self.tasks.push((polygon, root));
        while let Some((polygon, mut node)) = self.tasks.pop() {
            let p = polygon;
            loop {
                let plane = self.nodes[node].plane;
                let mut front = false;
                let mut back = false;
                for i in p.vertices.clone() {
                    let d = plane.distance(self.arena[i].position);
                    front |= d > EPSILON;
                    back |= d < -EPSILON;
                }
                if front && back {
                    let a = self.clip(&p, plane, true)?;
                    let b = self.clip(&p, plane, false)?;
                    for (fragment, side) in [(a, true), (b, false)] {
                        let child = if side {
                            self.nodes[node].front
                        } else {
                            self.nodes[node].back
                        };
                        if let Some(child) = child {
                            self.tasks.push((fragment, child));
                        } else {
                            let child = self.node(fragment)?;
                            if side {
                                self.nodes[node].front = Some(child);
                            } else {
                                self.nodes[node].back = Some(child);
                            }
                        }
                    }
                    break;
                }
                if !front && !back {
                    let next = self.nodes[node].next;
                    let added = self.node(p)?;
                    self.nodes[added].next = next;
                    self.nodes[node].next = Some(added);
                    break;
                }
                let child = if front {
                    self.nodes[node].front
                } else {
                    self.nodes[node].back
                };
                if let Some(child) = child {
                    node = child;
                } else {
                    let child = self.node(p)?;
                    if front {
                        self.nodes[node].front = Some(child);
                    } else {
                        self.nodes[node].back = Some(child);
                    }
                    break;
                }
            }
        }
        Ok(())
    }
    fn clip(&mut self, p: &Polygon, plane: Plane, front: bool) -> Result<Polygon> {
        let start = self.arena.len();
        let sign = if front { 1.0 } else { -1.0 };
        for i in p.vertices.clone() {
            let j = if i + 1 == p.vertices.end {
                p.vertices.start
            } else {
                i + 1
            };
            let a = self.arena[i];
            let b = self.arena[j];
            // Snap near-plane distances for identical shared edges on both sides.
            let snap = |d: f64| if d.abs() <= EPSILON { 0.0 } else { d };
            let da = snap(plane.distance(a.position));
            let db = snap(plane.distance(b.position));
            if da * sign >= 0.0 {
                self.push_vertex(a)?;
            }
            if da * db < 0.0 {
                let t = da / (da - db);
                self.push_vertex(Vertex {
                    position: a.position.lerp(b.position, t),
                    uv: a.uv.lerp(b.uv, t),
                })?;
            }
        }
        ensure(
            self.arena.len() - start >= 3,
            "degenerate intersection fragment",
        )?;
        Ok(Polygon {
            layer: p.layer,
            vertices: start..self.arena.len(),
        })
    }
    fn push_vertex(&mut self, v: Vertex) -> Result<()> {
        ensure(
            self.arena.len() < MAX_ARENA,
            "intersection geometry budget exceeded",
        )?;
        self.arena.push(v);
        Ok(())
    }
    fn traverse(&mut self, root: usize, scene: &Scene) -> Result<()> {
        let eye = scene.camera.eye.as_dvec3();
        let toward_viewer = eye - scene.camera.target.as_dvec3();
        self.visits.push((root, false));
        while let Some((node, emit)) = self.visits.pop() {
            if emit {
                self.coplanar.clear();
                let mut current = Some(node);
                while let Some(i) = current {
                    self.coplanar.push(i);
                    current = self.nodes[i].next;
                }
                self.coplanar
                    .sort_unstable_by_key(|i| scene.layers[self.nodes[*i].polygon.layer].order);
                for i in 0..self.coplanar.len() {
                    self.emit(self.nodes[self.coplanar[i]].polygon.clone())?;
                }
                continue;
            }
            let n = &self.nodes[node];
            let viewer_side = match scene.camera.projection {
                ProjectionKind::Perspective => n.plane.distance(eye),
                // Parallel rays have a viewer at infinity, independent of the
                // plane's position relative to the finite camera eye.
                ProjectionKind::Orthographic => n.plane.normal.dot(toward_viewer),
            };
            let (far, near) = if viewer_side >= 0.0 {
                (n.back, n.front)
            } else {
                (n.front, n.back)
            };
            if let Some(near) = near {
                self.visits.push((near, false));
            }
            self.visits.push((node, true));
            if let Some(far) = far {
                self.visits.push((far, false));
            }
        }
        Ok(())
    }
    fn emit(&mut self, p: Polygon) -> Result<()> {
        let count = (p.vertices.len() - 2) * 3;
        ensure(
            self.vertices.len() + count <= MAX_OUTPUT,
            "intersection triangle budget exceeded",
        )?;
        let start = self.vertices.len() as u32;
        for i in p.vertices.start + 1..p.vertices.end - 1 {
            for j in [p.vertices.start, i, i + 1] {
                let v = self.arena[j];
                self.vertices.push(PlaneVertex {
                    position: v.position.as_vec3().to_array(),
                    uv: v.uv.as_vec2().to_array(),
                });
            }
        }
        let end = self.vertices.len() as u32;
        if let Some(last) = self.batches.last_mut().filter(|b| b.layer == p.layer) {
            last.vertices.end = end;
        } else {
            self.batches.push(PlaneBatch {
                layer: p.layer,
                vertices: start..end,
            });
        }
        Ok(())
    }
}
