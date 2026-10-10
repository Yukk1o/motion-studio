//! Immediate-mode widgets and drawing helpers for panel content.
//!
//! Widgets write into a [`PaintList`] and read interaction from an [`Input`].
//! Keeping the API in terms of logical pixels means the same panel code serves
//! a 100% and a 200% display without branching.

use crate::input::{Event, Input, Key, MouseButton, Rect};
use crate::paint::{self, PaintList};
use crate::theme::{metrics, palette, Color};

/// Interaction result for one widget.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Response {
    pub hovered: bool,
    pub clicked: bool,
    pub active: bool,
}

impl Response {
    fn none() -> Self {
        Self {
            hovered: false,
            clicked: false,
            active: false,
        }
    }
}

/// Draw and hit test a push button.
pub fn button(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    label: &str,
    input: &Input,
    enabled: bool,
) -> Response {
    let hovered = enabled && area.contains(input.mouse);
    let clicked = hovered
        && matches!(
            input.pressed,
            Some((MouseButton::Left, p)) if area.contains(p)
        );
    let background = if !enabled {
        palette::PANEL
    } else if clicked {
        palette::ACCENT.with_alpha(0.35)
    } else if hovered {
        palette::CHROME
    } else {
        palette::SUNKEN
    };
    paint::rect(list, area, background);
    if enabled {
        paint::stroke(list, area, palette::BORDER, 1.0);
    }
    let color = if enabled {
        palette::TEXT
    } else {
        palette::TEXT_MUTED
    };
    let width = atlas.measure(label);
    text(
        list,
        atlas,
        Rect::new(
            area.min[0] + 4.0,
            area.min[1],
            (area.width() - 8.0).max(0.0),
            area.height(),
        ),
        label,
        color,
        Align::Center,
    );
    let _ = width;
    Response {
        hovered,
        clicked,
        active: false,
    }
}

/// Horizontal alignment within a text rect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Draw a single line of text, ellipsized to fit `area`.
pub fn text(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    label: &str,
    color: Color,
    align: Align,
) {
    if area.width() <= 0.0 || label.is_empty() {
        return;
    }
    let shown = atlas.ellipsize(label, area.width());
    let width = atlas.measure(&shown);
    let origin = match align {
        Align::Left => area.min[0],
        Align::Center => area.min[0] + (area.width() - width).max(0.0) * 0.5,
        Align::Right => area.max[0] - width,
    };
    let baseline = area.min[1] + (area.height() + atlas.ascent) * 0.5;
    let mut pen = origin;
    for character in shown.chars() {
        let glyph = atlas.glyph(character);
        let width = (glyph.uv[2] - glyph.uv[0]) * atlas.config().width as f32;
        let height = (glyph.uv[3] - glyph.uv[1]) * atlas.config().height as f32;
        // Whitespace has no atlas rectangle and only advances the pen.
        if width > 0.0 && height > 0.0 {
            list.glyphs.push(paint::Glyph {
                uv: glyph.uv,
                position: [pen + glyph.offset[0], baseline + glyph.offset[1]],
                size: [width, height],
                color,
            });
        }
        pen += glyph.advance;
    }
}

/// A numeric field that scrubs horizontally and accepts a typed value.
///
/// This is the interaction After Effects uses for every property value, so it
/// is deliberately shared: drag changes the value smoothly, a click opens the
/// text field, and the modifier keys invert or constrain the drag.
#[derive(Clone, Copy, Default, Debug)]
pub struct ScrubField {
    pub editing: bool,
    /// Set while the pointer is dragging the value.
    pub dragging: bool,
    /// Value captured at drag start so late updates cannot move the base.
    pub drag_origin: f32,
}

impl ScrubField {
    pub fn begin(&mut self, value: f32) {
        self.dragging = true;
        self.drag_origin = value;
    }

    pub fn end(&mut self) {
        self.dragging = false;
        self.editing = false;
    }

    /// Value for a horizontal drag of `delta` logical pixels from where the
    /// drag started, clamped to the property's range.
    pub fn scrub(&self, delta: f32, range: (f32, f32), step: f32) -> f32 {
        (self.drag_origin + delta * step).clamp(range.0, range.1)
    }
}

/// Draw a scrub field and report whether the user grabbed it.
pub fn scrub_field(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    label: &str,
    value: f32,
    field: &mut ScrubField,
    input: &Input,
) -> Response {
    let hovered = area.contains(input.mouse);
    let grabbed = hovered
        && matches!(
            input.pressed,
            Some((MouseButton::Left, p)) if area.contains(p)
        );
    paint::rect(list, area, palette::SUNKEN);
    if hovered || field.dragging {
        paint::stroke(list, area, palette::ACCENT.with_alpha(0.6), 1.0);
    }
    let right = Rect::new(
        area.min[0] + area.width() * 0.45,
        area.min[1],
        area.width() * 0.55,
        area.height(),
    );
    text(list, atlas, area, label, palette::TEXT_MUTED, Align::Left);
    text(
        list,
        atlas,
        right,
        &format_value(value),
        palette::TEXT,
        Align::Right,
    );
    Response {
        hovered,
        clicked: grabbed,
        active: field.dragging,
    }
}

