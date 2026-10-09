//! Glyph atlas and text shaping for the shell chrome.
//!
//! Panels render a bounded set of short strings, so a single-page atlas is
//! enough and a full shaping engine is not needed. Font selection prefers the
//! platform UI face and falls back to any installed font, so the shell renders
//! on a machine with no fonts of its own.

use ab_glyph::{Font, FontArc, PxScale, PxScaleFont, ScaleFont};

/// One rendered glyph in the atlas.
#[derive(Clone, Copy)]
pub struct GlyphEntry {
    /// Atlas rectangle in normalised coordinates.
    pub uv: [f32; 4],
    /// Offset from the pen position to the glyph's top-left, in pixels.
    pub offset: [f32; 2],
    pub advance: f32,
}

/// Atlas geometry.
#[derive(Clone, Copy)]
pub struct AtlasConfig {
    pub width: u32,
    pub height: u32,
    pub padding: u32,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            padding: 1,
        }
    }
}

/// CPU-side glyph cache plus the coverage bitmap uploaded to the GPU.
///
/// The bitmap holds white premultiplied coverage in every channel; the shader
/// multiplies it by the per-glyph colour, so a glyph only ever costs one texel
/// read regardless of how many times it is drawn.
pub struct TextAtlas {
    font: FontArc,
    scale: f32,
    config: AtlasConfig,
    pixels: Vec<u8>,
    entries: std::collections::HashMap<char, GlyphEntry>,
    /// Pen position of the next glyph in the atlas, in texels.
    cursor: [f32; 2],
    pub line_height: f32,
    pub ascent: f32,
    dirty: bool,
    /// Families discovered on this machine, for the diagnostics panel.
    pub families: Vec<String>,
}

impl TextAtlas {
    /// Build an atlas from the system UI font at `scale` logical pixels.
    pub fn from_system(scale: f32) -> Result<Self, String> {
        let (font, families) = load_system_font()?;
        Ok(Self::new(font, scale, families))
    }

    pub fn new(font: FontArc, scale: f32, families: Vec<String>) -> Self {
        let scaled = PxScaleFont::from(font.clone()).with_scale(PxScale::from(scale));
        let line_height = scaled.height().max(scale);
        let ascent = scaled.ascent();
        let config = AtlasConfig::default();
        let pixels = vec![0u8; (config.width * config.height * 4) as usize];
        Self {
            font,
            scale,
            config,
            pixels,
            entries: std::collections::HashMap::new(),
            cursor: [0.0, 0.0],
            line_height,
            ascent,
            dirty: true,
            families,
        }
    }

    /// Scale the rasterisation size, for example after a display scale change.
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        self.clear();
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.pixels.fill(0);
        self.cursor = [0.0, 0.0];
        self.dirty = true;
    }

    pub fn pixel_data(&self) -> &[u8] {
        &self.pixels
    }

    pub fn config(&self) -> AtlasConfig {
        self.config
    }

    /// True when new glyphs were rasterised since the last upload.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Advance width of a character, without rasterising it.
    fn advance(&self, character: char) -> f32 {
        let scaled = PxScaleFont::from(self.font.clone()).with_scale(PxScale::from(self.scale));
        let id = scaled.glyph_id(character);
        if id.0 == 0 {
            // Unmapped code points take a fixed width so layout stays stable.
            return self.scale * 0.5;
        }
        scaled.h_advance(id)
    }

    /// Rasterise a character if it is not already cached.
    pub fn glyph(&mut self, character: char) -> GlyphEntry {
        if let Some(entry) = self.entries.get(&character) {
            return *entry;
        }
        let scaled = PxScaleFont::from(self.font.clone()).with_scale(PxScale::from(self.scale));
        let id = scaled.glyph_id(character);
        let advance = if id.0 == 0 {
            self.scale * 0.5
        } else {
            scaled.h_advance(id)
        };
        if id.0 == 0 {
            let entry = GlyphEntry {
                uv: [0.0; 4],
                offset: [0.0, 0.0],
                advance,
            };
            self.entries.insert(character, entry);
            return entry;
        }
        let glyph = scaled.scaled_glyph(character);
        let bounds = scaled.glyph_bounds(&glyph);
        let width = bounds.max.x - bounds.min.x;
        let height = bounds.max.y - bounds.min.y;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            // Whitespace advances without a bitmap.
            let entry = GlyphEntry {
                uv: [0.0; 4],
                offset: [0.0, 0.0],
                advance,
            };
            self.entries.insert(character, entry);
            return entry;
        }
        let width = width.ceil() as u32;
        let height = height.ceil() as u32;
        let padding = self.config.padding;
        let row_height = self.line_height.ceil() as u32;
        if self.cursor[0] + width as f32 + padding as f32 > self.config.width as f32 {
            self.cursor[0] = 0.0;
            self.cursor[1] += row_height as f32;
        }
        if self.cursor[1] + row_height as f32 > self.config.height as f32 {
            // Atlas full. Panel text is short, so this only happens with an
            // unusual font or a very large UI scale; repacking is cheaper than
            // growing the atlas at runtime.
            self.clear();
        }
        let x = self.cursor[0] as u32;
        let y = self.cursor[1] as u32;
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let stride = self.config.width as usize;
            outlined.draw(|px, py, coverage| {
                // The outline is positioned in absolute pixel space; shift it
                // into the atlas rectangle this glyph occupies.
                let column = (px - bounds.min.x) as i64;
                let row = (py - bounds.min.y) as i64;
                if column < 0 || row < 0 {
                    return;
                }
                let (column, row) = (column as usize, row as usize);
                if column >= width as usize || row >= height as usize {
                    return;
                }
                let index = ((y as usize + row) * stride + x as usize + column) * 4;
                self.pixels[index..index + 4].fill(coverage);
            });
        }
        self.cursor[0] += width as f32 + padding as f32;
        let entry = GlyphEntry {
            uv: [
                x as f32 / self.config.width as f32,
                y as f32 / self.config.height as f32,
                (x + width) as f32 / self.config.width as f32,
                (y + height) as f32 / self.config.height as f32,
            ],
            // Atlas rows advance downward; glyph space advances upward.
            offset: [
                bounds.min.x,
                (self.ascent - bounds.min.y).round(),
            ],
            advance,
        };
        self.entries.insert(character, entry);
        self.dirty = true;
        entry
    }

    /// Total advance width of a string in logical pixels.
    pub fn measure(&mut self, text: &str) -> f32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// Truncate to fit `width`, appending an ellipsis when characters are lost.
    pub fn ellipsize(&mut self, text: &str, width: f32) -> String {
        if self.measure(text) <= width {
            return text.to_owned();
        }
        let ellipsis_width = self.advance('\u{2026}');
        let mut result = String::new();
        let mut used = 0.0;
        for character in text.chars() {
            let advance = self.advance(character);
            if used + advance + ellipsis_width > width {
                break;
            }
            result.push(character);
            used += advance;
        }
        result.push('\u{2026}');
        result
    }
}

