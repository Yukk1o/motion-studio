//! Desktop panels: one fixed workspace, logical-pixel geometry and typed actions.
use crate::i18n::{Locale, Text as T};
use aem_core::{Command, Property};
use aem_ui::{
    dock::{self, Node, Panel},
    input::{Input, Key, MouseButton, Rect},
    paint::{self, PaintList},
    text::TextAtlas,
    theme::{metrics, palette},
    ui::{self, Align, Icon},
};
use serde_json::Value;

pub const CHROME_HEIGHT: f32 = metrics::MENU_BAR + metrics::TOOL_BAR;
const GUTTER: f32 = 440.0;

#[derive(Debug)]
pub enum Action {
    New,
    Open,
    Save,
    AddSolid,
    Pack,
    RefreshState,
    Language(Locale),
    ResetWorkspace,
    ShowPanel(dock::DockId),
    About,
    PreviewMode(i32),
    SelectAt([f64; 2]),
    OpenPath(std::path::PathBuf),
    Float(dock::DockId),
    Redock(dock::DockId),
    Activate {
        dock: dock::DockId,
        tab: dock::DockId,
    },
    History(i32),
    Seek(f64),
    TogglePlay,
    Edit(Command),
}
#[derive(Default)]
pub struct Panels {
    pub locale: Locale,
    query: String,
    search: bool,
    transform_collapsed: bool,
    effects_expanded: bool,
    pub selected: Option<u64>,
    pub playing: bool,
    pub message: Option<String>,
    menu: Option<usize>,
    scroll: f32,
    properties_scroll: f32,
    zoom: f32,
    offset: f32,
    scrubbing: bool,
    moving: Option<(u64, f32, u32, u32, u32)>,
    field: Option<Field>,
}
struct Field {
    object: u64,
    property: Property,
    axis: usize,
    text: String,
    replace: bool,
}
pub struct Frame {
    pub panel: Panel,
    pub tabs: Rect,
    pub content: Rect,
}

pub fn frames(layout: &Node, size: [f32; 2]) -> Vec<Frame> {
    dock::solve(
        layout,
        Rect::new(
            0.0,
            CHROME_HEIGHT,
            size[0],
            (size[1] - CHROME_HEIGHT - 22.0).max(0.0),
        ),
    )
    .into_iter()
    .filter_map(|p| {
        let panel = match p.id.0 {
            1 => Panel::Project,
            2 => Panel::EffectControls,
            3 => Panel::Composition,
            4 => Panel::Timeline,
            _ => return None,
        };
        Some(Frame {
            panel,
            tabs: Rect::new(
                p.area.min[0],
                p.area.min[1],
                p.area.width(),
                metrics::TAB_BAR,
            ),
            content: Rect::new(
                p.area.min[0],
                p.area.min[1] + metrics::TAB_BAR,
                p.area.width(),
                (p.area.height() - metrics::TAB_BAR).max(0.0),
            ),
        })
    })
    .collect()
}
pub fn composition_view(layout: &Node, size: [f32; 2]) -> Rect {
    frames(layout, size)
        .into_iter()
        .find(|f| f.panel == Panel::Composition)
        .map(|f| viewer(f.content))
        .unwrap_or_default()
}
fn viewer(area: Rect) -> Rect {
    Rect::new(
        area.min[0] + 8.0,
        area.min[1] + 8.0,
        (area.width() - 16.0).max(1.0),
        (area.height() - 42.0).max(1.0),
    )
}
fn label(list: &mut PaintList, atlas: &mut TextAtlas, area: Rect, text: &str, muted: bool) {
    ui::text(
        list,
        atlas,
        area,
        text,
        if muted {
            palette::TEXT_MUTED
        } else {
            palette::TEXT
        },
        Align::Left,
    );
}
fn clicked(input: &Input, area: Rect) -> bool {
    matches!(input.pressed, Some((MouseButton::Left, p)) if area.contains(p))
}
fn button(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    text: &str,
    input: &Input,
    enabled: bool,
    action: Action,
    out: &mut Vec<Action>,
) {
    if ui::button(list, atlas, area, text, input, enabled).clicked {
        out.push(action);
    }
}
pub fn timecode(frame: f64, fps: u32) -> String {
    let frame = frame.max(0.0).floor() as u64;
    let fps = u64::from(fps.max(1));
    let seconds = frame / fps;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frame % fps
    )
}
pub fn cancel_interaction(panels: &mut Panels) -> Vec<Action> {
    panels.field = None;
    panels.scrubbing = false;
    if panels.moving.take().is_some() {
        vec![Action::History(4)]
    } else {
        vec![]
    }
}

