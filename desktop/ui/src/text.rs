//! Glyph atlas and text shaping for the shell chrome.
//!
//! Panels render a bounded set of short strings, so a single-page atlas with
//! run-length glyph reuse is enough and avoids a full shaping engine. Font
//! selection prefers the platform UI face and falls back to any installed
//! font, so the shell renders on a machine with no fonts of its own.

use ab_glyph::{Font, FontVec, Glyph, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};

/// One rendered glyph in the atlas.
#[derive(Clone, Copy)]
pub struct GlyphEntry {
    /// Atlas rectangle, in texels.
    pub uv: [f32; 4],
    /// Offset from the pen position, in pixels.
    pub offset: [f32; 2],
    pub advance: f32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
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

/// CPU-side glyph cache plus the RGBA bitmap uploaded to the GPU.
pub struct TextAtlas {
    font: FontVec,
    scale: f32,
    config: AtlasConfig,
    /// Premultiplied white coverage in the atlas; the shader tints it.
    pixels: Vec<u8>,
    entries: std::collections::HashMap<char, GlyphEntry>,
    /// Pen position of the next glyph, in pixels.
    cursor: [f32; 2],
    /// Row height, so callers can align baselines.
    pub line_height: f32,
    pub ascent: f32,
    dirty: bool,
    /// Fonts seen so far, for the diagnostics panel.
    pub families: Vec<String>,
}

impl TextAtlas {
    /// Build an atlas from the system UI font at `scale` logical pixels.
    pub fn from_system(scale: f32) -> Result<Self, String> {
        let (font, families) = load_system_font()?;
        Ok(Self::new(font, scale, families))
    }

    pub fn new(font: FontVec, scale: f32, families: Vec<String>) -> Self {
        let scaled = ScaleFont::new(font.clone());
        let px = PxScale::from(scale);
        let line_height = scaled.ascent(px) - scaled.descent(px);
        Self {
            scale,
            config: AtlasConfig::default(),
            pixels: vec![0; (AtlasConfig::default().width * AtlasConfig::default().height * 4) as usize],
            entries: std::collections::HashMap::new(),
            cursor: [0.0, 0.0],
            line_height: line_height.max(scale),
            ascent: scaled.ascent(px),
            font: scaled.into_font(),
            dirty: true,
            families,
        }
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

    pub fn glyph(&mut self, character: char) -> GlyphEntry {
        if let Some(entry) = self.entries.get(&character) {
            return *entry;
        }
        let scaled = ScaleFont::new(self.font.clone());
        let px = PxScale::from(self.scale);
        let glyph_id = scaled.glyph_id(character);
        let glyph: Glyph = glyph_id.into();
        let advance = scaled.h_advance(glyph_id);
        let (px_min, px_max, offset) = scaled.pixel_bounding_box(glyph);
        let width = px_max.x.round().max(0.0) - px_min.x.round().max(0.0);
        let height = px_max.y.round().max(0.0) - px_min.y.round().max(0.0);
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            // Whitespace and unmapped code points advance without a bitmap.
            let entry = GlyphEntry {
                uv: [0.0; 4],
                offset: [0.0, self.ascent - px_min.y],
                advance,
            };
            self.entries.insert(character, entry);
            return entry;
        }
        if self.cursor[0] + width + self.config.padding as f32 > self.config.width as f32 {
            self.cursor[0] = 0.0;
            self.cursor[1] += self.line_height + self.config.padding as f32;
        }
        if self.cursor[1] + height > self.config.height as f32 {
            // Atlas full: drop the cache so the next frame repacks. Panel text is
            // short enough that this only happens with an unusual font choice.
            self.entries.clear();
            self.pixels.fill(0);
            self.cursor = [0.0, 0.0];
        }
        let x = self.cursor[0] as u32;
        let y = self.cursor[1] as u32;
        let mut bitmap = ab_glyph::Mask::new(
            width as usize,
            height as usize,
            ab_glyph::Mask::overlapping_bounds(
                px_min.x.round() as i32,
                px_min.y.round() as i32,
                width as i32,
                height as i32,
            ),
        );
        scaled.outline_glyph(glyph, &mut bitmap);
        let atlas_stride = self.config.width as usize;
        for row in 0..bitmap.dimensions[1] {
            for column in 0..bitmap.dimensions[0] {
                let value = bitmap.data[row * bitmap.dimensions[0] + column];
                let px_index = ((y as usize + row) * atlas_stride + x as usize + column) * 4;
                self.pixels[px_index..px_index + 4].fill(value);
            }
        }
        self.cursor[0] += width + self.config.padding as f32;
        self.cursor[1] = self.cursor[1];
        let entry = GlyphEntry {
            uv: [
                x as f32 / self.config.width as f32,
                y as f32 / self.config.height as f32,
                (x + width as u32) as f32 / self.config.width as f32,
                (y + height as u32) as f32 / self.config.height as f32,
            ],
            offset: [px_min.x.round(), self.ascent - px_max.y.round()],
            advance,
        };
        self.entries.insert(character, entry);
        self.dirty = true;
        entry
    }

    /// Total advance width of a string in logical pixels.
    pub fn measure(&mut self, text: &str) -> f32 {
        text.chars().map(|c| self.glyph(c).advance).sum()
    }

    /// Truncate to fit `width`, appending an ellipsis when characters are lost.
    pub fn ellipsize(&mut self, text: &str, width: f32) -> String {
        if self.measure(text) <= width {
            return text.to_owned();
        }
        let ellipsis = '\u{2026}';
        let ellipsis_width = self.glyph(ellipsis).advance;
        let mut result = String::new();
        let mut used = 0.0;
        for character in text.chars() {
            let advance = self.glyph(character).advance;
            if used + advance + ellipsis_width > width {
                break;
            }
            result.push(character);
            used += advance;
        }
        result.push(ellipsis);
        result
    }
}

/// Prefer a platform UI face, then fall back to any installed font.
fn load_system_font() -> Result<(FontVec, Vec<String>), String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
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
    let mut families = Vec::new();
    for face in database.faces() {
        let family = face.families.first().map(|f| f.as_str()).unwrap_or_default();
        if !families.iter().any(|f| f == family) {
            families.push(family.to_owned());
        }
    }
    families.sort();
    for wanted in preferred {
        if let Some(id) = database.query(&fontdb::Query {
            families: &fontdb::Family::Name(wanted),
            ..fontdb::Query::default()
        }) {
            if let Some(font) = database.with_face_data(id, |data, _| FontVec::try_from_vec(data.to_vec()))
            {
                return Ok((font, families));
            }
        }
    }
    let id = database
        .faces()
        .find(|face| face.monospaced)
        .map(|face| face.id)
        .or_else(|| database.faces().next().map(|face| face.id))
        .ok_or("no usable system font was found")?;
    let font = database
        .with_face_data(id, |data, _| FontVec::try_from_vec(data.to_vec()))
        .ok_or("system font could not be parsed")?;
    Ok((font, families))
}