/// Prefer a platform UI face, then fall back to any installed font.
fn load_system_font() -> Result<(FontArc, Vec<String>), String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let families: Vec<String> = database
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !name.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let preferred = [
        "Segoe UI",
        "SF Pro Text",
        "Helvetica Neue",
        "Inter",
        "Roboto",
        "Noto Sans",
        "DejaVu Sans",
        "Arial",
    ];
    let mut candidates: Vec<fontdb::ID> = Vec::new();
    for wanted in preferred {
        if let Some(id) = database.query(&fontdb::Query {
            families: &[fontdb::Family::Name(wanted)],
            ..fontdb::Query::default()
        }) {
            candidates.push(id);
        }
    }
    // A monospaced face keeps numeric columns aligned if it is all we have.
    candidates.extend(
        database
            .faces()
            .filter(|face| face.monospaced)
            .map(|face| face.id),
    );
    candidates.extend(database.faces().map(|face| face.id));
    for id in candidates {
        let font = database.with_face_data(id, |data, _| FontArc::try_from_vec(data.to_vec()));
        if let Some(Ok(font)) = font {
            return Ok((font, families));
        }
    }
    Err("no usable system font was found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The atlas needs a real font, so these tests use whichever system face the
    /// host provides and skip when none is installed.
    fn fixture() -> Option<TextAtlas> {
        TextAtlas::from_system(13.0).ok()
    }

    #[test]
    fn every_glyph_lands_inside_the_atlas() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        let (width, height) = (atlas.config().width, atlas.config().height);
        for character in "Motion Studio 0123456789 /.".chars() {
            let entry = atlas.glyph(character);
            assert!(entry.uv[0] >= 0.0 && entry.uv[2] <= 1.0, "{character}");
            assert!(entry.uv[1] >= 0.0 && entry.uv[3] <= 1.0, "{character}");
            assert!(
                (entry.uv[2] - entry.uv[0]) * width as f32 <= width as f32,
                "{character} overflows"
            );
            assert!(
                (entry.uv[3] - entry.uv[1]) * height as f32 <= height as f32,
                "{character} overflows"
            );
        }
    }

    #[test]
    fn whitespace_advances_without_consuming_atlas_texels() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        let space = atlas.glyph(' ');
        assert_eq!(space.uv, [0.0; 4]);
        assert!(space.advance > 0.0);
    }

    #[test]
    fn measuring_does_not_rasterise() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        atlas.take_dirty();
        let width = atlas.measure("Motion Studio");
        assert!(width > 0.0);
        assert!(!atlas.take_dirty(), "measure must not dirty the atlas");
    }

    #[test]
    fn ellipsize_fits_within_the_requested_width() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        let limit = 40.0;
        let shown = atlas.ellipsize("Effect Controls", limit);
        assert!(shown.ends_with('\u{2026}'));
        assert!(atlas.measure(&shown) <= limit, "{shown:?}");
        assert_eq!(atlas.ellipsize("ab", 10_000.0), "ab");
    }

    #[test]
    fn the_atlas_reports_dirty_only_after_new_glyphs() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        assert!(atlas.take_dirty());
        atlas.glyph('A');
        assert!(atlas.take_dirty());
        atlas.glyph('A');
        assert!(!atlas.take_dirty());
    }

    #[test]
    fn changing_scale_repacks_the_atlas() {
        let Some(mut atlas) = fixture() else {
            return;
        };
        atlas.glyph('A');
        atlas.take_dirty();
        atlas.set_scale(26.0);
        assert!(atlas.take_dirty());
        assert_eq!(atlas.entries.len(), 0);
        assert!(atlas.line_height >= 26.0);
    }
}