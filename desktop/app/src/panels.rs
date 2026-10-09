//! After Effects style panel chrome.
//!
//! Every panel reads the same engine snapshot, so the shell renders the project
//! panel, effect controls, composition viewer and timeline from one state value
//! rather than from per-panel caches that could drift apart.

use aem_host::ops::editing;
use aem_ui::{
    dock::{self, Node, Panel},
    input::{Input, Rect, Scroll},
    paint::{self, PaintList},
    text::TextAtlas,
    theme::{metrics, palette},
    ui::{self, Align, Icon, LayerRow},
};
use serde_json::Value;

/// Menu bar plus context toolbar, in logical pixels at 100% UI scale.
pub const CHROME_HEIGHT: f32 = metrics::MENU_BAR + metrics::TOOL_BAR;

/// Menu definitions, matching the After Effects File/Edit menu names so the
/// keyboard map and muscle memory carry over.
pub const MENUS: [(&str, &[&str]); 4] = [
    ("File", &["New Project…", "Open Project…", "Save", "Import Footage…", "Export"]),
    (
        "Edit",
        &["Undo", "Redo", "Cut", "Copy", "Paste", "Preferences…"],
    ),
    (
        "Layer",
        &["New", "Duplicate", "Delete", "Precompose", "Mask", "Effect…"],
    ),
    (
        "Composition",
        &["New Footage Bin", "Settings…", "Track Viewers", "Frame Blending"],
    ),
];

/// Persistent UI state that does not belong to the engine project.
#[derive(Default)]
pub struct Panels {
    pub project_scroll: Scroll,
    pub timeline_scroll: Scroll,
    pub properties_scroll: Scroll,
    pub selected_layer: Option<u64>,
    pub selected_property: usize,
    /// Pixels per composition frame in the timeline.
    pub timeline_zoom: f32,
    pub timeline_offset: f32,
    /// Panels open in the Effect Controls dock, in tab order.
    pub control_tabs: Vec<Panel>,
    pub control_active: usize,
}

impl Panels {
    /// The tab list for a dock, combining the base panel with opened tabs.
    fn tabs_for(&self, panel: Panel) -> Vec<Panel> {
        match panel {
            Panel::EffectControls => {
                let mut tabs = vec![Panel::EffectControls];
                for candidate in &self.control_tabs {
                    if *candidate != Panel::EffectControls && !tabs.contains(candidate) {
                        tabs.push(*candidate);
                    }
                }
                tabs
            }
            _ => vec![panel],
        }
    }

    fn active_for(&self, panel: Panel) -> usize {
        match panel {
            Panel::EffectControls => self.control_active,
            _ => 0,
        }
    }
}

/// Which panel a point falls inside, plus its content rectangle.
struct PanelFrame {
    panel: Panel,
    tabs: Rect,
    content: Rect,
}

fn frames(layout: &Node, area: Rect) -> Vec<PanelFrame> {
    dock::solve(layout, area)
        .into_iter()
        .filter_map(|placement| {
            let panel = match placement.id {
                id if id == Panel::Project.dock() => Panel::Project,
                id if id == Panel::EffectControls.dock() => Panel::EffectControls,
                id if id == Panel::Composition.dock() => Panel::Composition,
                id if id == Panel::Timeline.dock() => Panel::Timeline,
                _ => return None,
            };
            let tabs = Rect::new(
                placement.area.min[0],
                placement.area.min[1],
                placement.area.width(),
                metrics::TAB_BAR,
            );
            let content = Rect::new(
                placement.area.min[0],
                tabs.max[1],
                placement.area.width(),
                (placement.area.height() - metrics::TAB_BAR).max(0.0),
            );
            Some(PanelFrame {
                panel,
                tabs,
                content,
            })
        })
        .collect()
}