/// Format a value the way the engine reports it: no exponent, stable precision.
pub fn format_value(value: f32) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if !value.is_finite() {
        return "—".to_owned();
    }
    let magnitude = value.abs();
    let text = if magnitude >= 1000.0 {
        format!("{value:.1}")
    } else if magnitude >= 10.0 {
        format!("{value:.2}")
    } else if magnitude >= 1.0 {
        format!("{value:.3}")
    } else {
        format!("{value:.4}")
    };
    let trimmed = text.trim_end_matches('0');
    if magnitude >= 1000.0 && trimmed.ends_with('.') {
        format!("{trimmed}0")
    } else {
        trimmed.trim_end_matches('.').to_owned()
    }
}

/// Tab strip for a dock. Returns the tab index that was clicked, if any.
pub fn tab_strip(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    titles: &[&str],
    active: usize,
    input: &Input,
) -> Option<usize> {
    paint::rect(list, area, palette::CHROME);
    paint::rect(
        list,
        Rect::new(area.min[0], area.max[1] - 1.0, area.width(), 1.0),
        palette::BORDER,
    );
    if titles.is_empty() {
        return None;
    }
    let width = area.width() / titles.len() as f32;
    let mut clicked = None;
    for (index, title) in titles.iter().enumerate() {
        let tab = Rect::new(
            area.min[0] + width * index as f32,
            area.min[1],
            width,
            area.height() - 1.0,
        );
        let selected = index == active;
        paint::rect(
            list,
            tab,
            if selected {
                palette::PANEL
            } else {
                palette::CHROME
            },
        );
        if selected {
            paint::rect(
                list,
                Rect::new(tab.min[0], tab.min[1], tab.width(), 2.0),
                palette::ACCENT,
            );
        }
        let color = if selected {
            palette::TEXT
        } else {
            palette::TEXT_MUTED
        };
        text(
            list,
            atlas,
            Rect::new(
                tab.min[0] + 6.0,
                tab.min[1],
                tab.width() - 12.0,
                tab.height(),
            ),
            title,
            color,
            Align::Left,
        );
        if tab.contains(input.mouse)
            && matches!(input.pressed, Some((MouseButton::Left, p)) if tab.contains(p))
        {
            clicked = Some(index);
        }
    }
    clicked
}

/// A checkbox with a label, used across the property panels.
pub fn checkbox(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    label: &str,
    value: &mut bool,
    input: &Input,
) -> Response {
    let box_area = Rect::new(area.min[0] + 2.0, area.min[1] + 3.0, 12.0, 12.0);
    let hovered = area.contains(input.mouse);
    let clicked =
        hovered && matches!(input.pressed, Some((MouseButton::Left, p)) if area.contains(p));
    if clicked {
        *value = !*value;
    }
    paint::rect(list, box_area, palette::SUNKEN);
    paint::stroke(
        list,
        box_area,
        if *value {
            palette::ACCENT
        } else {
            palette::BORDER
        },
        1.0,
    );
    if *value {
        paint::rect(
            list,
            Rect::new(box_area.min[0] + 3.0, box_area.min[1] + 3.0, 6.0, 6.0),
            palette::ACCENT,
        );
    }
    text(
        list,
        atlas,
        Rect::new(
            box_area.max[0] + 6.0,
            area.min[1],
            area.width(),
            area.height(),
        ),
        label,
        palette::TEXT,
        Align::Left,
    );
    Response {
        hovered,
        clicked,
        active: false,
    }
}

/// One row of the layer list: eye, lock, name and colour chip.
pub struct LayerRow {
    pub id: u64,
    pub name: String,
    pub color: Color,
    pub visible: bool,
    pub locked: bool,
    pub selected: bool,
    pub camera: bool,
    pub active: bool,
}

