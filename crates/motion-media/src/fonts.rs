//! Shared, bounded font loading and coverage rasterization for ASCII and text.
//! No Android UI, installed system fonts, network, or GPU readbacks required.
use crate::Result;
use fontdue::{Font, FontSettings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
pub const GLYPH_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_FACES: usize = 2;
const MAX_GLYPH_PIXELS: usize = 1024 * 1024;
pub const BUILTIN_BYTES: &[u8] = include_bytes!("../fonts/IBMPlexMono-Regular.ttf");
const BUILTIN_LICENSE: &str = include_str!("../fonts/OFL.txt");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontInfo {
    pub id: String,
    pub name: String,
    pub glyphs: u16,
    pub source_bytes: usize,
    pub license: String,
    pub face_index: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct GlyphMetrics {
    pub character: char,
    pub width: usize,
    pub height: usize,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance: f32,
}
pub struct Glyph {
    pub metrics: GlyphMetrics,
    pub coverage: Vec<u8>,
}
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
pub struct AsciiAtlas {
    pub raster: Raster,
    /// Sorted by actual raster coverage, not the caller's character order.
    pub characters: String,
    pub cell: [u32; 2],
    pub coverage: Vec<f32>,
}
pub struct FontStore {
    root: PathBuf,
    faces: VecDeque<(String, Arc<Font>)>,
    glyphs: VecDeque<((String, char, u32), Arc<Glyph>)>,
    glyph_bytes: usize,
    pub cache_hits: u64,
}
impl FontStore {
    pub fn new(root: &Path) -> Result<Self> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let mut store = Self {
            root: root.canonicalize().map_err(|e| e.to_string())?,
            faces: VecDeque::new(),
            glyphs: VecDeque::new(),
            glyph_bytes: 0,
            cache_hits: 0,
        };
        store.import_bytes(BUILTIN_BYTES, BUILTIN_LICENSE)?;
        Ok(store)
    }
    pub fn builtin_id() -> String {
        format!("{:x}", Sha256::digest(BUILTIN_BYTES))
    }
    pub fn import(&mut self, source: &Path, license: &str) -> Result<FontInfo> {
        self.import_face(source, license, 0)
    }
    pub fn import_face(
        &mut self,
        source: &Path,
        license: &str,
        face_index: u32,
    ) -> Result<FontInfo> {
        let mut data = Vec::new();
        fs::File::open(source)
            .map_err(|e| e.to_string())?
            .take(MAX_FONT_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        self.import_face_bytes(&data, license, face_index)
    }
    pub fn import_bytes(&mut self, data: &[u8], license: &str) -> Result<FontInfo> {
        self.import_face_bytes(data, license, 0)
    }
    pub fn import_face_bytes(
        &mut self,
        data: &[u8],
        license: &str,
        face_index: u32,
    ) -> Result<FontInfo> {
        if data.is_empty() || data.len() > MAX_FONT_BYTES || license.len() > 65536 {
            return Err("font or license exceeds import limits".into());
        }
        if face_index > 255 {
            return Err("font collection face index exceeds limit".into());
        }
        let font = parse(data, face_index)?;
        let hash = format!("{:x}", Sha256::digest(data));
        let id = if face_index == 0 {
            hash
        } else {
            format!("{hash}-{face_index}")
        };
        let info = FontInfo {
            id: id.clone(),
            name: font.name().unwrap_or("Imported font").into(),
            glyphs: font.glyph_count(),
            source_bytes: data.len(),
            license: license.into(),
            face_index,
        };
        // Managed paths are content hashes, never names supplied by a font file.
        let path = self.path(&id, "ttf")?;
        if !path.exists() {
            fs::write(&path, data).map_err(|e| e.to_string())?;
        }
        fs::write(
            self.path(&id, "json")?,
            serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.retain_face(id, Arc::new(font));
        Ok(info)
    }
    fn path(&self, id: &str, extension: &str) -> Result<PathBuf> {
        let (hash, index) = identity(id)?;
        let key = if extension == "ttf" || index == 0 {
            hash.to_string()
        } else {
            format!("{hash}-{index}")
        };
        let path = self.root.join(format!("{key}.{extension}"));
        if path.exists()
            && !path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&self.root)
        {
            return Err("font path escapes store".into());
        }
        Ok(path)
    }
    fn retain_face(&mut self, id: String, font: Arc<Font>) {
        self.faces.retain(|(key, _)| *key != id);
        self.faces.push_back((id, font));
        while self.faces.len() > MAX_FACES {
            self.faces.pop_front();
        }
    }
    fn face(&mut self, id: &str) -> Result<Arc<Font>> {
        if let Some(index) = self.faces.iter().position(|(key, _)| key == id) {
            let pair = self.faces.remove(index).unwrap();
            let font = pair.1.clone();
            self.faces.push_back(pair);
            return Ok(font);
        }
        let path = self.path(id, "ttf")?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_FONT_BYTES as u64 {
            return Err("font exceeds size limit".into());
        }
        let (hash, index) = identity(id)?;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_FONT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_FONT_BYTES {
            return Err("font exceeds size limit".into());
        }
        if format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err("font content hash does not match identity".into());
        }
        let font = Arc::new(parse(&bytes, index)?);
        self.retain_face(id.into(), font.clone());
        Ok(font)
    }
    pub fn catalogue(&self) -> Result<Vec<FontInfo>> {
        let mut result = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|v| v.to_str())
                .ok_or("invalid font metadata")?;
            let path = self.path(id, "json")?;
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 128 * 1024 {
                return Err("font metadata exceeds size limit".into());
            }
            let info = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            result.push(info);
        }
        result.sort_by(|a: &FontInfo, b| a.id.cmp(&b.id));
        Ok(result)
    }
    pub fn glyph(&mut self, id: &str, ch: char, size: f32) -> Result<Arc<Glyph>> {
        if !size.is_finite() || !(4. ..=256.).contains(&size) {
            return Err("font size must be 4..256 pixels".into());
        }
        let key = (id.to_owned(), ch, size.to_bits());
        if let Some(index) = self.glyphs.iter().position(|(k, _)| *k == key) {
            let entry = self.glyphs.remove(index).unwrap();
            let glyph = entry.1.clone();
            self.glyphs.push_back(entry);
            self.cache_hits += 1;
            return Ok(glyph);
        }
        let font = self.face(id)?;
        if !font.has_glyph(ch) {
            return Err(format!("font has no glyph for U+{:04X}", ch as u32));
        }
        let m = font.metrics(ch, size);
        if m.width == 0 && m.height == 0 && !ch.is_whitespace() {
            return Err("glyph has no scalable outline; bitmap/color fonts are unsupported".into());
        }
        if m.width.saturating_mul(m.height) > MAX_GLYPH_PIXELS {
            return Err("glyph exceeds raster budget".into());
        }
        let (m, coverage) = font.rasterize(ch, size);
        let glyph = Arc::new(Glyph {
            metrics: GlyphMetrics {
                character: ch,
                width: m.width,
                height: m.height,
                bearing_x: m.xmin,
                bearing_y: m.ymin,
                advance: m.advance_width,
            },
            coverage,
        });
        while self.glyph_bytes + glyph.coverage.len() > GLYPH_CACHE_BYTES
            || self.glyphs.len() >= 2048
        {
            let Some((_, old)) = self.glyphs.pop_front() else {
                break;
            };
            self.glyph_bytes -= old.coverage.len();
        }
        self.glyph_bytes += glyph.coverage.len();
        self.glyphs.push_back((key, glyph.clone()));
        Ok(glyph)
    }
    pub fn cache_bytes(&self) -> usize {
        self.glyph_bytes
    }
    pub fn ascii_atlas(&mut self, id: &str, characters: &str, size: f32) -> Result<AsciiAtlas> {
        if !size.is_finite() || !(4. ..=256.).contains(&size) {
            return Err("font size must be 4..256 pixels".into());
        }
        let chars: Vec<_> = characters.chars().collect();
        if chars.is_empty()
            || chars.len() > 256
            || chars.iter().any(|c| c.is_control())
            || chars.iter().collect::<std::collections::HashSet<_>>().len() != chars.len()
        {
            return Err("ASCII atlas requires 1..256 distinct printable characters".into());
        }
        // Check the complete atlas before retaining hundreds of glyph bitmaps.
        let font = self.face(id)?;
        let metrics: Vec<_> = chars.iter().map(|c| font.metrics(*c, size)).collect();
        let max_width = metrics
            .iter()
            .map(|m| m.advance_width.max(m.width as f32 + m.xmin.abs() as f32))
            .fold(1., f32::max)
            .ceil() as u64
            + 4;
        let ascent = metrics
            .iter()
            .map(|m| m.height as i64 + i64::from(m.ymin))
            .max()
            .unwrap()
            .max(0);
        let descent = metrics
            .iter()
            .map(|m| i64::from(m.ymin))
            .min()
            .unwrap()
            .min(0);
        let height = (ascent - descent) as u64 + 4;
        let width = max_width * chars.len() as u64;
        if width > 8192 || height > 8192 || width * height > 16 * 1024 * 1024 {
            return Err("font atlas exceeds dimension or memory limits".into());
        }
        let glyphs = chars
            .into_iter()
            .map(|c| self.glyph(id, c, size))
            .collect::<Result<Vec<_>>>()?;
        let ascent = glyphs
            .iter()
            .map(|g| g.metrics.height as i32 + g.metrics.bearing_y)
            .max()
            .unwrap()
            .max(0);
        let descent = glyphs
            .iter()
            .map(|g| g.metrics.bearing_y)
            .min()
            .unwrap()
            .min(0);
        let cell = [
            (glyphs
                .iter()
                .map(|g| {
                    g.metrics
                        .advance
                        .max(g.metrics.width as f32 + g.metrics.bearing_x.abs() as f32)
                })
                .fold(1., f32::max)
                .ceil() as u32
                + 4),
            (ascent - descent) as u32 + 4,
        ];
        let mut ordered: Vec<_> = glyphs
            .into_iter()
            .map(|g| {
                let coverage = g.coverage.iter().map(|v| u64::from(*v)).sum::<u64>() as f32
                    / (cell[0] * cell[1] * 255) as f32;
                (g, coverage)
            })
            .collect();
        ordered.sort_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then(a.0.metrics.character.cmp(&b.0.metrics.character))
        });
        let mut raster = blank(cell[0] * ordered.len() as u32, cell[1])?;
        let mut characters = String::new();
        let mut coverage = Vec::new();
        for (i, (glyph, density)) in ordered.iter().enumerate() {
            let m = &glyph.metrics;
            let x = i as i32 * cell[0] as i32
                + ((cell[0] as f32 - m.advance) / 2.).floor() as i32
                + m.bearing_x;
            let y = 2 + ascent - m.bearing_y - m.height as i32;
            blit(&mut raster, glyph, x, y);
            characters.push(m.character);
            coverage.push(*density);
        }
        Ok(AsciiAtlas {
            raster,
            characters,
            cell,
            coverage,
        })
    }
    /// Basic LTR layout with kerning and wrapping. A later shaping engine can
    /// supply glyph IDs/positions without replacing this raster/cache module.
    pub fn raster_text(&mut self, id: &str, text: &str, size: f32, width: u32) -> Result<Raster> {
        if !size.is_finite() || !(4. ..=256.).contains(&size) {
            return Err("font size must be 4..256 pixels".into());
        }
        if text.chars().count() > 4096 || width == 0 || width > 8192 {
            return Err("text layout exceeds limits".into());
        }
        let font = self.face(id)?;
        let line = font
            .horizontal_line_metrics(size)
            .ok_or("font has no line metrics")?;
        let line_height = line.new_line_size.ceil().max(1.);
        let mut x = 0.0;
        let mut baseline = line.ascent.ceil();
        let mut previous = None;
        let mut placements = Vec::new();
        for ch in text.chars() {
            if ch == '\n' {
                x = 0.;
                baseline += line_height;
                previous = None;
                continue;
            }
            let glyph = self.glyph(id, ch, size)?;
            x += previous
                .and_then(|p| font.horizontal_kern(p, ch, size))
                .unwrap_or(0.);
            if x + glyph.metrics.advance > width as f32 && x > 0. {
                x = 0.;
                baseline += line_height;
            }
            placements.push((
                glyph.clone(),
                x.floor() as i32 + glyph.metrics.bearing_x,
                baseline as i32 - glyph.metrics.bearing_y - glyph.metrics.height as i32,
            ));
            x += glyph.metrics.advance;
            previous = Some(ch);
        }
        let mut raster = blank(width, (baseline - line.descent).ceil().max(1.) as u32)?;
        for (glyph, x, y) in placements {
            blit(&mut raster, &glyph, x, y);
        }
        Ok(raster)
    }
}
fn identity(id: &str) -> Result<(&str, u32)> {
    let (hash, index) = id
        .split_once('-')
        .map_or((id, 0), |(a, b)| (a, b.parse().unwrap_or(256)));
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || index > 255 {
        return Err("invalid font identity".into());
    }
    Ok((hash, index))
}
fn parse(data: &[u8], index: u32) -> Result<Font> {
    Font::from_bytes(
        data,
        FontSettings {
            collection_index: index,
            ..Default::default()
        },
    )
    .map_err(|e| format!("invalid TTF/OTF font: {e}"))
}
pub fn raster_key(raster: &Raster) -> String {
    let mut hash = Sha256::new();
    hash.update(raster.width.to_le_bytes());
    hash.update(raster.height.to_le_bytes());
    hash.update(&raster.rgba);
    format!("{:x}", hash.finalize())
}
pub fn save_raster(root: &Path, relative: &str, raster: &Raster) -> Result<()> {
    motion_core::storage::validate_relative_path(relative).map_err(|e| e.to_string())?;
    let path = root.join(relative);
    crate::contained_dir(root, path.parent().unwrap())?;
    if path.exists()
        && !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("font raster path escapes project".into());
    }
    image::save_buffer(
        path,
        &raster.rgba,
        raster.width,
        raster.height,
        image::ColorType::Rgba8,
    )
    .map_err(|e| e.to_string())
}
fn blank(width: u32, height: u32) -> Result<Raster> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16 * 1024 * 1024
    {
        return Err("font raster exceeds dimension or memory limits".into());
    }
    Ok(Raster {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    })
}
fn blit(out: &mut Raster, glyph: &Glyph, x: i32, y: i32) {
    for row in 0..glyph.metrics.height {
        for col in 0..glyph.metrics.width {
            let xx = x + col as i32;
            let yy = y + row as i32;
            if xx < 0 || yy < 0 || xx >= out.width as i32 || yy >= out.height as i32 {
                continue;
            }
            let index = (yy as usize * out.width as usize + xx as usize) * 4;
            let a = glyph.coverage[row * glyph.metrics.width + col];
            let previous = out.rgba[index + 3];
            out.rgba[index..index + 4].copy_from_slice(&[
                255,
                255,
                255,
                previous.saturating_add(((255 - previous) as u16 * a as u16 / 255) as u8),
            ]);
        }
    }
}