/// Draw the whole shell: menu bar, toolbar and every dock.
#[allow(clippy::too_many_arguments)]
pub fn chrome(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    layout: &Node,
    panels: &Panels,
    state: Option<&Value>,
    input: &Input,
    scale: f32,
) {
    paint::rect(list, area, palette::APP_BACKGROUND);
    menu_bar(list, atlas, area, input, scale);
    toolbar(list, atlas, Rect::new(0.0, metrics::MENU_BAR, area.width(), metrics::TOOL_BAR), state, input);
    let dock_area = Rect::new(
        0.0,
        CHROME_HEIGHT,
        area.width(),
        (area.height() - CHROME_HEIGHT).max(0.0),
    );
    for frame in frames(layout, dock_area) {
        let tabs = panels.tabs_for(frame.panel);
        let titles: Vec<&str> = tabs.iter().map(|p| p.title()).collect();
        let active = panels.active_for(frame.panel);
        if ui::tab_strip(list, atlas, frame.tabs, &titles, active, input).is_some() {
            continue;
        }
        let body = match frame.panel {
            Panel::Project => project_panel(list, atlas, frame.content, state, input),
            Panel::EffectControls => properties_panel(list, atlas, frame.content, state, input),
            Panel::Composition => composition_panel(list, atlas, frame.content, state, input),
            Panel::Timeline => timeline_panel(list, atlas, frame.content, state, input),
        };
        if body {
            paint::stroke(list, frame.content, palette::BORDER, 1.0);
        }
    }
    if let Some(error) = state.and_then(|s| s["renderError"].as_str()) {
        if !error.is_empty() {
            status_bar(list, atlas, dock_area, error, scale);
        }
    }
}

fn menu_bar(list: &mut PaintList, atlas: &mut TextAtlas, area: Rect, input: &Input, scale: f32) {
    let bar = Rect::new(0.0, 0.0, area.width(), metrics::MENU_BAR);
    paint::rect(list, bar, palette::CHROME);
    let mut x = metrics::SPLITTER * 2.0;
    for (name, items) in MENUS {
        let width = atlas.measure(name) + metrics::SPLITTER * 4.0;
        let item = Rect::new(x, 0.0, width, metrics::MENU_BAR);
        let hovered = item.contains(input.mouse);
        if hovered {
            paint::rect(list, item, palette::ACCENT.with_alpha(0.25));
        }
        ui::text(
            list,
            atlas,
            item,
            name,
            palette::TEXT,
            Align::Left,
        );
        // Opening a menu is reported by returning true; the shell shows the
        // popup so the item list above stays the single source of truth.
        if hovered && matches!(input.pressed, Some((aem_ui::MouseButton::Left, p)) if item.contains(p))
        {
            list.colors.push(palette::ACCENT);
        }
        let _ = items;
        x += width;
    }
    ui::text(
        list,
        atlas,
        Rect::new(x + metrics::SPLITTER * 2.0, 0.0, 220.0, metrics::MENU_BAR),
        "Motion Studio",
        palette::TEXT_MUTED,
        Align::Left,
    );
    paint::rect(
        list,
        Rect::new(0.0, metrics::MENU_BAR - 1.0, area.width(), 1.0),
        palette::BORDER,
    );
    let _ = scale;
}

fn toolbar(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    state: Option<&Value>,
    input: &Input,
) {
    paint::rect(list, area, palette::PANEL);
    let can_undo = state.and_then(|s| s["canUndo"].as_bool()).unwrap_or(false);
    let can_redo = state.and_then(|s| s["canRedo"].as_bool()).unwrap_or(false);
    let mut x = metrics::SPLITTER * 2.0;
    let mut button = |list: &mut PaintList,
                      atlas: &mut TextAtlas,
                      x: &mut f32,
                      label: &str,
                      enabled: bool,
                      input: &Input| {
        let item = Rect::new(*x, area.min[1] + 4.0, 62.0, area.height() - 8.0);
        ui::button(list, atlas, item, label, input, enabled);
        *x += 66.0;
    };
    button(list, atlas, &mut x, "Undo", can_undo, input);
    button(list, atlas, &mut x, "Redo", can_redo, input);
    if let Some(preview) = state {
        let tier = preview["preview"]["tier"].as_str().unwrap_or("-");
        let fps = preview["preview"]["fps"].as_i64().unwrap_or(0);
        ui::text(
            list,
            atlas,
            Rect::new(x + 12.0, area.min[1], 240.0, area.height()),
            &format!("Preview {tier} · {fps} fps"),
            palette::TEXT_MUTED,
            Align::Left,
        );
    }
}