/// Draw the layer list, returning the clicked row id and column.
#[derive(Default, Debug)]
pub struct LayerListHit {
    pub row: Option<u64>,
    pub column: LayerColumn,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LayerColumn {
    #[default]
    Name,
    Visibility,
    Lock,
}

/// Draw a vertical layer list with fixed row height.
pub fn layer_list(
    list: &mut PaintList,
    atlas: &mut crate::text::TextAtlas,
    area: Rect,
    rows: &[LayerRow],
    offset: f32,
    input: &Input,
) -> LayerListHit {
    let mut hit = LayerListHit::default();
    let row_height = metrics::ROW;
    let gutter = 44.0;
    let visible = (area.height() / row_height).ceil() as usize;
    let first = (offset / row_height).floor().max(0.0) as usize;
    paint::rect(list, area, palette::PANEL);
    for index in first..(first + visible).min(rows.len()) {
        let row = &rows[index];
        let top = area.min[1] + index as f32 * row_height - offset;
        let row_area = Rect::new(area.min[0], top, area.width(), row_height);
        if !area.contains_rect(row_area) {
            continue;
        }
        let background = if row.selected {
            palette::ACCENT.with_alpha(0.18)
        } else if row.camera {
            palette::TRACK_CAMERA.with_alpha(0.10)
        } else {
            palette::PANEL
        };
        paint::rect(list, row_area, background);
        if !row.active {
            // Inactive clips stay readable but clearly recede.
            paint::rect(list, row_area, palette::APP_BACKGROUND.with_alpha(0.55));
        }
        let color = if row.camera {
            palette::TRACK_CAMERA
        } else {
            row.color
        };
        paint::rect(
            list,
            Rect::new(row_area.min[0] + 2.0, top + 5.0, 3.0, row_height - 10.0),
            color,
        );
        let name_area = Rect::new(
            row_area.min[0] + gutter,
            top,
            row_area.width() - gutter - 8.0,
            row_height,
        );
        text(
            list,
            atlas,
            name_area,
            &row.name,
            if row.active {
                palette::TEXT
            } else {
                palette::TEXT_MUTED
            },
            Align::Left,
        );
        let eye = Rect::new(row_area.min[0] + 12.0, top, 14.0, row_height);
        let lock = Rect::new(row_area.min[0] + 28.0, top, 14.0, row_height);
        icon(
            list,
            eye,
            if row.visible { Icon::Eye } else { Icon::EyeOff },
        );
        icon(
            list,
            lock,
            if row.locked { Icon::Lock } else { Icon::Unlock },
        );
        if let Some((MouseButton::Left, point)) = input.pressed {
            if row_area.contains(point) {
                hit.row = Some(row.id);
                hit.column = if eye.contains(point) {
                    LayerColumn::Visibility
                } else if lock.contains(point) {
                    LayerColumn::Lock
                } else {
                    LayerColumn::Name
                };
            }
        }
    }
    hit
}

/// Minimal line icons drawn as shapes; the project ships no icon font.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Keyframe,
    Stopwatch,
    Camera,
    Chevron,
}

pub fn icon(list: &mut PaintList, area: Rect, icon: Icon) {
    let size = 12.0;
    let min = [
        area.min[0] + (area.width() - size) * 0.5,
        area.min[1] + (area.height() - size) * 0.5,
    ];
    let max = [min[0] + size, min[1] + size];
    let color = palette::TEXT_MUTED;
    match icon {
        Icon::Chevron => {
            paint::line(
                list,
                vec![
                    [min[0] + 3.0, min[1] + 2.0],
                    [min[0] + 7.0, min[1] + 6.0],
                    [min[0] + 11.0, min[1] + 2.0],
                ],
                color,
            );
        }
        Icon::Eye | Icon::EyeOff => {
            paint::line(
                list,
                vec![
                    [min[0], min[1] + 6.0],
                    [min[0] + 4.0, min[1] + 2.0],
                    [min[0] + 8.0, min[1] + 2.0],
                    [min[0] + 12.0, min[1] + 6.0],
                    [min[0] + 8.0, min[1] + 10.0],
                    [min[0] + 4.0, min[1] + 10.0],
                    [min[0], min[1] + 6.0],
                ],
                color,
            );
            paint::rect(list, Rect::new(min[0] + 5.0, min[1] + 5.0, 2.0, 2.0), color);
            if icon == Icon::EyeOff {
                paint::line(
                    list,
                    vec![[min[0], min[1] + 10.0], [min[0] + 12.0, min[1] + 1.0]],
                    color,
                );
            }
        }
        Icon::Lock | Icon::Unlock => {
            paint::rect(list, Rect::new(min[0] + 2.0, min[1] + 6.0, 8.0, 5.0), color);
            let shackle = if icon == Icon::Lock {
                vec![
                    [min[0] + 3.0, min[1] + 6.0],
                    [min[0] + 3.0, min[1] + 3.0],
                    [min[0] + 9.0, min[1] + 3.0],
                    [min[0] + 9.0, min[1] + 6.0],
                ]
            } else {
                vec![
                    [min[0] + 3.0, min[1] + 6.0],
                    [min[0] + 3.0, min[1] + 4.0],
                    [min[0] + 9.0, min[1] + 3.0],
                    [min[0] + 9.0, min[1] + 1.0],
                ]
            };
            paint::line(list, shackle, color);
        }
        Icon::Keyframe => {
            // A diamond, the same marker the timeline uses.
            let mid = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
            let radius = 4.0;
            paint::line(
                list,
                vec![
                    [mid[0], mid[1] - radius],
                    [mid[0] + radius, mid[1]],
                    [mid[0], mid[1] + radius],
                    [mid[0] - radius, mid[1]],
                    [mid[0], mid[1] - radius],
                ],
                palette::KEYFRAME,
            );
        }
        Icon::Stopwatch => {
            paint::rect(list, Rect::new(min[0] + 2.0, min[1] + 4.0, 8.0, 7.0), color);
            paint::line(
                list,
                vec![[min[0] + 6.0, min[1] + 2.0], [min[0] + 6.0, min[1] + 4.0]],
                color,
            );
        }
        Icon::Camera => {
            paint::rect(list, Rect::new(min[0], min[1] + 3.0, 9.0, 6.0), color);
            paint::line(
                list,
                vec![
                    [min[0] + 9.0, min[1] + 5.0],
                    [min[0] + 12.0, min[1] + 3.0],
                    [min[0] + 12.0, min[1] + 9.0],
                    [min[0] + 9.0, min[1] + 7.0],
                ],
                color,
            );
        }
    }
}

