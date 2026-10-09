//! Bounded image decoding. Dimensions describe the source, never the proxy.
use aem_core::{Asset, Scene};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
};

pub const MAX_ENCODED_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PREVIEW_EDGE: u32 = 2048;
pub const IDLE_TEXTURE_BYTES: u64 = 32 * 1024 * 1024;
pub const PREFETCH_SECONDS: f64 = 0.5;
pub const MAX_PREFETCH_IMAGES: usize = 2;

/// Inspect clip dependencies without sampling expressions or animation. Times
/// are mapped through nested clips in seconds, including mixed frame rates.
pub fn upcoming_assets(project: &aem_core::Project, frame: f64) -> Vec<u64> {
    fn visit(
        document: &aem_core::Project,
        layers: &[aem_core::Layer],
        fps: u32,
        frames: u32,
        begin: f64,
        end: f64,
        delay: f64,
        depth: usize,
        found: &mut std::collections::BTreeMap<u64, f64>,
    ) {
        if depth >= aem_core::composition::MAX_COMPOSITION_DEPTH {
            return;
        }
        for layer in layers.iter().filter(|l| l.visible) {
            let clip = layer.clip(frames);
            let start = begin.max(f64::from(clip.in_frame)).max(0.);
            let finish = end.min(f64::from(clip.out_frame)).min(f64::from(frames));
            if start >= finish {
                continue;
            }
            let wait = delay + (start - begin) / f64::from(fps);
            match &layer.content {
                aem_core::Content::Image { asset }
                | aem_core::Content::Text {
                    raster_asset: asset,
                    ..
                } => {
                    found
                        .entry(*asset)
                        .and_modify(|t| *t = t.min(wait))
                        .or_insert(wait);
                }
                aem_core::Content::Composition { clip } => {
                    if let Some(child) = document
                        .compositions
                        .iter()
                        .find(|c| c.id == clip.composition)
                    {
                        visit(
                            document,
                            &child.layers,
                            child.fps,
                            child.frames,
                            clip.source_frame(layer.local_frame(start), fps, child.fps),
                            clip.source_frame(layer.local_frame(finish), fps, child.fps),
                            wait,
                            depth + 1,
                            found,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if !frame.is_finite() || frame < 0. {
        return vec![];
    }
    let mut found = std::collections::BTreeMap::new();
    visit(
        project,
        &project.layers,
        project.fps,
        project.frames,
        frame,
        frame + f64::from(project.fps) * PREFETCH_SECONDS,
        0.,
        0,
        &mut found,
    );
    let mut ordered: Vec<_> = found.into_iter().collect();
    ordered.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    ordered.into_iter().map(|(id, _)| id).collect()
}

pub(crate) const CANCELLED: &str = "image decode superseded";
fn check_cancel(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err(CANCELLED.into())
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Full,
    Preview(u32),
}
impl Resolution {
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        let edge = match self {
            Self::Full => return (width, height),
            Self::Preview(edge) => edge.clamp(1, MAX_PREVIEW_EDGE),
        };
        let longest = width.max(height);
        if longest <= edge {
            (width, height)
        } else {
            // Integer ceil avoids a zero short side and never exceeds edge.
            (
                ((u64::from(width) * u64::from(edge)).div_ceil(u64::from(longest))) as u32,
                ((u64::from(height) * u64::from(edge)).div_ceil(u64::from(longest))) as u32,
            )
        }
    }
}

/// Collect sampled dependencies recursively, including alpha occluders. Scene
/// sampling already excludes inactive clips; offscreen geometry is kept because
/// effects can move it into the viewport or use it for occlusion.
pub fn scene_assets(scene: &Scene) -> BTreeSet<u64> {
    fn collect(scene: &Scene, ids: &mut BTreeSet<u64>) {
        ids.extend(scene.layers.iter().filter_map(|l| l.asset));
        ids.extend(scene.effects.iter().filter(|e| e.enabled && scene.layers.iter().any(|l|l.id==e.layer))
            .filter_map(|e| match e.image_input { Some(aem_core::EffectImageInput::Asset {asset}) => Some(asset), _ => None }));
        ids.extend(scene.effects.iter()
            .filter(|effect| effect.enabled && scene.layers.iter().any(|layer| layer.id == effect.layer))
            .filter_map(|effect| effect.scene.as_ref().and_then(|settings| settings.sprite_asset)));
        for child in &scene.nested {
            collect(&child.scene, ids);
        }
    }
    let mut ids = BTreeSet::new();
    collect(scene, &mut ids);
    ids
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: u64,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub root: PathBuf,
}
impl Source {
    pub fn new(root: &Path, asset: &Asset) -> Result<Self, String> {
        aem_core::storage::validate_relative_path(&asset.path).map_err(|e| e.to_string())?;
        let base = root.canonicalize().map_err(|e| e.to_string())?;
        let path = root
            .join(&asset.path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(&base) {
            return Err("image resolves outside project directory".into());
        }
        Ok(Self {
            id: asset.id,
            path,
            width: asset.width,
            height: asset.height,
            root: base,
        })
    }
    pub fn validate(&self) -> Result<image::ImageFormat, String> {
        let (width, height, format, _) = inspect(&self.path)?;
        if (width, height) != (self.width, self.height) {
            return Err(format!("image {} metadata mismatch", self.id));
        }
        Ok(format)
    }
}

pub fn inspect(path: &Path) -> Result<(u32, u32, image::ImageFormat, u64), String> {
    let meta = path.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_ENCODED_BYTES {
        return Err("encoded image must be 1 byte..64 MiB".into());
    }
    let reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let format = reader.format().ok_or("unsupported image format")?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err("image dimensions must be 1..16384".into());
    }
    Ok((w, h, format, meta.len()))
}

pub struct Pixels {
    pub width: u32,
    pub height: u32,
    /// sRGB encoded, premultiplied in linear space, matching the renderer.
    pub rgba: Vec<u8>,
    pub cached: bool,
}

pub fn decode(source: &Source, resolution: Resolution) -> Result<Pixels, String> {
    decode_cancellable(source, resolution, None)
}
fn decode_cancellable(
    source: &Source,
    resolution: Resolution,
    cancel: Option<&AtomicBool>,
) -> Result<Pixels, String> {
    check_cancel(cancel)?;
    let (w, h) = resolution.dimensions(source.width, source.height);
    let cost = u64::from(w) * u64::from(h) * 4;
    if cost > MAX_DECODED_BYTES {
        return Err("decoded image exceeds 128 MiB".into());
    }
    source.validate()?;
    let cache = if matches!(resolution, Resolution::Preview(_))
        && (w, h) != (source.width, source.height)
    {
        proxy_path(source, w, h, cancel)
    } else {
        None
    };
    if let Some(path) = &cache {
        if let Some(rgba) = read_proxy(path, w, h) {
            return Ok(Pixels {
                width: w,
                height: h,
                rgba,
                cached: true,
            });
        }
    }
    check_cancel(cancel)?;
    let mut pixels = Pixels {
        width: w,
        height: h,
        rgba: vec![0; cost as usize],
        cached: false,
    };
    decode_into_cancellable(source, resolution, &mut pixels.rgba, cancel)?;
    check_cancel(cancel)?;
    if let Some(path) = &cache {
        write_proxy(path, &pixels);
    }
    Ok(pixels)
}

// Disposable cache, outside assets/ and project packages. Keys include a full
// source SHA-256 and algorithm version; corrupt cache data is never trusted.
pub const PROXY_CACHE_BYTES: u64 = 64 * 1024 * 1024;
static CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static CACHE_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn proxy_path(source: &Source, w: u32, h: u32, cancel: Option<&AtomicBool>) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    source.validate().ok()?;
    let mut hash = Sha256::new();
    hash.update(b"motion-image-proxy-linear-box-v1");
    hash.update(w.to_le_bytes());
    hash.update(h.to_le_bytes());
    let mut file = File::open(&source.path).ok()?;
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0;
    loop {
        check_cancel(cancel).ok()?;
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_ENCODED_BYTES {
            return None;
        }
        hash.update(&buffer[..n]);
    }
    let directory = source.root.join(".cache/image-proxies-v1");
    fs::create_dir_all(&directory).ok()?;
    let directory = directory.canonicalize().ok()?;
    if !directory.starts_with(&source.root) {
        return None;
    }
    Some(directory.join(format!("{:x}.rgba", hash.finalize())))
}
fn read_proxy(path: &Path, w: u32, h: u32) -> Option<Vec<u8>> {
    use sha2::{Digest, Sha256};
    let _guard = CACHE_LOCK.lock().ok()?;
    let mut file = File::open(path).ok()?;
    let cost = u64::from(w) * u64::from(h) * 4;
    if file.metadata().ok()?.len() != cost + 44 {
        return None;
    }
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[..4] != b"MSIP"
        || header[4..8] != w.to_le_bytes()
        || header[8..12] != h.to_le_bytes()
    {
        return None;
    }
    let mut rgba = vec![0; cost as usize];
    file.read_exact(&mut rgba).ok()?;
    let mut digest = [0u8; 32];
    file.read_exact(&mut digest).ok()?;
    if Sha256::digest(&rgba).as_slice() != digest {
        return None;
    }
    Some(rgba)
}
fn write_proxy(path: &Path, pixels: &Pixels) {
    use sha2::{Digest, Sha256};
    let Ok(_guard) = CACHE_LOCK.lock() else {
        return;
    };
    let Some(directory) = path.parent() else {
        return;
    };
    let cost = pixels.rgba.len() as u64 + 44;
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut old: Vec<_> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_str()?.to_owned();
            if name.len() != 69
                || !name.ends_with(".rgba")
                || !name[..64].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((m.modified().ok()?, e.path(), m.len()))
        })
        .collect();
    old.sort_by_key(|e| e.0);
    let mut total: u64 = old.iter().map(|e| e.2).sum();
    // Rebuild corrupt entries using exclusive temporary files and atomic rename.
    if let Ok(meta) = path.metadata() {
        if fs::remove_file(path).is_err() {
            return;
        }
        total = total.saturating_sub(meta.len());
    }
    for (_, previous, bytes) in old {
        if total + cost <= PROXY_CACHE_BYTES {
            break;
        }
        if previous != path && fs::remove_file(previous).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
    if total + cost > PROXY_CACHE_BYTES {
        return;
    }
    let serial = CACHE_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = directory.join(format!("proxy-{}-{serial}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(b"MSIP")?;
        file.write_all(&pixels.width.to_le_bytes())?;
        file.write_all(&pixels.height.to_le_bytes())?;
        file.write_all(&pixels.rgba)?;
        file.write_all(&Sha256::digest(&pixels.rgba))?;
        file.flush()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
}

/// The PNG path writes directly into the caller's allocation, including full
/// resolution export; no full-size intermediate RGBA/Java byte[] is required.
pub fn decode_into(
    source: &Source,
    resolution: Resolution,
    output: &mut [u8],
) -> Result<(), String> {
    decode_into_cancellable(source, resolution, output, None)
}
fn decode_into_cancellable(
    source: &Source,
    resolution: Resolution,
    output: &mut [u8],
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let format = source.validate()?;
    let (w, h) = resolution.dimensions(source.width, source.height);
    let cost = u64::from(w) * u64::from(h) * 4;
    if cost > MAX_DECODED_BYTES || output.len() as u64 != cost {
        return Err("invalid image output length or 128 MiB budget exceeded".into());
    }
    if format == image::ImageFormat::Png {
        return png_into(source, w, h, output, cancel);
    }
    // Existing JPEG resources remain supported. Its decoder needs a full image,
    // so validate that cost before allocating even when requesting a proxy.
    if u64::from(source.width) * u64::from(source.height) * 4 > MAX_DECODED_BYTES {
        return Err("non-PNG source decode exceeds 128 MiB".into());
    }
    let mut reader = image::ImageReader::open(&source.path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    check_cancel(cancel)?;
    use image::GenericImageView;
    let lut = linear_lut();
    let mut row = vec![[0.; 4]; w as usize];
    let mut rows = 0;
    let mut y_out = 0;
    for y in 0..source.height {
        check_cancel(cancel)?;
        for x in 0..source.width {
            accumulate(
                &mut row[(u64::from(x) * u64::from(w) / u64::from(source.width)) as usize],
                image.get_pixel(x, y).0,
                &lut,
            );
        }
        rows += 1;
        if y + 1 == source.height
            || (u64::from(y + 1) * u64::from(h) / u64::from(source.height)) > u64::from(y_out)
        {
            finish_row(
                &mut row,
                rows,
                source.width,
                w,
                &mut output[(y_out * w * 4) as usize..((y_out + 1) * w * 4) as usize],
            );
            rows = 0;
            y_out += 1;
        }
    }
    Ok(())
}

fn linear_lut() -> [f32; 256] {
    std::array::from_fn(|i| {
        let v = i as f32 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}
fn encode(v: f32) -> u8 {
    let v = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (v.clamp(0., 1.) * 255.).round() as u8
}
fn accumulate(sum: &mut [f32; 4], pixel: [u8; 4], lut: &[f32; 256]) {
    let a = pixel[3] as f32 / 255.;
    for c in 0..3 {
        sum[c] += lut[pixel[c] as usize] * a;
    }
    sum[3] += a;
}
fn finish_row(row: &mut [[f32; 4]], rows: u32, sw: u32, w: u32, output: &mut [u8]) {
    for (x, (sum, pixel)) in row.iter_mut().zip(output.chunks_exact_mut(4)).enumerate() {
        let left = (x as u64 * u64::from(sw)).div_ceil(u64::from(w));
        let right = ((x as u64 + 1) * u64::from(sw)).div_ceil(u64::from(w));
        let n = ((right - left) * u64::from(rows)) as f32;
        for c in 0..3 {
            pixel[c] = encode(sum[c] / n);
        }
        pixel[3] = (sum[3] / n * 255.).round() as u8;
        *sum = [0.; 4];
    }
}
fn png_into(
    source: &Source,
    w: u32,
    h: u32,
    output: &mut [u8],
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let mut decoder = png::Decoder::new(BufReader::new(
        File::open(&source.path).map_err(|e| e.to_string())?,
    ));
    decoder.set_limits(png::Limits {
        bytes: 16 * 1024 * 1024,
    });
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    if reader.info().width != source.width || reader.info().height != source.height {
        return Err("PNG metadata changed during decoding".into());
    }
    let color = reader.output_color_type().0;
    let channels = color.samples();
    let lut = linear_lut();
    let rgba = |pixel: &[u8]| match color {
        png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
        png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
        png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
        png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
        _ => unreachable!("EXPAND removes indexed output"),
    };
    if reader.info().interlaced {
        // Adam7 rows visit each source pixel exactly once. Proxy accumulators
        // are bounded by the proxy dimensions, never by the original image.
        let full = (w, h) == (source.width, source.height);
        let mut sums = if full {
            Vec::new()
        } else {
            vec![[0.; 4]; (w * h) as usize]
        };
        for (x0, y0, dx, dy) in [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ] {
            if x0 >= source.width {
                continue;
            }
            for y in (y0..source.height).step_by(dy) {
                check_cancel(cancel)?;
                let row = reader
                    .next_row()
                    .map_err(|e| e.to_string())?
                    .ok_or("truncated Adam7 image")?;
                let xs = (x0..source.width).step_by(dx);
                if row.data().len() != xs.clone().count() * channels {
                    return Err("invalid Adam7 row".into());
                }
                for (x, p) in xs.zip(row.data().chunks_exact(channels)) {
                    let xo = u64::from(x) * u64::from(w) / u64::from(source.width);
                    let yo = u64::from(y) * u64::from(h) / u64::from(source.height);
                    let offset = (yo * u64::from(w) + xo) as usize;
                    if full {
                        let mut sum = [0.; 4];
                        accumulate(&mut sum, rgba(p), &lut);
                        for c in 0..3 {
                            output[offset * 4 + c] = encode(sum[c]);
                        }
                        output[offset * 4 + 3] = rgba(p)[3];
                    } else {
                        accumulate(&mut sums[offset], rgba(p), &lut);
                    }
                }
            }
        }
        if !full {
            for y in 0..h {
                check_cancel(cancel)?;
                let rows = ((u64::from(y + 1) * u64::from(source.height)).div_ceil(u64::from(h))
                    - (u64::from(y) * u64::from(source.height)).div_ceil(u64::from(h)))
                    as u32;
                finish_row(
                    &mut sums[(y * w) as usize..((y + 1) * w) as usize],
                    rows,
                    source.width,
                    w,
                    &mut output[(y * w * 4) as usize..((y + 1) * w * 4) as usize],
                );
            }
        }
    } else {
        if (w, h) == (source.width, source.height) {
            for y in 0..source.height {
                check_cancel(cancel)?;
                let row = reader
                    .next_row()
                    .map_err(|e| e.to_string())?
                    .ok_or("truncated PNG image")?;
                if row.data().len() != source.width as usize * channels {
                    return Err("invalid PNG row".into());
                }
                let target = &mut output[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
                for (input, pixel) in row
                    .data()
                    .chunks_exact(channels)
                    .zip(target.chunks_exact_mut(4))
                {
                    let p = rgba(input);
                    if p[3] == 255 {
                        pixel.copy_from_slice(&p);
                    } else {
                        let a = p[3] as f32 / 255.;
                        for c in 0..3 {
                            pixel[c] = encode(lut[p[c] as usize] * a);
                        }
                        pixel[3] = p[3];
                    }
                }
            }
            reader.finish().map_err(|e| e.to_string())?;
            return Ok(());
        }
        let mut sums = vec![[0.; 4]; w as usize];
        let mut rows = 0;
        let mut yo = 0;
        for y in 0..source.height {
            check_cancel(cancel)?;
            let row = reader
                .next_row()
                .map_err(|e| e.to_string())?
                .ok_or("truncated PNG image")?;
            if row.data().len() != source.width as usize * channels {
                return Err("invalid PNG row".into());
            }
            for (x, p) in row.data().chunks_exact(channels).enumerate() {
                accumulate(
                    &mut sums[(x as u64 * u64::from(w) / u64::from(source.width)) as usize],
                    rgba(p),
                    &lut,
                );
            }
            rows += 1;
            if y + 1 == source.height
                || (u64::from(y + 1) * u64::from(h) / u64::from(source.height)) > u64::from(yo)
            {
                finish_row(
                    &mut sums,
                    rows,
                    source.width,
                    w,
                    &mut output[(yo * w * 4) as usize..((yo + 1) * w * 4) as usize],
                );
                rows = 0;
                yo += 1;
            }
        }
    }
    reader.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// One outstanding decode per renderer, with at most one result allocation.
/// Decoding never holds a session/GPU lock. Stale results are discarded by the
/// owner using the exact source and requested dimensions.
#[derive(Default)]
pub struct DecodeTask {
    pending: Option<(
        Source,
        Resolution,
        bool,
        Arc<AtomicBool>,
        Receiver<Result<Pixels, String>>,
    )>,
}
impl DecodeTask {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn cancel_unwanted(&mut self, wanted: &BTreeSet<u64>, resolution: Resolution) {
        if let Some((source, mode, _, cancel, _)) = &self.pending {
            if *mode != resolution || !wanted.contains(&source.id) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }
    pub fn cancel(&mut self) {
        if let Some((_, _, _, cancel, _)) = &self.pending {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    pub fn start(
        &mut self,
        source: Source,
        resolution: Resolution,
        speculative: bool,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("an image decode is already pending".into());
        }
        let (send, receive) = mpsc::sync_channel(1);
        let input = source.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("motion-image-decode".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    decode_cancellable(&input, resolution, Some(&worker_cancel))
                })
                .unwrap_or_else(|_| Err("image decoder panicked".into()));
                let _ = send.send(result);
            })
            .map_err(|e| e.to_string())?;
        self.pending = Some((source, resolution, speculative, cancel, receive));
        Ok(())
    }
    pub fn poll(&mut self) -> Option<(Source, Resolution, bool, Result<Pixels, String>)> {
        let (_, _, _, _, receive) = self.pending.as_ref()?;
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(_) => Err("image decode worker disconnected".into()),
        };
        let (source, resolution, speculative, cancel, _) = self.pending.take().unwrap();
        Some((
            source,
            resolution,
            speculative,
            if cancel.load(Ordering::Relaxed) {
                Err(CANCELLED.into())
            } else {
                result
            },
        ))
    }
}
impl Drop for DecodeTask {
    fn drop(&mut self) {
        self.cancel();
    }
}