/// Project panel: composition metadata, settings and engine diagnostics.
fn project_panel(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    state: Option<&Value>,
    input: &Input,
) -> bool {
    paint::rect(list, area, palette::PANEL);
    let Some(state) = state else {
        ui::text(
            list,
            atlas,
            area,
            "No project open",
            palette::TEXT_MUTED,
            Align::Left,
        );
        return false;
    };
    let project = &state["project"];
    let mut y = area.min[1] + 8.0;
    let mut row = |list: &mut PaintList,
                   atlas: &mut TextAtlas,
                   y: &mut f32,
                   key: &str,
                   value: &str| {
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 10.0, *y, area.width() * 0.45, metrics::ROW),
            key,
            palette::TEXT_MUTED,
            Align::Left,
        );
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + area.width() * 0.45, *y, area.width() * 0.5, metrics::ROW),
            value,
            palette::TEXT,
            Align::Left,
        );
        *y += metrics::ROW;
    };
    row(
        list,
        atlas,
        &mut y,
        "Composition",
        project["name"].as_str().unwrap_or("comp-main"),
    );
    row(
        list,
        atlas,
        &mut y,
        "Size",
        &format!(
            "{} × {}",
            project["width"].as_u64().unwrap_or(0),
            project["height"].as_u64().unwrap_or(0)
        ),
    );
    row(
        list,
        atlas,
        &mut y,
        "Frame rate",
        &format!("{} fps", project["fps"].as_u64().unwrap_or(0)),
    );
    row(
        list,
        atlas,
        &mut y,
        "Duration",
        &format!(
            "{} frames",
            project["frames"].as_u64().unwrap_or(0)
        ),
    );
    row(
        list,
        atlas,
        &mut y,
        "Layers",
        &project["layers"].as_array().map(|l| l.len()).unwrap_or(0).to_string(),
    );
    row(
        list,
        atlas,
        &mut y,
        "Revision",
        &state["revision"].as_u64().unwrap_or(0).to_string(),
    );
    y += 8.0;
    paint::rect(
        list,
        Rect::new(area.min[0] + 10.0, y, area.width() - 20.0, 1.0),
        palette::BORDER,
    );
    y += 8.0;
    let capabilities = &state["capabilities"];
    ui::text(
        list,
        atlas,
        Rect::new(area.min[0] + 10.0, y, area.width() - 20.0, metrics::ROW),
        "Capabilities",
        palette::ACCENT,
        Align::Left,
    );
    y += metrics::ROW + 4.0;
    for (key, label) in [
        ("layer_3d", "3D layers"),
        ("vector_drawing", "Vector drawing"),
        ("layer_masks", "Masks"),
        ("property_expressions", "Expressions"),
        ("scene_effects", "Scene effects"),
        ("native_plugin_ui", "Plugin editors"),
        ("multiple_compositions", "Nested compositions"),
    ] {
        let supported = capabilities[key]["supported"]
            .as_bool()
            .or_else(|| capabilities[key].as_bool())
            .unwrap_or(false);
        let dot = Rect::new(area.min[0] + 12.0, y + 6.0, 6.0, 6.0);
        paint::rect(
            list,
            dot,
            if supported {
                palette::ACCENT
            } else {
                palette::TEXT_MUTED
            },
        );
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 24.0, y, area.width() - 34.0, metrics::ROW),
            label,
            palette::TEXT_MUTED,
            Align::Left,
        );
        y += metrics::ROW;
    }
    let _ = input;
    true
}