/// The chrome leaves a hole for the preview; its geometry is also used for hit testing.
pub fn chrome(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    size: [f32; 2],
    layout: &Node,
    panels: &mut Panels,
    state: &Value,
    input: &Input,
) -> Vec<Action> {
    let mut out = Vec::new();
    if let Some(id) = panels.selected {
        if id != 0
            && !state["project"]["layers"]
                .as_array()
                .is_some_and(|ls| ls.iter().any(|l| l["id"].as_u64() == Some(id)))
        {
            panels.selected = None;
            panels.field = None;
        }
    }
    if input.keys.iter().any(|(k, _)| *k == Key::Escape) {
        out.extend(cancel_interaction(panels));
        panels.menu = None;
        panels.search = false;
    }
    let field_active = panels.field.is_some() || panels.search;
    if !field_active {
        for (key, modifiers) in &input.keys {
            match (key, modifiers.control) {
                (Key::N, true) => out.push(Action::New),
                (Key::O, true) => out.push(Action::Open),
                (Key::S, true) => out.push(Action::Save),
                (Key::Z, true) => out.push(Action::History(if modifiers.shift { 1 } else { 0 })),
                (Key::Y, true) => out.push(Action::History(1)),
                (Key::Space, false) => out.push(Action::TogglePlay),
                (Key::Left, false) => out.push(Action::Seek(
                    (state["frame"].as_f64().unwrap_or(0.0) - 1.0).max(0.0),
                )),
                (Key::Right, false) => out.push(Action::Seek(
                    (state["frame"].as_f64().unwrap_or(0.0) + 1.0).min(last_frame(state)),
                )),
                (Key::Home, false) => out.push(Action::Seek(0.0)),
                (Key::End, false) => out.push(Action::Seek(last_frame(state))),
                (Key::D, true) => {
                    if let Some(object) = panels.selected.filter(|id| *id != 0) {
                        out.push(Action::Edit(Command::Duplicate { object }));
                    }
                }
                (Key::Delete, false) => {
                    if let Some(object) = panels.selected.filter(|id| *id != 0) {
                        out.push(Action::Edit(Command::Delete { object }));
                    }
                }
                _ => {}
            }
        }
    }
    let menu_modal = panels.menu.is_some();
    let neutral = Input::default();
    let body_input = if menu_modal { &neutral } else { input };
    for f in frames(layout, size) {
        let title = match f.panel {
            Panel::EffectControls => "Properties / Effect Controls",
            _ => panels.locale.panel(f.panel.dock()),
        };
        let (dock, tabs, active) = match layout.find(f.panel.dock()) {
            Some(Node::Dock { id, tabs, active }) => (*id, tabs.clone(), *active),
            _ => (f.panel.dock(), vec![f.panel.dock()], 0),
        };
        let titles: Vec<_> = tabs.iter().map(|id| panels.locale.panel(*id)).collect();
        let tab_area = Rect::new(
            f.tabs.min[0],
            f.tabs.min[1],
            (f.tabs.width() - 28.0).max(1.0),
            f.tabs.height(),
        );
        if let Some(index) = ui::tab_strip(list, atlas, tab_area, &titles, active, body_input) {
            out.push(Action::Activate {
                dock,
                tab: tabs[index],
            });
        }
        let detach = Rect::new(f.tabs.max[0] - 27.0, f.tabs.min[1] + 3.0, 24.0, 20.0);
        button(
            list,
            atlas,
            detach,
            "↗",
            body_input,
            true,
            Action::Float(f.panel.dock()),
            &mut out,
        );
        let _ = title;
        let mut body = PaintList::default();
        match f.panel {
            Panel::Composition => composition(
                &mut body, atlas, f.content, panels, state, body_input, &mut out,
            ),
            Panel::Project => project(&mut body, atlas, f.content, panels, state, body_input),
            Panel::EffectControls => properties(
                &mut body, atlas, f.content, panels, state, body_input, &mut out,
            ),
            Panel::Timeline => timeline(
                &mut body, atlas, f.content, panels, state, body_input, &mut out,
            ),
            _ => {}
        }
        body.clip_to(f.content);
        list.append(body);
        paint::stroke(list, f.content, palette::BORDER, 1.0);
    }
    let toolbar = Rect::new(0.0, metrics::MENU_BAR, size[0], metrics::TOOL_BAR);
    paint::rect(list, toolbar, palette::CHROME);
    let locale = panels.locale;
    for (i, (symbol, action, enabled)) in [
        (Tool::Open, Action::Open, true),
        (Tool::Save, Action::Save, true),
        (
            Tool::Undo,
            Action::History(0),
            state["canUndo"].as_bool().unwrap_or(false),
        ),
        (
            Tool::Redo,
            Action::History(1),
            state["canRedo"].as_bool().unwrap_or(false),
        ),
        (Tool::Solid, Action::AddSolid, true),
    ]
    .into_iter()
    .enumerate()
    {
        icon_button(
            list,
            Rect::new(12.0 + i as f32 * 44.0, toolbar.min[1] + 6.0, 34.0, 30.0),
            symbol,
            body_input,
            enabled,
            action,
            &mut out,
        );
    }
    paint::rect(
        list,
        Rect::new(240.0, toolbar.min[1] + 8.0, 1.0, 26.0),
        palette::BORDER,
    );
    label(
        list,
        atlas,
        Rect::new(
            260.0,
            toolbar.min[1],
            (size[0] - 490.0).max(0.0),
            toolbar.height(),
        ),
        &format!(
            "Space: {}     {}",
            locale.t(T::Play),
            locale.t(T::FrameStep)
        ),
        true,
    );
    button(
        list,
        atlas,
        Rect::new(
            (size[0] - 214.0).max(520.0),
            toolbar.min[1] + 6.0,
            88.0,
            30.0,
        ),
        locale.t(T::Default),
        body_input,
        true,
        Action::ResetWorkspace,
        &mut out,
    );
    button(
        list,
        atlas,
        Rect::new(
            (size[0] - 116.0).max(618.0),
            toolbar.min[1] + 6.0,
            104.0,
            30.0,
        ),
        if locale == Locale::Zh {
            "中文 / EN"
        } else {
            "EN / 中文"
        },
        body_input,
        true,
        Action::Language(if locale == Locale::Zh {
            Locale::En
        } else {
            Locale::Zh
        }),
        &mut out,
    );
    let status = Rect::new(0.0, size[1] - 22.0, size[0], 22.0);
    paint::rect(list, status, palette::CHROME);
    let render_error = state["renderError"].as_str().filter(|s| !s.is_empty());
    let localized_error = render_error.map(|e| panels.locale.error(e));
    let message = localized_error
        .as_deref()
        .or(panels.message.as_deref())
        .unwrap_or(panels.locale.t(T::Ready));
    label(
        list,
        atlas,
        Rect::new(
            8.0,
            status.min[1],
            (status.width() - 16.0).max(0.0),
            status.height(),
        ),
        message,
        render_error.is_none(),
    );
    menus(list, atlas, size, panels, input, state, &mut out);
    out
}
pub fn panel(id: dock::DockId) -> Option<Panel> {
    Some(match id.0 {
        1 => Panel::Project,
        2 => Panel::EffectControls,
        3 => Panel::Composition,
        4 => Panel::Timeline,
        _ => return None,
    })
}
pub fn floating_view(panel: Panel, size: [f32; 2]) -> Option<Rect> {
    if panel == Panel::Composition {
        Some(viewer(Rect::new(
            0.0,
            30.0,
            size[0],
            (size[1] - 52.0).max(1.0),
        )))
    } else {
        None
    }
}
pub fn floating_chrome(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    panel: Panel,
    size: [f32; 2],
    panels: &mut Panels,
    state: &Value,
    input: &Input,
) -> Vec<Action> {
    let mut out = vec![];
    let body = Rect::new(0.0, 30.0, size[0], (size[1] - 52.0).max(1.0));
    let mut content = PaintList::default();
    match panel {
        Panel::Project => project(&mut content, atlas, body, panels, state, input),
        Panel::Composition => {
            composition(&mut content, atlas, body, panels, state, input, &mut out)
        }
        Panel::EffectControls => {
            properties(&mut content, atlas, body, panels, state, input, &mut out)
        }
        Panel::Timeline => timeline(&mut content, atlas, body, panels, state, input, &mut out),
        _ => {}
    }
    content.clip_to(body);
    list.append(content);
    paint::rect(list, Rect::new(0.0, 0.0, size[0], 30.0), palette::CHROME);
    label(
        list,
        atlas,
        Rect::new(8.0, 0.0, (size[0] - 100.0).max(1.0), 30.0),
        panels.locale.panel(panel.dock()),
        false,
    );
    button(
        list,
        atlas,
        Rect::new(size[0] - 78.0, 4.0, 72.0, 22.0),
        panels.locale.t(T::Dock),
        input,
        true,
        Action::Redock(panel.dock()),
        &mut out,
    );
    paint::rect(
        list,
        Rect::new(0.0, size[1] - 22.0, size[0], 22.0),
        palette::CHROME,
    );
    label(
        list,
        atlas,
        Rect::new(8.0, size[1] - 22.0, (size[0] - 16.0).max(1.0), 22.0),
        panels
            .message
            .as_deref()
            .unwrap_or(panels.locale.t(T::SharedProject)),
        true,
    );
    out
}
fn last_frame(state: &Value) -> f64 {
    state["project"]["frames"].as_f64().unwrap_or(1.0).max(1.0) - 1.0
}
fn project(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    panels: &mut Panels,
    state: &Value,
    input: &Input,
) {
    paint::rect(list, area, palette::PANEL);
    let locale = panels.locale;
    let p = &state["project"];
    let raw_name = p["name"].as_str().unwrap_or(locale.t(T::Untitled));
    let name = if raw_name == "Untitled Project" {
        locale.t(T::Untitled)
    } else {
        raw_name
    };
    let thumb = Rect::new(area.min[0] + 12.0, area.min[1] + 16.0, 96.0, 70.0);
    paint::rect(list, thumb, palette::SUNKEN);
    paint::stroke(list, thumb, palette::BORDER, 1.0);
    let x = thumb.max[0] + 12.0;
    let width = (area.max[0] - x - 10.0).max(1.0);
    label(
        list,
        atlas,
        Rect::new(x, thumb.min[1], width, 24.0),
        name,
        false,
    );
    let fps = p["fps"].as_u64().unwrap_or(30) as u32;
    label(
        list,
        atlas,
        Rect::new(x, thumb.min[1] + 26.0, width, 22.0),
        &format!("{} × {}", p["width"], p["height"]),
        true,
    );
    label(
        list,
        atlas,
        Rect::new(x, thumb.min[1] + 48.0, width, 22.0),
        &format!("{} {}", fps, locale.t(T::Fps)),
        true,
    );
    label(
        list,
        atlas,
        Rect::new(
            area.min[0] + 12.0,
            area.min[1] + 100.0,
            area.width() - 24.0,
            22.0,
        ),
        &format!(
            "{} {}",
            locale.t(T::Duration),
            timecode(p["frames"].as_f64().unwrap_or(0.0), fps)
        ),
        true,
    );
    let search = Rect::new(
        area.min[0] + 12.0,
        area.min[1] + 146.0,
        area.width() - 24.0,
        28.0,
    );
    paint::rect(list, search, palette::SUNKEN);
    paint::stroke(
        list,
        search,
        if panels.search {
            palette::ACCENT
        } else {
            palette::BORDER
        },
        1.0,
    );
    let focus = clicked(input, search);
    if focus {
        panels.search = true;
    } else if input.pressed.is_some() {
        panels.search = false;
    }
    if panels.search {
        panels.query.push_str(&input.text);
        if input.keys.iter().any(|(k, _)| *k == Key::Backspace) {
            panels.query.pop();
        }
    }
    label(
        list,
        atlas,
        search.shrink(6.0),
        if panels.query.is_empty() {
            locale.t(T::Search)
        } else {
            &panels.query
        },
        panels.query.is_empty(),
    );
    let header = Rect::new(area.min[0], search.max[1] + 10.0, area.width(), 28.0);
    paint::rect(list, header, palette::CHROME);
    label(
        list,
        atlas,
        Rect::new(
            header.min[0] + 14.0,
            header.min[1],
            header.width() * 0.62,
            28.0,
        ),
        locale.t(T::SourceName),
        true,
    );
    label(
        list,
        atlas,
        Rect::new(
            header.min[0] + header.width() * 0.64,
            header.min[1],
            header.width() * 0.34,
            28.0,
        ),
        locale.t(T::Type),
        true,
    );
    if name.to_lowercase().contains(&panels.query.to_lowercase()) {
        let row = Rect::new(
            area.min[0] + 8.0,
            header.max[1] + 1.0,
            area.width() - 16.0,
            32.0,
        );
        paint::rect(list, row, palette::CHROME.with_alpha(0.45));
        paint::rect(
            list,
            Rect::new(row.min[0] + 6.0, row.min[1] + 9.0, 12.0, 14.0),
            palette::ACCENT,
        );
        label(
            list,
            atlas,
            Rect::new(
                row.min[0] + 26.0,
                row.min[1],
                row.width() * 0.59 - 26.0,
                32.0,
            ),
            name,
            false,
        );
        label(
            list,
            atlas,
            Rect::new(
                row.min[0] + row.width() * 0.64,
                row.min[1],
                row.width() * 0.34,
                32.0,
            ),
            locale.t(T::Composition),
            true,
        );
    }
    let footer = Rect::new(area.min[0], area.max[1] - 30.0, area.width(), 30.0);
    paint::rect(list, footer, palette::CHROME);
    label(
        list,
        atlas,
        footer.shrink(10.0),
        &format!(
            "1 {}   ·   {} {}",
            locale.t(T::Items),
            p["layers"].as_array().map_or(0, Vec::len),
            locale.t(T::Layers)
        ),
        true,
    );
}
fn composition(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    panels: &Panels,
    state: &Value,
    input: &Input,
    out: &mut Vec<Action>,
) {
    let v = viewer(area);
    // Paint the viewer surround, leaving its contents to the shared GPU renderer.
    for r in [
        Rect::new(
            area.min[0],
            area.min[1],
            area.width(),
            v.min[1] - area.min[1],
        ),
        Rect::new(area.min[0], v.min[1], v.min[0] - area.min[0], v.height()),
        Rect::new(v.max[0], v.min[1], area.max[0] - v.max[0], v.height()),
        Rect::new(area.min[0], v.max[1], area.width(), area.max[1] - v.max[1]),
    ] {
        paint::rect(list, r, palette::SUNKEN);
    }
    let controls = Rect::new(
        area.min[0] + 8.0,
        area.max[1] - 29.0,
        area.width() - 16.0,
        24.0,
    );
    button(
        list,
        atlas,
        Rect::new(controls.min[0], controls.min[1], 30.0, 24.0),
        "|<",
        input,
        true,
        Action::Seek(0.0),
        out,
    );
    button(
        list,
        atlas,
        Rect::new(controls.min[0] + 36.0, controls.min[1], 54.0, 24.0),
        if panels.playing {
            panels.locale.t(T::Pause)
        } else {
            panels.locale.t(T::Play)
        },
        input,
        true,
        Action::TogglePlay,
        out,
    );
    let fps = state["project"]["fps"].as_u64().unwrap_or(30) as u32;
    label(
        list,
        atlas,
        Rect::new(
            controls.min[0] + 108.0,
            controls.min[1],
            controls.width() - 108.0,
            24.0,
        ),
        &format!(
            "{}   ·   {}",
            timecode(state["frame"].as_f64().unwrap_or(0.0), fps),
            panels.locale.t(T::Fit)
        ),
        false,
    );
    let (quality, next) = match state["preview"]["mode"].as_str().unwrap_or("auto") {
        "high" => (T::Full, 2),
        "balanced" => (T::Smooth, 3),
        "economy" => (T::Economy, 0),
        _ => (T::Auto, 1),
    };
    button(
        list,
        atlas,
        Rect::new(controls.max[0] - 86.0, controls.min[1], 80.0, 24.0),
        panels.locale.t(quality),
        input,
        true,
        Action::PreviewMode(next),
        out,
    );
    if clicked(input, v) {
        let w = state["project"]["width"].as_f64().unwrap_or(1.0);
        let h = state["project"]["height"].as_f64().unwrap_or(1.0);
        let scale = (v.width() as f64 / w).min(v.height() as f64 / h);
        let ox = v.min[0] as f64 + (v.width() as f64 - w * scale) * 0.5;
        let oy = v.min[1] as f64 + (v.height() as f64 - h * scale) * 0.5;
        out.push(Action::SelectAt([
            (input.mouse[0] as f64 - ox) / scale,
            (input.mouse[1] as f64 - oy) / scale,
        ]));
    }
}
fn properties(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    panels: &mut Panels,
    state: &Value,
    input: &Input,
    out: &mut Vec<Action>,
) {
    paint::rect(list, area, palette::PANEL);
    let filtered = Input {
        pressed: input.pressed.filter(|(_, p)| area.contains(*p)),
        ..input.clone()
    };
    let input = &filtered;
    let Some(object) = panels.selected.filter(|id| *id != 0) else {
        label(
            list,
            atlas,
            area.shrink(10.0),
            panels.locale.t(T::SelectLayer),
            true,
        );
        return;
    };
    let Some(layer) = state["project"]["layers"]
        .as_array()
        .and_then(|ls| ls.iter().find(|l| l["id"].as_u64() == Some(object)))
    else {
        return;
    };
    let Some(sample) = state["sampledLayers"]
        .as_array()
        .and_then(|ls| ls.iter().find(|l| l["id"].as_u64() == Some(object)))
    else {
        return;
    };
    let locale = panels.locale;
    let locked = layer["locked"].as_bool().unwrap_or(false);
    let three_d = layer["three_d"].as_bool().unwrap_or(false);
    let timeline = state["timeline_layers"]
        .as_array()
        .and_then(|ls| ls.iter().find(|l| l["object"].as_u64() == Some(object)));
    let content_height = if three_d { 520.0 } else { 420.0 };
    if area.contains(input.mouse) {
        panels.properties_scroll = (panels.properties_scroll + input.wheel[1])
            .clamp(0.0, (content_height - area.height()).max(0.0));
    }
    let mut y = area.min[1] + 8.0 - panels.properties_scroll;
    let heading = Rect::new(area.min[0], y, area.width(), 32.0);
    paint::rect(list, heading, palette::CHROME.with_alpha(0.55));
    label(
        list,
        atlas,
        heading.shrink(10.0),
        layer["name"].as_str().unwrap_or(locale.t(T::Layer)),
        false,
    );
    y += 40.0;
    let check = Rect::new(area.max[0] - 58.0, y + 7.0, 14.0, 14.0);
    paint::stroke(list, check, palette::TEXT_MUTED, 1.0);
    if three_d {
        paint::rect(list, check.shrink(3.0), palette::ACCENT);
    }
    label(
        list,
        atlas,
        Rect::new(area.min[0] + 20.0, y, area.width() - 82.0, 30.0),
        locale.t(T::Enable3D),
        false,
    );
    if clicked(
        input,
        Rect::new(area.min[0] + 12.0, y, area.width() - 24.0, 30.0),
    ) && !locked
    {
        out.push(Action::Edit(Command::SetLayer3d {
            object,
            enabled: !three_d,
        }));
    }
    y += 42.0;
    paint::rect(
        list,
        Rect::new(area.min[0] + 12.0, y, area.width() - 24.0, 1.0),
        palette::BORDER,
    );
    y += 4.0;
    let group = Rect::new(area.min[0] + 10.0, y, area.width() - 20.0, 30.0);
    label(
        list,
        atlas,
        group,
        &format!(
            "{}  {}",
            if panels.transform_collapsed {
                "›"
            } else {
                "⌄"
            },
            locale.t(T::Transform)
        ),
        false,
    );
    if clicked(input, group) {
        panels.transform_collapsed = !panels.transform_collapsed;
        panels.field = None;
    }
    y += 34.0;
    let frame = state["frame"].as_f64().unwrap_or(0.0).floor() as u32;
    if !panels.transform_collapsed {
        for (name, property, key, axes) in [
            (
                locale.t(T::Position),
                Property::Position,
                "position",
                if three_d { vec![0, 1, 2] } else { vec![0, 1] },
            ),
            (
                locale.t(T::Rotation),
                Property::Rotation,
                "rotation",
                if three_d { vec![0, 1, 2] } else { vec![2] },
            ),
            (
                locale.t(T::Scale),
                Property::Scale,
                "scale",
                if three_d { vec![0, 1, 2] } else { vec![0, 1] },
            ),
            (locale.t(T::Opacity), Property::Opacity, "opacity", vec![0]),
        ] {
            let track = timeline.map(|l| &l["properties"][key]);
            let separated = track.is_some_and(|t| t["separated"].as_bool().unwrap_or(false));
            let animated = track.is_some_and(|t| !t["keys"].as_array().is_none_or(Vec::is_empty));
            let animate = Rect::new(area.min[0] + 14.0, y + 2.0, 14.0, 24.0);
            ui::icon(list, animate, Icon::Stopwatch);
            if animated {
                paint::rect(
                    list,
                    Rect::new(animate.min[0], y + 22.0, 12.0, 2.0),
                    palette::ACCENT,
                );
            }
            if clicked(input, animate) && !locked && !separated {
                out.push(Action::Edit(Command::Animate {
                    object,
                    property,
                    axis: None,
                    frame,
                    enabled: !animated,
                }));
            }
            label(
                list,
                atlas,
                Rect::new(area.min[0] + 38.0, y, area.width() * 0.40, 28.0),
                name,
                true,
            );
            for axis in axes {
                let value = if property == Property::Opacity {
                    sample[key].as_f64().unwrap_or(1.0) * 100.0
                } else {
                    sample[key][axis].as_f64().unwrap_or(0.0)
                };
                let r = Rect::new(
                    area.min[0] + area.width() * 0.59,
                    y,
                    (area.width() * 0.38 - 8.0).max(1.0),
                    28.0,
                );
                let active = panels.field.as_ref().is_some_and(|f| {
                    f.object == object && f.property == property && f.axis == axis
                });
                if active {
                    paint::rect(list, r, palette::SUNKEN);
                    paint::stroke(list, r, palette::ACCENT, 1.0);
                }
                if property != Property::Opacity {
                    label(
                        list,
                        atlas,
                        Rect::new(r.min[0] - 20.0, y, 18.0, 28.0),
                        ["X", "Y", "Z"][axis],
                        true,
                    );
                }
                let text = if active {
                    panels.field.as_ref().unwrap().text.clone()
                } else {
                    format!(
                        "{value:.2}{}",
                        if matches!(property, Property::Scale | Property::Opacity) {
                            "%"
                        } else {
                            ""
                        }
                    )
                };
                ui::text(list, atlas, r, &text, palette::ACCENT, Align::Left);
                if clicked(input, r) && !locked {
                    panels.field = Some(Field {
                        object,
                        property,
                        axis,
                        text: format!("{value:.2}"),
                        replace: true,
                    });
                }
                y += 28.0;
            }
            y += 6.0;
        }
    }
    if let Some(field) = panels.field.as_mut() {
        if !input.text.is_empty() {
            if field.replace {
                field.text.clear();
                field.replace = false;
            }
            field.text.push_str(&input.text);
        }
        if input.keys.iter().any(|(k, _)| *k == Key::Backspace) {
            if field.replace {
                field.text.clear();
                field.replace = false;
            } else {
                field.text.pop();
            }
        }
        if input.keys.iter().any(|(k, _)| *k == Key::Enter) {
            if let Ok(value) = field.text.parse::<f32>() {
                if value.is_finite() {
                    if field.property == Property::Opacity {
                        out.push(Action::Edit(Command::SetScalar {
                            object,
                            property: field.property,
                            frame,
                            value: value / 100.0,
                        }));
                    } else {
                        let key = match field.property {
                            Property::Position => "position",
                            Property::Rotation => "rotation",
                            _ => "scale",
                        };
                        let separated = timeline.is_some_and(|t| {
                            t["properties"][key]["separated"].as_bool().unwrap_or(false)
                        });
                        if separated {
                            out.push(Action::Edit(Command::SetComponent {
                                object,
                                property: field.property,
                                axis: [aem_core::Axis::X, aem_core::Axis::Y, aem_core::Axis::Z]
                                    [field.axis],
                                frame,
                                value,
                            }));
                        } else {
                            let mut xyz = std::array::from_fn(|i| {
                                sample[key][i].as_f64().unwrap_or(0.0) as f32
                            });
                            xyz[field.axis] = value;
                            out.push(Action::Edit(Command::SetVector {
                                object,
                                property: field.property,
                                frame,
                                value: xyz,
                            }));
                        }
                    }
                    panels.field = None;
                } else {
                    panels.message = Some(locale.t(T::EnterFinite).into());
                }
            } else {
                panels.message = Some(locale.t(T::EnterValid).into());
            }
        }
    }
    y += 8.0;
    paint::rect(
        list,
        Rect::new(area.min[0] + 12.0, y, area.width() - 24.0, 1.0),
        palette::BORDER,
    );
    y += 4.0;
    let effects = Rect::new(area.min[0] + 12.0, y, area.width() - 24.0, 30.0);
    label(
        list,
        atlas,
        effects,
        &format!(
            "{}  {}",
            if panels.effects_expanded {
                "⌄"
            } else {
                "›"
            },
            locale.t(T::Effects)
        ),
        false,
    );
    if clicked(input, effects) {
        panels.effects_expanded = !panels.effects_expanded;
    }
    if panels.effects_expanded {
        y += 34.0;
        if layer["effects"].as_array().is_none_or(Vec::is_empty) {
            label(
                list,
                atlas,
                Rect::new(area.min[0] + 24.0, y, area.width() - 36.0, 28.0),
                locale.t(T::NoEffects),
                true,
            );
        }
        for effect in layer["effects"].as_array().into_iter().flatten() {
            label(
                list,
                atlas,
                Rect::new(area.min[0] + 24.0, y, area.width() - 36.0, 28.0),
                effect["effect"].as_str().unwrap_or(locale.t(T::Effect)),
                true,
            );
            y += 28.0;
        }
    }
}
struct Row {
    id: u64,
    name: String,
    visible: bool,
    locked: bool,
    clip: [u32; 2],
    keys: Vec<f64>,
}
fn rows(state: &Value) -> Vec<Row> {
    let total = state["project"]["frames"].as_u64().unwrap_or(1) as u32;
    state["project"]["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .map(|l| {
            let id = l["id"].as_u64().unwrap_or(0);
            let t = state["timeline_layers"]
                .as_array()
                .and_then(|ts| ts.iter().find(|t| t["object"].as_u64() == Some(id)));
            let mut keys = Vec::new();
            if let Some(t) = t {
                for property in ["position", "rotation", "scale", "opacity"] {
                    let track = &t["properties"][property];
                    for k in track["keys"].as_array().into_iter().flatten() {
                        if let Some(f) = k["frame"].as_f64() {
                            keys.push(f);
                        }
                    }
                    for axis in ["x", "y", "z"] {
                        for k in track["axes"][axis]["keys"].as_array().into_iter().flatten() {
                            if let Some(f) = k["frame"].as_f64() {
                                keys.push(f);
                            }
                        }
                    }
                }
            }
            keys.sort_by(|a, b| a.total_cmp(b));
            keys.dedup();
            Row {
                id,
                name: l["name"].as_str().unwrap_or("Layer").into(),
                visible: l["visible"].as_bool().unwrap_or(true),
                locked: l["locked"].as_bool().unwrap_or(false),
                clip: [
                    t.and_then(|t| t["in_frame"].as_u64()).unwrap_or(0) as u32,
                    t.and_then(|t| t["out_frame"].as_u64())
                        .unwrap_or(u64::from(total)) as u32,
                ],
                keys,
            }
        })
        .collect()
}
fn timeline(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    area: Rect,
    panels: &mut Panels,
    state: &Value,
    input: &Input,
    out: &mut Vec<Action>,
) {
    paint::rect(list, area, palette::SUNKEN);
    let total = last_frame(state) + 1.0;
    let rows = rows(state);
    let gutter = GUTTER.min(area.width() * 0.4);
    let track = Rect::new(
        area.min[0] + gutter,
        area.min[1],
        (area.width() - gutter - 8.0).max(1.0),
        area.height(),
    );
    let ruler = Rect::new(track.min[0], area.min[1] + 36.0, track.width(), 28.0);
    if area.contains(input.mouse) {
        if input.modifiers.control {
            panels.zoom =
                (panels.zoom.max(1.0) * (input.wheel[1] * -0.005).exp()).clamp(1.0, 100.0);
        } else if input.modifiers.shift {
            panels.offset = (panels.offset + input.wheel[1] as f32).max(0.0);
        } else {
            panels.scroll = (panels.scroll + input.wheel[1]).clamp(
                0.0,
                (rows.len() as f32 * metrics::ROW - (area.height() - 64.0)).max(0.0),
            );
        }
    }
    panels.offset = panels
        .offset
        .min((total as f32 - total as f32 / panels.zoom.max(1.0)).max(0.0));
    let ppf = track.width() / total as f32 * panels.zoom.max(1.0);
    let x_of = |f: f64| track.min[0] + (f as f32 - panels.offset) * ppf;
    let frame_of = |x: f32| {
        ((x - track.min[0]) / ppf + panels.offset)
            .round()
            .clamp(0.0, (total - 1.0) as f32) as f64
    };
    paint::rect(
        list,
        Rect::new(area.min[0], area.min[1], area.width(), 64.0),
        palette::CHROME,
    );
    label(
        list,
        atlas,
        Rect::new(area.min[0] + 10.0, area.min[1], gutter - 20.0, 30.0),
        &timecode(
            state["frame"].as_f64().unwrap_or(0.0),
            state["project"]["fps"].as_u64().unwrap_or(30) as u32,
        ),
        false,
    );
    let locale = panels.locale;
    for (i, (tool, action)) in [
        (Tool::First, Action::Seek(0.0)),
        (
            if panels.playing {
                Tool::Pause
            } else {
                Tool::Play
            },
            Action::TogglePlay,
        ),
        (Tool::Last, Action::Seek(last_frame(state))),
    ]
    .into_iter()
    .enumerate()
    {
        icon_button(
            list,
            Rect::new(
                area.min[0] + 146.0 + i as f32 * 38.0,
                area.min[1] + 2.0,
                32.0,
                28.0,
            ),
            tool,
            input,
            true,
            action,
            out,
        );
    }
    label(
        list,
        atlas,
        Rect::new(
            area.min[0] + 90.0,
            ruler.min[1],
            (gutter - 212.0).max(1.0),
            28.0,
        ),
        locale.t(T::SourceName),
        true,
    );
    label(
        list,
        atlas,
        Rect::new(area.min[0] + gutter - 112.0, ruler.min[1], 104.0, 28.0),
        locale.t(T::ParentLink),
        true,
    );
    ui::icon(
        list,
        Rect::new(area.min[0] + 10.0, ruler.min[1] + 3.0, 16.0, 24.0),
        Icon::Eye,
    );
    ui::icon(
        list,
        Rect::new(area.min[0] + 30.0, ruler.min[1] + 3.0, 16.0, 24.0),
        Icon::Lock,
    );
    let fps = state["project"]["fps"].as_u64().unwrap_or(30).max(1) as f32;
    let step = (60.0 / (ppf * fps)).ceil().max(1.0) * fps;
    let mut f = (panels.offset / step).ceil() * step;
    while x_of(f as f64) < track.max[0] {
        let x = x_of(f as f64);
        paint::rect(
            list,
            Rect::new(x, ruler.max[1] - 6.0, 1.0, 6.0),
            palette::TEXT_MUTED,
        );
        label(
            list,
            atlas,
            Rect::new(x + 3.0, ruler.min[1], 52.0, 22.0),
            &format!("{:.0}s", f / fps),
            true,
        );
        f += step;
    }
    if clicked(input, ruler) {
        panels.scrubbing = true;
        out.push(Action::Seek(frame_of(input.mouse[0])));
    }
    if panels.scrubbing && input.dragging() {
        out.push(Action::Seek(frame_of(input.mouse[0])));
    }
    if input.released.is_some() {
        panels.scrubbing = false;
    }
    let body = Rect::new(
        area.min[0],
        ruler.max[1],
        area.width(),
        (area.max[1] - ruler.max[1]).max(0.0),
    );
    for (index, row) in rows.iter().enumerate() {
        let y = body.min[1] + index as f32 * metrics::ROW - panels.scroll;
        if y < body.min[1] || y + metrics::ROW > body.max[1] {
            continue;
        }
        let name = Rect::new(
            area.min[0] + 90.0,
            y,
            (gutter - 214.0).max(1.0),
            metrics::ROW,
        );
        let selected = panels.selected == Some(row.id);
        paint::rect(
            list,
            Rect::new(area.min[0], y, gutter, metrics::ROW),
            if selected {
                palette::ACCENT.with_alpha(0.18)
            } else {
                palette::PANEL
            },
        );
        label(list, atlas, name, &row.name, false);
        paint::rect(
            list,
            Rect::new(area.min[0] + 66.0, y + 8.0, 14.0, 14.0),
            palette::TRACKS[index % palette::TRACKS.len()],
        );
        label(
            list,
            atlas,
            Rect::new(area.min[0] + 50.0, y, 14.0, metrics::ROW),
            &(index + 1).to_string(),
            true,
        );
        let layer = state["project"]["layers"]
            .as_array()
            .and_then(|ls| ls.iter().find(|l| l["id"].as_u64() == Some(row.id)));
        let parent = layer
            .and_then(|l| l["parent"]["object"].as_u64())
            .and_then(|id| {
                state["project"]["layers"]
                    .as_array()?
                    .iter()
                    .find(|l| l["id"].as_u64() == Some(id))
            })
            .and_then(|l| l["name"].as_str())
            .unwrap_or(locale.t(T::None));
        label(
            list,
            atlas,
            Rect::new(area.min[0] + gutter - 110.0, y, 102.0, metrics::ROW),
            parent,
            true,
        );
        let eye = Rect::new(area.min[0] + 10.0, y, 16.0, metrics::ROW);
        let lock = Rect::new(area.min[0] + 28.0, y, 16.0, metrics::ROW);
        ui::icon(
            list,
            eye,
            if row.visible { Icon::Eye } else { Icon::EyeOff },
        );
        ui::icon(
            list,
            lock,
            if row.locked { Icon::Lock } else { Icon::Unlock },
        );
        if clicked(input, eye) {
            out.push(Action::Edit(Command::Flags {
                object: row.id,
                visible: !row.visible,
                locked: row.locked,
            }));
        }
        if clicked(input, lock) {
            out.push(Action::Edit(Command::Flags {
                object: row.id,
                visible: row.visible,
                locked: !row.locked,
            }));
        }
        if clicked(input, name) {
            panels.selected = Some(row.id);
            panels.field = None;
        }
        let left = x_of(row.clip[0] as f64).max(track.min[0]);
        let right = x_of(row.clip[1] as f64).min(track.max[0]);
        let bar = Rect::new(left, y + 5.0, (right - left).max(0.0), metrics::ROW - 10.0);
        paint::rect(
            list,
            bar,
            palette::TRACKS[index % palette::TRACKS.len()].with_alpha(if row.visible {
                0.7
            } else {
                0.2
            }),
        );
        if selected {
            paint::stroke(list, bar, palette::ACCENT, 1.0);
        }
        if clicked(input, bar) {
            panels.selected = Some(row.id);
            if !row.locked {
                panels.moving = Some((
                    row.id,
                    input.mouse[0],
                    row.clip[0],
                    row.clip[1] - row.clip[0],
                    row.clip[0],
                ));
                out.push(Action::History(2));
            }
        }
        for f in &row.keys {
            let x = x_of(*f);
            if x >= track.min[0] + 4.0 && x < track.max[0] - 4.0 {
                paint::diamond(list, [x, y + metrics::ROW * 0.5], 4.0, palette::KEYFRAME);
            }
        }
    }
    if let Some((object, start, in_frame, duration, last)) = panels.moving.as_mut() {
        let next = (*in_frame as f32 + (input.mouse[0] - *start) / ppf)
            .round()
            .clamp(0.0, (total as u32 - *duration) as f32) as u32;
        if next != *last && (input.dragging() || input.released.is_some()) {
            out.push(Action::Edit(Command::MoveLayerClip {
                object: *object,
                in_frame: next,
            }));
            *last = next;
        }
        if input.released.is_some() {
            out.push(Action::History(3));
            panels.moving = None;
        }
    }
    let x = x_of(state["frame"].as_f64().unwrap_or(0.0));
    if x >= track.min[0] && x < track.max[0] {
        paint::rect(
            list,
            Rect::new(x, ruler.min[1], 1.0, area.height()),
            palette::ACCENT,
        );
        paint::diamond(list, [x, ruler.min[1] + 6.0], 6.0, palette::ACCENT);
    }
}

