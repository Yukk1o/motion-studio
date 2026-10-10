//! Editable dock tree. Native windows live in the shell; panel ownership lives here.
use motion_ui::{
    dock::{self, DockId, Edge, Node},
    input::{Input, MouseButton, Rect},
    theme::metrics,
};
use std::path::Path;
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    #[serde(default)]
    pub locale: crate::i18n::Locale,
    pub root: Node,
    pub floating: Vec<DockId>,
    #[serde(skip)]
    drag: Option<Drag>,
}
enum Drag {
    Split {
        path: Vec<bool>,
        area: Rect,
        edge: Edge,
    },
    Tab {
        panel: DockId,
        start: [f32; 2],
    },
}
pub enum Change {
    Float(DockId),
    Changed,
}
struct Handle {
    path: Vec<bool>,
    area: Rect,
    edge: Edge,
    hit: Rect,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            locale: Default::default(),
            root: dock::editor_layout(),
            floating: vec![],
            drag: None,
        }
    }
}
fn panels(node: &Node, out: &mut Vec<DockId>) {
    match node {
        Node::Dock { tabs, .. } => out.extend_from_slice(tabs),
        Node::Split { first, second, .. } => {
            panels(first, out);
            panels(second, out)
        }
        _ => {}
    }
}
fn remove(node: Node, panel: DockId) -> Option<Node> {
    match node {
        Node::Dock { id, tabs, active } => {
            let tabs: Vec<_> = tabs.into_iter().filter(|id| *id != panel).collect();
            if tabs.is_empty() {
                None
            } else {
                Some(Node::Dock {
                    id: if id == panel { tabs[0] } else { id },
                    active: active.min(tabs.len() - 1),
                    tabs,
                })
            }
        }
        Node::Split {
            edge,
            split,
            first,
            second,
        } => match (remove(*first, panel), remove(*second, panel)) {
            (Some(first), Some(second)) => Some(Node::Split {
                edge,
                split,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (Some(node), None) | (None, Some(node)) => Some(node),
            (None, None) => None,
        },
        _ => None,
    }
}
fn insert(node: &mut Node, panel: DockId, target: DockId, edge: Option<Edge>) -> bool {
    match node {
        Node::Dock { tabs, active, .. } if tabs.contains(&target) => {
            if let Some(edge) = edge {
                let old = node.clone();
                let incoming = Node::dock(panel);
                let (first, second) = if matches!(edge, Edge::Left | Edge::Top) {
                    (incoming, old)
                } else {
                    (old, incoming)
                };
                // Left/Top divide is expressed as the first child's share.
                *node = Node::Split {
                    edge: if edge.is_horizontal() {
                        Edge::Left
                    } else {
                        Edge::Top
                    },
                    split: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
            } else {
                tabs.push(panel);
                *active = tabs.len() - 1;
            }
            true
        }
        Node::Split { first, second, .. } => {
            insert(first, panel, target, edge) || insert(second, panel, target, edge)
        }
        _ => false,
    }
}
fn split_rect(edge: Edge, area: Rect, ratio: f32) -> (Rect, Rect) {
    let ratio = ratio.clamp(0.05, 0.95);
    if edge.is_horizontal() {
        let first = if edge == Edge::Right {
            1.0 - ratio
        } else {
            ratio
        };
        let w = area.width() * first;
        (
            Rect::new(area.min[0], area.min[1], w, area.height()),
            Rect::new(
                area.min[0] + w,
                area.min[1],
                area.width() - w,
                area.height(),
            ),
        )
    } else {
        let first = if edge == Edge::Bottom {
            1.0 - ratio
        } else {
            ratio
        };
        let h = area.height() * first;
        (
            Rect::new(area.min[0], area.min[1], area.width(), h),
            Rect::new(
                area.min[0],
                area.min[1] + h,
                area.width(),
                area.height() - h,
            ),
        )
    }
}
fn handles(node: &Node, area: Rect, path: &mut Vec<bool>, out: &mut Vec<Handle>) {
    if let Node::Split {
        edge,
        split,
        first,
        second,
    } = node
    {
        let (a, b) = split_rect(*edge, area, *split);
        let hit = if edge.is_horizontal() {
            Rect::new(a.max[0] - 3.0, area.min[1], 6.0, area.height())
        } else {
            Rect::new(area.min[0], a.max[1] - 3.0, area.width(), 6.0)
        };
        out.push(Handle {
            path: path.clone(),
            area,
            edge: *edge,
            hit,
        });
        path.push(false);
        handles(first, a, path, out);
        path.pop();
        path.push(true);
        handles(second, b, path, out);
        path.pop();
    }
}
fn set_ratio(node: &mut Node, path: &[bool], value: f32) {
    if let Node::Split {
        split,
        first,
        second,
        ..
    } = node
    {
        if let Some((branch, tail)) = path.split_first() {
            set_ratio(if *branch { second } else { first }, tail, value)
        } else {
            *split = value.clamp(0.05, 0.95);
        }
    }
}
impl Workspace {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .filter(|b| b.len() <= 16 * 1024)
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .filter(Self::valid)
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    fn valid(&self) -> bool {
        let mut ids = vec![];
        panels(&self.root, &mut ids);
        ids.extend_from_slice(&self.floating);
        ids.sort_by_key(|id| id.0);
        fn sane(n: &Node) -> bool {
            match n {
                Node::Split {
                    split,
                    first,
                    second,
                    ..
                } => {
                    split.is_finite()
                        && (0.05..=0.95).contains(split)
                        && sane(first)
                        && sane(second)
                }
                Node::Dock { tabs, active, .. } => !tabs.is_empty() && *active < tabs.len(),
                _ => false,
            }
        }
        // A fully floating workspace uses an empty Fixed placeholder.
        ids == vec![DockId(1), DockId(2), DockId(3), DockId(4)]
            && (sane(&self.root) || self.floating.len() == 4)
    }
    pub fn float(&mut self, panel: DockId) -> bool {
        if self.floating.contains(&panel) {
            return false;
        }
        let mut ids = vec![];
        panels(&self.root, &mut ids);
        if !ids.contains(&panel) {
            return false;
        }
        self.root = remove(self.root.clone(), panel).unwrap_or(Node::Fixed {
            edge: Edge::Top,
            size: 0.0,
        });
        self.floating.push(panel);
        self.drag = None;
        true
    }
    pub fn redock(&mut self, panel: DockId) {
        self.floating.retain(|id| *id != panel);
        let mut ids = vec![];
        panels(&self.root, &mut ids);
        if ids.contains(&panel) {
            return;
        }
        if let Some(target) = ids.first().copied() {
            let edge = match panel.0 {
                1 => Edge::Left,
                2 => Edge::Right,
                4 => Edge::Bottom,
                _ => Edge::Left,
            };
            insert(&mut self.root, panel, target, Some(edge));
        } else {
            self.root = Node::dock(panel);
        }
    }
    pub fn move_panel(&mut self, panel: DockId, target: DockId, edge: Option<Edge>) -> bool {
        if panel == target {
            return false;
        }
        let next = remove(self.root.clone(), panel);
        let Some(mut next) = next else { return false };
        if !insert(&mut next, panel, target, edge) {
            return false;
        }
        self.root = next;
        self.drag = None;
        true
    }
    /// Drag a divider to resize; drag a title to dock on an edge or combine tabs.
    /// Releasing a title outside the workspace detaches it to a native window.
    pub fn interact(&mut self, input: &Input, size: [f32; 2]) -> Option<Change> {
        let area = Rect::new(
            0.0,
            crate::panels::CHROME_HEIGHT,
            size[0],
            (size[1] - crate::panels::CHROME_HEIGHT - 22.0).max(0.0),
        );
        if input.pressed.is_some_and(|(b, _)| b == MouseButton::Left) {
            let mut found = vec![];
            handles(&self.root, area, &mut vec![], &mut found);
            if let Some(h) = found
                .into_iter()
                .rev()
                .find(|h| h.hit.contains(input.mouse))
            {
                self.drag = Some(Drag::Split {
                    path: h.path,
                    area: h.area,
                    edge: h.edge,
                });
            } else {
                for placement in dock::solve(&self.root, area) {
                    let title = Rect::new(
                        placement.area.min[0],
                        placement.area.min[1],
                        (placement.area.width() - 30.0).max(0.0),
                        metrics::TAB_BAR,
                    );
                    if title.contains(input.mouse) {
                        self.drag = Some(Drag::Tab {
                            panel: placement.id,
                            start: input.mouse,
                        });
                        break;
                    }
                }
            }
        }
        if let Some(Drag::Split { path, area, edge }) = &self.drag {
            let size = if edge.is_horizontal() {
                area.width()
            } else {
                area.height()
            };
            let coordinate = if edge.is_horizontal() {
                input.mouse[0] - area.min[0]
            } else {
                input.mouse[1] - area.min[1]
            };
            let minimum = if edge.is_horizontal() {
                metrics::MIN_DOCK
            } else {
                metrics::MIN_TIMELINE
            };
            let low = (minimum / size.max(1.0)).min(0.45);
            let ratio = (coordinate / size.max(1.0)).clamp(low, 1.0 - low);
            let ratio = if matches!(edge, Edge::Right | Edge::Bottom) {
                1.0 - ratio
            } else {
                ratio
            };
            let path = path.clone();
            set_ratio(&mut self.root, &path, ratio);
            if input.released.is_some() {
                self.drag = None;
            }
            return Some(Change::Changed);
        }
        if input.released.is_some() {
            if let Some(Drag::Tab { panel, start }) = self.drag.take() {
                if (start[0] - input.mouse[0]).hypot(start[1] - input.mouse[1]) < 6.0 {
                    return None;
                }
                let placements = dock::solve(&self.root, area);
                if let Some(target) = placements
                    .into_iter()
                    .find(|p| p.area.contains(input.mouse) && p.id != panel)
                {
                    let p = input.mouse;
                    let a = target.area;
                    let dx = (p[0] - a.min[0]) / a.width().max(1.0);
                    let dy = (p[1] - a.min[1]) / a.height().max(1.0);
                    let edge = if dx < 0.25 {
                        Some(Edge::Left)
                    } else if dx > 0.75 {
                        Some(Edge::Right)
                    } else if dy < 0.25 {
                        Some(Edge::Top)
                    } else if dy > 0.75 {
                        Some(Edge::Bottom)
                    } else {
                        None
                    };
                    if self.move_panel(panel, target.id, edge) {
                        return Some(Change::Changed);
                    }
                } else if !area.contains(input.mouse) {
                    return Some(Change::Float(panel));
                }
            }
        }
        None
    }
    pub fn cancel(&mut self) {
        self.drag = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_panel_keeps_one_owner_after_dock_float_and_close() {
        let mut w = Workspace::default();
        assert!(w.valid());
        assert!(w.move_panel(DockId(2), DockId(1), None));
        assert!(w.valid());
        assert!(w.float(DockId(3)));
        assert!(w.valid());
        assert!(!w.float(DockId(3)));
        w.redock(DockId(3));
        assert!(w.valid());
        assert_eq!(w.floating.len(), 0);
        for p in 1..=4 {
            assert!(w.float(DockId(p)));
        }
        assert!(w.valid());
        for p in 1..=4 {
            w.redock(DockId(p));
            assert!(w.valid());
        }
    }
    #[test]
    fn layout_round_trips_and_invalid_preferences_reset() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("layout.json");
        let mut w = Workspace::default();
        w.float(DockId(2));
        w.save(&path).unwrap();
        let loaded = Workspace::load(&path);
        assert_eq!(loaded.floating, vec![DockId(2)]);
        assert!(loaded.valid());
        std::fs::write(&path, b"{}").unwrap();
        assert!(Workspace::load(&path).valid());
    }
}