/// Effect Controls panel: the selected layer's transform and effect instances.
fn properties_panel(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    state: Option<&Value>,
    input: &Input,
) -> bool {
    paint::rect(list, area, palette::PANEL);
    let Some(state) = state else {
        return false;
    };
    let layers = state["sampledLayers"].as_array().cloned().unwrap_or_default();
    let Some(layer) = layers.first() else {
        ui::text(
            list,
            atlas,
            area,
            "No layer selected",
            palette::TEXT_MUTED,
            Align::Left,
        );
        return false;
    };
    let name = layer["id"].as_u64().map(|id| format!("Layer {id}")).unwrap_or_default();
    ui::text(
        list,
        atlas,
        Rect::new(area.min[0] + 8.0, area.min[1] + 4.0, area.width() - 16.0, metrics::ROW),
        &name,
        palette::TEXT,
        Align::Left,
    );
    let mut y = area.min[1] + metrics::ROW + 8.0;
    for (property, keys) in [
        ("Position", ["x", "y", "z"].as_slice()),
        ("Rotation", ["angle"].as_slice()),
        ("Scale", ["x", "y"].as_slice()),
        ("Opacity", ["value"].as_slice()),
    ] {
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 8.0, y, 84.0, metrics::PROPERTY_ROW),
            property,
            palette::TEXT_MUTED,
            Align::Left,
        );
        let value = layer[property.to_ascii_lowercase()][0]
            .as_f64()
            .unwrap_or_default();
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 96.0, y, area.width() - 110.0, metrics::PROPERTY_ROW),
            &format!("{}  {}", keys.join("  "), ui::format_value(value as f32)),
            palette::TEXT,
            Align::Left,
        );
        let stopwatch = Rect::new(area.max[0] - 20.0, y, 14.0, metrics::PROPERTY_ROW);
        ui::icon(list, stopwatch, Icon::Stopwatch);
        y += metrics::PROPERTY_ROW;
    }
    y += 6.0;
    let effects = state["project"]["layers"][0]["effects"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ui::text(
        list,
        atlas,
        Rect::new(area.min[0] + 8.0, y, area.width() - 16.0, metrics::ROW),
        "Effects",
        palette::ACCENT,
        Align::Left,
    );
    y += metrics::ROW + 4.0;
    if effects.is_empty() {
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 8.0, y, area.width() - 16.0, metrics::ROW),
            "No effects applied",
            palette::TEXT_MUTED,
            Align::Left,
        );
    }
    for effect in effects {
        let label = effect["effect"].as_str().unwrap_or("effect");
        ui::text(
            list,
            atlas,
            Rect::new(area.min[0] + 16.0, y, area.width() - 24.0, metrics::PROPERTY_ROW),
            label,
            palette::TEXT,
            Align::Left,
        );
        let enabled = effect["enabled"].as_bool().unwrap_or(true);
        let toggle = Rect::new(area.min[0] + 2.0, y, 14.0, metrics::PROPERTY_ROW);
        ui::icon(
            list,
            toggle,
            if enabled { Icon::Eye } else { Icon::EyeOff },
        );
        y += metrics::PROPERTY_ROW;
    }
    let _ = input;
    true
}

/// Composition viewer: the composition bounds plus the transport controls.
fn composition_panel(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    state: Option<&Value>,
    input: &Input,
) -> bool {
    paint::rect(list, area, palette::SUNKEN);
    let Some(state) = state else {
        return false;
    };
    let width = state["project"]["width"].as_f64().unwrap_or(1.0) as f32;
    let height = state["project"]["height"].as_f64().unwrap_or(1.0) as f32;
    let transport = 26.0;
    let viewer = Rect::new(
        area.min[0],
        area.min[1],
        area.width(),
        (area.height() - transport).max(0.0),
    );
    let scale = (viewer.width() / width).min(viewer.height() / height).max(0.01);
    let shown = Rect::new(
        viewer.min[0] + (viewer.width() - width * scale) * 0.5,
        viewer.min[1] + (viewer.height() - height * scale) * 0.5,
        width * scale,
        height * scale,
    );
    paint::checkerboard(list, shown, palette::TRANSPARENT_LIGHT, palette::TRANSPARENT_DARK, 8.0);
    paint::stroke(list, shown, palette::BORDER, 1.0);
    // The composition itself is presented by the engine underneath this chrome,
    // so the viewer frame only draws the surround and the bounds outline.
    let frame = state["frame"].as_f64().unwrap_or(0.0);
    let total = state["project"]["frames"].as_f64().unwrap_or(1.0).max(1.0);
    let controls = Rect::new(area.min[0], viewer.max[1], area.width(), transport);
    paint::rect(list, controls, palette::PANEL);
    let mut x = controls.min[0] + 6.0;
    for (label, enabled) in [("|<", false), ("<<", true), (">>", true), (">|", false)] {
        let item = Rect::new(x, controls.min[1] + 3.0, 30.0, transport - 6.0);
        ui::button(list, atlas, item, label, input, enabled);
        x += 34.0;
    }
    ui::text(
        list,
        atlas,
        Rect::new(x + 8.0, controls.min[1], area.width() - x - 8.0, transport),
        &format!("{frame:.2} / {:.0}  ·  {}%", frame, (frame / total * 100.0)),
        palette::TEXT,
        Align::Center,
    );
    true
}