/// Translate a raw event into a frame's input, keeping the previous mouse
/// position so drags can be measured as deltas.
pub fn accumulate(input: &mut Input, event: Event) {
    match event {
        Event::MouseMoved { position } => input.mouse = position,
        Event::MousePressed { position, button } => {
            input.mouse = position;
            input.held = Some(button);
            input.pressed = Some((button, position));
        }
        Event::MouseReleased { position, button } => {
            input.mouse = position;
            input.held = None;
            input.released = Some((button, position));
        }
        Event::MouseWheel { delta } => input.wheel = delta,
        Event::KeyPressed { key, modifiers } => {
            input.modifiers = modifiers;
            input.keys.push((key, modifiers));
        }
        Event::TextInput(text) => input.text.push_str(&text),
        Event::LongPress { position } => input.long_press = Some(position),
        Event::FocusLost => {
            input.held = None;
            input.pressed = None;
            input.released = None;
            input.modifiers = Default::default();
        }
    }
}

/// Key chords the shell reacts to globally.
pub fn matches(input: &Input, key: Key, control: bool) -> bool {
    input
        .keys
        .iter()
        .any(|(k, m)| *k == key && m.control == control)
}

impl Default for Response {
    fn default() -> Self {
        Response::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Modifiers, Scroll};

    #[test]
    fn values_format_without_trailing_noise() {
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(1080.0), "1080.0");
        assert_eq!(format_value(0.5), "0.5");
        assert_eq!(format_value(-12.25), "-12.25");
        assert_eq!(format_value(f32::NAN), "—");
    }

    #[test]
    fn scrubbing_is_relative_to_the_drag_origin_not_the_current_value() {
        let mut field = ScrubField::default();
        field.begin(10.0);
        // A second update mid-drag must not compound.
        assert_eq!(field.scrub(100.0, (-1000.0, 1000.0), 0.5), 60.0);
        assert_eq!(field.scrub(100.0, (-1000.0, 1000.0), 0.5), 60.0);
    }

    #[test]
    fn scrubbing_respects_the_declared_range() {
        let mut field = ScrubField::default();
        field.begin(0.0);
        assert_eq!(field.scrub(10_000.0, (0.0, 360.0), 1.0), 360.0);
        assert_eq!(field.scrub(-10_000.0, (0.0, 360.0), 1.0), 0.0);
    }

    #[test]
    fn modifiers_are_recorded_for_keyboard_chords() {
        let mut input = Input::default();
        input.begin_frame();
        accumulate(
            &mut input,
            Event::KeyPressed {
                key: Key::Z,
                modifiers: Modifiers {
                    control: true,
                    ..Default::default()
                },
            },
        );
        assert!(matches(&input, Key::Z, true));
        assert!(!matches(&input, Key::Z, false));
        input.begin_frame();
        assert!(input.keys.is_empty());
    }

    #[test]
    fn a_scroll_region_clamps_to_its_content() {
        let mut scroll = Scroll::default();
        scroll.consume(-500.0, 100.0, 400.0);
        assert_eq!(scroll.offset, 300.0);
        scroll.consume(-500.0, 100.0, 400.0);
        assert_eq!(scroll.offset, 300.0, "must not scroll past the content");
        scroll.consume(5000.0, 100.0, 400.0);
        assert_eq!(scroll.offset, 0.0);
    }
}