#[derive(Clone, Copy)]
enum Tool {
    Open,
    Save,
    Undo,
    Redo,
    Solid,
    Play,
    Pause,
    First,
    Last,
}
fn icon_button(
    list: &mut PaintList,
    r: Rect,
    tool: Tool,
    input: &Input,
    enabled: bool,
    action: Action,
    out: &mut Vec<Action>,
) {
    let hovered = enabled && r.contains(input.mouse);
    if hovered {
        paint::rect(list, r, palette::CHROME);
        paint::stroke(list, r, palette::BORDER, 1.0);
    }
    let c = if enabled {
        palette::TEXT
    } else {
        palette::TEXT_MUTED.with_alpha(0.45)
    };
    let x = r.min[0] + 8.0;
    let y = r.min[1] + 6.0;
    let line = |list: &mut PaintList, points: &[[f32; 2]]| {
        paint::line(
            list,
            points.iter().map(|p| [x + p[0], y + p[1]]).collect(),
            c,
        )
    };
    match tool {
        Tool::Open => {
            line(
                list,
                &[
                    [0., 16.],
                    [0., 3.],
                    [6., 3.],
                    [8., 6.],
                    [18., 6.],
                    [18., 16.],
                    [0., 16.],
                ],
            );
        }
        Tool::Save => {
            paint::stroke(list, Rect::new(x, y, 17., 17.), c, 1.);
            paint::stroke(list, Rect::new(x + 4., y + 1., 9., 5.), c, 1.);
            paint::stroke(list, Rect::new(x + 4., y + 10., 9., 6.), c, 1.);
        }
        Tool::Undo => {
            line(list, &[[4., 4.], [0., 8.], [4., 12.]]);
            line(list, &[[0., 8.], [12., 8.], [16., 12.], [16., 16.]]);
        }
        Tool::Redo => {
            line(list, &[[12., 4.], [16., 8.], [12., 12.]]);
            line(list, &[[16., 8.], [4., 8.], [0., 12.], [0., 16.]]);
        }
        Tool::Solid => paint::rect(list, Rect::new(x + 1., y + 1., 16., 16.), c),
        Tool::Play => {
            list.shapes.push(paint::Shape::Quad {
                points: [
                    [x + 3., y],
                    [x + 16., y + 8.],
                    [x + 3., y + 16.],
                    [x + 3., y + 16.],
                ],
            });
            list.colors.push(c);
        }
        Tool::Pause => {
            paint::rect(list, Rect::new(x + 2., y, 5., 16.), c);
            paint::rect(list, Rect::new(x + 11., y, 5., 16.), c);
        }
        Tool::First => {
            line(list, &[[5., 0.], [5., 16.]]);
            line(list, &[[16., 0.], [7., 8.], [16., 16.]]);
        }
        Tool::Last => {
            line(list, &[[13., 0.], [13., 16.]]);
            line(list, &[[2., 0.], [11., 8.], [2., 16.]]);
        }
    }
    if clicked(input, r) && enabled {
        out.push(action);
    }
}
fn menus(
    list: &mut PaintList,
    atlas: &mut TextAtlas,
    size: [f32; 2],
    panels: &mut Panels,
    input: &Input,
    state: &Value,
    out: &mut Vec<Action>,
) {
    let locale = panels.locale;
    paint::rect(
        list,
        Rect::new(0.0, 0.0, size[0], metrics::MENU_BAR),
        palette::CHROME,
    );
    let mut x = 8.0;
    let mut starts = vec![];
    for (i, key) in [
        T::File,
        T::Edit,
        T::Composition,
        T::Layer,
        T::Effect,
        T::Animation,
        T::View,
        T::Window,
        T::Help,
    ]
    .into_iter()
    .enumerate()
    {
        let name = locale.t(key);
        let width = atlas.measure(name) + 22.0;
        let r = Rect::new(x, 0.0, width, metrics::MENU_BAR);
        starts.push(x);
        if r.contains(input.mouse) || panels.menu == Some(i) {
            paint::rect(list, r, palette::SUNKEN);
        }
        label(
            list,
            atlas,
            Rect::new(x + 10.0, 0.0, width - 20.0, metrics::MENU_BAR),
            name,
            false,
        );
        if clicked(input, r) {
            panels.menu = if panels.menu == Some(i) {
                None
            } else {
                Some(i)
            };
        }
        x += width;
    }
    label(
        list,
        atlas,
        Rect::new(
            (size[0] - 160.0).max(x + 12.0),
            0.0,
            150.0,
            metrics::MENU_BAR,
        ),
        "Motion Studio",
        true,
    );
    if let Some(menu) = panels.menu {
        let selected = panels.selected.filter(|id| *id != 0);
        let frame = state["frame"].as_f64().unwrap_or(0.0).floor() as u32;
        let entries: Vec<(String, Action, bool)> = match menu {
            0 => vec![
                (
                    format!("{}    Ctrl+N", locale.t(T::NewProject)),
                    Action::New,
                    true,
                ),
                (
                    format!("{}    Ctrl+O", locale.t(T::OpenProject)),
                    Action::Open,
                    true,
                ),
                (
                    format!("{}    Ctrl+S", locale.t(T::Save)),
                    Action::Save,
                    true,
                ),
                (locale.t(T::ExportPackage).into(), Action::Pack, true),
            ],
            1 => vec![
                (
                    format!("{}    Ctrl+Z", locale.t(T::Undo)),
                    Action::History(0),
                    state["canUndo"].as_bool().unwrap_or(false),
                ),
                (
                    format!("{}    Ctrl+Shift+Z", locale.t(T::Redo)),
                    Action::History(1),
                    state["canRedo"].as_bool().unwrap_or(false),
                ),
            ],
            2 => vec![
                (locale.t(T::Play).into(), Action::TogglePlay, true),
                (locale.t(T::FirstFrame).into(), Action::Seek(0.0), true),
                (
                    locale.t(T::LastFrame).into(),
                    Action::Seek(last_frame(state)),
                    true,
                ),
            ],
            3 => vec![
                (locale.t(T::NewSolid).into(), Action::AddSolid, true),
                (
                    format!("{}    Ctrl+D", locale.t(T::Duplicate)),
                    Action::Edit(Command::Duplicate {
                        object: selected.unwrap_or(0),
                    }),
                    selected.is_some(),
                ),
                (
                    locale.t(T::Delete).into(),
                    Action::Edit(Command::Delete {
                        object: selected.unwrap_or(0),
                    }),
                    selected.is_some(),
                ),
            ],
            4 => vec![(
                locale.t(T::ShowEffects).into(),
                Action::ShowPanel(dock::DockId(2)),
                true,
            )],
            5 => vec![(
                locale.t(T::AddPositionKey).into(),
                Action::Edit(Command::Animate {
                    object: selected.unwrap_or(0),
                    property: Property::Position,
                    axis: None,
                    frame,
                    enabled: true,
                }),
                selected.is_some(),
            )],
            6 => vec![(
                locale.t(T::ShowComposition).into(),
                Action::ShowPanel(dock::DockId(3)),
                true,
            )],
            7 => vec![
                (
                    locale.t(T::Project).into(),
                    Action::ShowPanel(dock::DockId(1)),
                    true,
                ),
                (
                    locale.t(T::EffectControls).into(),
                    Action::ShowPanel(dock::DockId(2)),
                    true,
                ),
                (
                    locale.t(T::Composition).into(),
                    Action::ShowPanel(dock::DockId(3)),
                    true,
                ),
                (
                    locale.t(T::Timeline).into(),
                    Action::ShowPanel(dock::DockId(4)),
                    true,
                ),
                (
                    locale.t(T::ResetWorkspace).into(),
                    Action::ResetWorkspace,
                    true,
                ),
                (
                    format!("{} › {}", locale.t(T::Language), locale.t(T::Chinese)),
                    Action::Language(Locale::Zh),
                    true,
                ),
                (
                    locale.t(T::English).into(),
                    Action::Language(Locale::En),
                    true,
                ),
            ],
            _ => vec![(locale.t(T::About).into(), Action::About, true)],
        };
        let popup = Rect::new(
            starts
                .get(menu)
                .copied()
                .unwrap_or(8.0)
                .min((size[0] - 280.0).max(0.0)),
            metrics::MENU_BAR,
            280.0,
            entries.len() as f32 * 32.0 + 8.0,
        );
        // Text is drawn in a second GPU pass: remove covered labels before drawing the popup.
        list.glyphs.retain(|g| {
            g.position[0] + g.size[0] <= popup.min[0]
                || g.position[0] >= popup.max[0]
                || g.position[1] + g.size[1] <= popup.min[1]
                || g.position[1] >= popup.max[1]
        });
        paint::rect(list, popup, palette::CHROME);
        paint::stroke(list, popup, palette::BORDER, 1.0);
        for (i, (text, action, enabled)) in entries.into_iter().enumerate() {
            let r = Rect::new(
                popup.min[0] + 4.0,
                popup.min[1] + 4.0 + i as f32 * 32.0,
                popup.width() - 8.0,
                30.0,
            );
            if r.contains(input.mouse) {
                paint::rect(list, r, palette::SUNKEN);
            }
            label(
                list,
                atlas,
                Rect::new(r.min[0] + 8.0, r.min[1], r.width() - 16.0, r.height()),
                &text,
                !enabled,
            );
            if clicked(input, r) && enabled {
                out.push(action);
                panels.menu = None;
            }
        }
        if input
            .pressed
            .is_some_and(|(_, p)| !popup.contains(p) && p[1] >= metrics::MENU_BAR)
        {
            panels.menu = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translated_numeric_edit_preserves_the_same_command_and_axis() {
        let state = serde_json::json!({"frame":21,"project":{"layers":[{"id":1,"name":"自定义名称","three_d":false}]},
            "sampledLayers":[{"id":1,"position":[10,20,30],"rotation":[0,0,0],"scale":[100,100,100],"opacity":1}],
            "timeline_layers":[{"object":1,"properties":{"position":{"separated":true}}}]});
        let mut commands = vec![];
        for locale in [Locale::En, Locale::Zh] {
            let mut panels = Panels {
                locale,
                selected: Some(1),
                field: Some(Field {
                    object: 1,
                    property: Property::Position,
                    axis: 0,
                    text: "123".into(),
                    replace: false,
                }),
                ..Default::default()
            };
            let input = Input {
                keys: vec![(Key::Enter, Default::default())],
                ..Default::default()
            };
            let mut atlas = TextAtlas::from_system(18.0).unwrap();
            let mut paint = PaintList::default();
            let mut out = vec![];
            properties(
                &mut paint,
                &mut atlas,
                Rect::new(0.0, 0.0, 340.0, 600.0),
                &mut panels,
                &state,
                &input,
                &mut out,
            );
            let command = out
                .into_iter()
                .find_map(|a| match a {
                    Action::Edit(c) => Some(c),
                    _ => None,
                })
                .unwrap();
            commands.push(serde_json::to_value(command).unwrap());
        }
        assert_eq!(commands[0], commands[1]);
        assert_eq!(commands[0]["op"], "set_component");
        assert_eq!(commands[0]["axis"], "x");
        assert_eq!(commands[0]["frame"], 21);
        assert_eq!(commands[0]["value"], 123.0);
    }
    #[test]
    fn timeline_uses_real_snapshot_ids_and_independent_axis_key_times() {
        let state = serde_json::json!({"project":{"frames":120,"layers":[{"id":7,"name":"Bottom"},{"id":9,"name":"Top"}]},
            "timeline_layers":[{"object":9,"in_frame":15,"out_frame":90,"properties":{"position":{"keys":[],"axes":{"x":{"keys":[{"frame":0},{"frame":60}]},"y":{"keys":[{"frame":15},{"frame":90}]}}}}}]});
        let rows = rows(&state);
        assert_eq!(rows[0].id, 9);
        assert_eq!(rows[0].clip, [15, 90]);
        assert_eq!(rows[0].keys, vec![0.0, 15.0, 60.0, 90.0]);
        assert_eq!(rows[1].id, 7);
    }
    #[test]
    fn timecodes_step_frames_without_rounding_fractional_time() {
        assert_eq!(timecode(65.99, 30), "00:00:02:05");
        assert_eq!(timecode(108000.0, 30), "01:00:00:00");
    }
    #[test]
    fn composition_view_is_inside_window_at_small_sizes() {
        for size in [[900.0, 600.0], [1600.0, 900.0]] {
            let r = composition_view(&dock::editor_layout(), size);
            assert!(r.width() > 0.0 && r.height() > 0.0);
            assert!(Rect::new(0.0, 0.0, size[0], size[1]).contains_rect(r));
        }
    }
}