/// Timeline: ruler, layer rows, clip bars and keyframe diamonds.
fn timeline_panel(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    state: Option<&Value>,
    input: &Input,
) -> bool {
    paint::rect(list, area, palette::SUNKEN);
    let ruler = Rect::new(area.min[0], area.min[1], area.width(), 22.0);
    paint::rect(list, ruler, palette::PANEL);
    let Some(state) = state else {
        return false;
    };
    let total = state["project"]["frames"].as_u64().unwrap_or(1).max(1) as f32;
    let track_width = (area.width() - metrics::RULER).max(1.0);
    let pixels_per_frame = track_width / total;
    // Ruler ticks every second, labelled every five seconds.
    let fps = state["project"]["fps"].as_u64().unwrap_or(30).max(1) as f32;
    let mut second = 0.0;
    while second < total / fps {
        let x = area.min[0] + metrics::RULER + second * fps * pixels_per_frame;
        paint::rect(
            list,
            Rect::new(x, ruler.max[1] - 5.0, 1.0, 5.0),
            palette::TEXT_MUTED,
        );
        if (second as i64) % 5 == 0 {
            ui::text(
                list,
                atlas,
                Rect::new(x + 2.0, ruler.min[1], 60.0, ruler.height()),
                &format!("{second:.0}s"),
                palette::TEXT_MUTED,
                Align::Left,
            );
        }
        second += 1.0;
    }
    // Playhead.
    let frame = state["frame"].as_f64().unwrap_or(0.0) as f32;
    let playhead = area.min[0] + metrics::RULER + frame * pixels_per_frame;
    paint::rect(
        list,
        Rect::new(playhead, area.min[1], 1.0, area.height()),
        palette::ACCENT_WARM,
    );
    // Rows: camera first, then layers in reverse stacking order like After Effects.
    let rows = timeline_rows(state);
    let body = Rect::new(
        area.min[0],
        ruler.max[1],
        area.width(),
        (area.height() - ruler.height()).max(0.0),
    );
    let visible = (body.height() / metrics::ROW).ceil() as usize;
    for (index, row) in rows.iter().enumerate().take(visible) {
        let top = body.min[1] + index as f32 * metrics::ROW;
        let row_area = Rect::new(area.min[0], top, area.width(), metrics::ROW);
        if row.selected {
            paint::rect(list, row_area, palette::ACCENT.with_alpha(0.16));
        }
        let color = if row.camera {
            palette::TRACK_CAMERA
        } else {
            row.color
        };
        ui::icon(
            list,
            Rect::new(area.min[0] + 12.0, top, 14.0, metrics::ROW),
            if row.visible { Icon::Eye } else { Icon::EyeOff },
        );
        ui::icon(
            list,
            Rect::new(area.min[0] + 28.0, top, 14.0, metrics::ROW),
            if row.locked { Icon::Lock } else { Icon::Unlock },
        );
        ui::text(
            list,
            atlas,
            Rect::new(
                area.min[0] + 46.0,
                top,
                metrics::RULER - 52.0,
                metrics::ROW,
            ),
            &row.name,
            palette::TEXT,
            Align::Left,
        );
        if row.camera {
            ui::icon(
                list,
                Rect::new(area.min[0] + metrics::RULER - 18.0, top, 14.0, metrics::ROW),
                Icon::Camera,
            );
            continue;
        }
        let (in_frame, out_frame) = row.clip.unwrap_or((0.0, total));
        let bar = Rect::new(
            area.min[0] + metrics::RULER + in_frame * pixels_per_frame,
            top + 4.0,
            ((out_frame - in_frame) * pixels_per_frame).max(1.0),
            metrics::ROW - 8.0,
        );
        paint::rect(list, bar, color.with_alpha(0.35));
        paint::stroke(list, bar, color, 1.0);
        for keyframe in &row.keyframes {
            let x = area.min[0] + metrics::RULER + keyframe * pixels_per_frame;
            ui::icon(
                list,
                Rect::new(x - 5.0, top + 3.0, 10.0, metrics::ROW - 6.0),
                Icon::Keyframe,
            );
        }
    }
    if rows.len() > visible {
        let total_height = rows.len() as f32 * metrics::ROW;
        let (thumb, position) = Scroll {
            offset: 0.0,
            pending: 0.0,
            max: (total_height - body.height()).max(0.0),
        }
        .bar(body.height(), total_height);
        let track = Rect::new(
            area.max[0] - 6.0,
            body.min[1],
            6.0,
            body.height(),
        );
        paint::rect(list, track, palette::SUNKEN);
        paint::rect(
            list,
            Rect::new(
                track.min[0],
                track.min[1] + (track.height() - thumb * track.height()) * position,
                track.width(),
                thumb * track.height(),
            ),
            palette::BORDER,
        );
    }
    let _ = input;
    true
}

fn timeline_rows(state: &Value) -> Vec<LayerRow> {
    let mut rows = Vec::new();
    if state["project"]["camera"]["created"].as_bool().unwrap_or(false) {
        rows.push(LayerRow {
            id: 0,
            name: "Camera 1".into(),
            color: palette::TRACK_CAMERA,
            visible: true,
            locked: false,
            selected: false,
            camera: true,
            active: true,
        });
    }
    let frame = state["frame"].as_f64().unwrap_or(0.0);
    let mut index = 0;
    for layer in state["timeline_layers"].as_array().into_iter().flatten().rev() {
        let clip = {
            let start = layer["in_frame"].as_f64().unwrap_or(0.0);
            let end = layer["out_frame"].as_f64().unwrap_or(0.0);
            if end > start {
                Some((start as f32, end as f32))
            } else {
                None
            }
        };
        let keyframes = layer["keyframes"]
            .as_array()
            .map(|keys| keys.iter().filter_map(|k| k.as_f64()).map(|f| f as f32).collect())
            .unwrap_or_default();
        rows.push(LayerRow {
            id: layer["id"].as_u64().unwrap_or(index),
            name: layer["name"].as_str().unwrap_or("Layer").to_owned(),
            color: palette::TRACKS[index % palette::TRACKS.len()],
            visible: layer["visible"].as_bool().unwrap_or(true),
            locked: layer["locked"].as_bool().unwrap_or(false),
            selected: false,
            camera: false,
            active: layer["active"].as_bool().unwrap_or(true) || clip.is_some() && {
                let (start, end) = clip.unwrap();
                frame >= f64::from(start) && frame <= f64::from(end)
            },
        });
        index += 1;
    }
    rows
}

fn status_bar(list: &mut PaintList, atlas: &mut TextAtlas, area: Rect, message: &str, scale: f32) {
    let bar = Rect::new(0.0, area.max[1] - 20.0 * scale, area.width(), 20.0 * scale);
    paint::rect(list, bar, palette::CHROME);
    ui::text(
        list,
        atlas,
        bar.shrink(8.0),
        message,
        palette::KEYFRAME,
        Align::Left,
    );
}

/// Re-exported so the shell can drive undo without importing the editing module.
pub const UNDO: i32 = editing::HISTORY_UNDO;
pub const REDO: i32 = editing::HISTORY_REDO;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_height_covers_the_menu_bar_and_toolbar() {
        assert_eq!(CHROME_HEIGHT, metrics::MENU_BAR + metrics::TOOL_BAR);
    }

    #[test]
    fn every_default_dock_is_drawn_once() {
        let area = Rect::new(0.0, CHROME_HEIGHT, 1600.0, 900.0 - CHROME_HEIGHT);
        let found = frames(&dock::editor_layout(), area)
            .into_iter()
            .map(|f| f.panel)
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            vec![
                Panel::Project,
                Panel::EffectControls,
                Panel::Composition,
                Panel::Timeline
            ]
        );
    }

    #[test]
    fn effect_controls_tabs_never_repeat_the_base_panel() {
        let mut panels = Panels::default();
        panels.control_tabs = vec![Panel::EffectControls, Panel::Effects, Panel::Masks];
        let tabs = panels.tabs_for(Panel::EffectControls);
        assert_eq!(tabs, vec![Panel::EffectControls, Panel::Effects, Panel::Masks]);
    }

    #[test]
    fn timeline_rows_put_the_camera_first_and_reverse_layer_order() {
        let state = serde_json::json!({
            "frame": 0,
            "project": {"camera": {"created": true}, "frames": 30},
            "timeline_layers": [
                {"id": 1, "name": "Bottom", "in_frame": 0, "out_frame": 10, "active": true},
                {"id": 2, "name": "Top", "in_frame": 0, "out_frame": 10, "active": true}
            ]
        });
        let rows = timeline_rows(&state);
        assert_eq!(rows[0].name, "Camera 1");
        assert!(rows[0].camera);
        assert_eq!(rows[1].name, "Top");
        assert_eq!(rows[2].name, "Bottom");
        assert_eq!(rows[1].clip, Some((0.0, 10.0)));
    }

    #[test]
    fn an_unselected_project_still_renders_without_panicking() {
        let mut list = PaintList::default();
        let mut atlas = TextAtlas::from_system(13.0).expect("system font");
        project_panel(&mut list, &mut atlas, Rect::new(0.0, 0.0, 100.0, 100.0), None, &Input::default());
        assert!(!list.glyphs.is_empty(), "expected the empty-state label");
    }
}