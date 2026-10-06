//! Android MediaExtractor / MediaCodec. Every native object stays on its worker.
use aem_core::VideoAsset;
use aem_media::{Result, VideoProbe};
use ndk::media::{
    image_reader::{AcquireResult, Image, ImageFormat, ImageReader},
    media_codec::{
        DequeuedInputBufferResult as Input, DequeuedOutputBufferInfoResult as Output, MediaCodec,
        MediaCodecDirection,
    },
    media_format::MediaFormat,
};
use ndk_sys as ffi;
use serde_json::json;
use std::{
    fs::File,
    os::fd::AsRawFd,
    path::Path,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
static JVM: std::sync::OnceLock<jni::JavaVM> = std::sync::OnceLock::new();
pub fn set_vm(vm: jni::JavaVM) {
    let _ = JVM.set(vm);
}
pub(super) fn attach() -> Result<jni::AttachGuard<'static>> {
    JVM.get()
        .ok_or("video Java VM not initialized")?
        .attach_current_thread()
        .map_err(|e| e.to_string())
}

static CODECS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn new() -> Result<Self> {
        CODECS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| "four video decoder limit reached; release idle/frozen readers")?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        CODECS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(super) struct Extractor(pub(super) NonNull<ffi::AMediaExtractor>);
impl Extractor {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let bytes = file.metadata().map_err(|e| e.to_string())?.len();
        let p = NonNull::new(unsafe { ffi::AMediaExtractor_new() })
            .ok_or("cannot allocate video extractor")?;
        let this = Self(p);
        status(
            unsafe {
                ffi::AMediaExtractor_setDataSourceFd(p.as_ptr(), file.as_raw_fd(), 0, bytes as i64)
            },
            "open video source",
        )?;
        Ok(this)
    }
    pub(super) fn format(&self, track: u32) -> Result<MediaFormat> {
        let p = NonNull::new(unsafe {
            ffi::AMediaExtractor_getTrackFormat(self.0.as_ptr(), track as usize)
        })
        .ok_or("missing media track")?;
        Ok(unsafe { MediaFormat::from_ptr(p) })
    }
    pub(super) fn select(&self, track: u32) -> Result<()> {
        status(
            unsafe { ffi::AMediaExtractor_selectTrack(self.0.as_ptr(), track as usize) },
            "select video track",
        )
    }
    pub(super) fn pts(&self) -> i64 {
        unsafe { ffi::AMediaExtractor_getSampleTime(self.0.as_ptr()) }
    }
    pub(super) fn eos(&self) -> bool {
        (unsafe { ffi::AMediaExtractor_getSampleTrackIndex(self.0.as_ptr()) }) < 0
    }
    pub(super) fn advance(&self) -> bool {
        unsafe { ffi::AMediaExtractor_advance(self.0.as_ptr()) }
    }
    fn seek(&self, pts: u64) -> Result<()> {
        status(
            unsafe {
                ffi::AMediaExtractor_seekTo(
                    self.0.as_ptr(),
                    pts as i64,
                    ffi::SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC,
                )
            },
            "seek video",
        )
    }
}
impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe {
            ffi::AMediaExtractor_delete(self.0.as_ptr());
        }
    }
}
fn status(s: ffi::media_status_t, label: &str) -> Result<()> {
    if s == ffi::media_status_t::AMEDIA_OK {
        Ok(())
    } else {
        Err(format!("{label}: {s:?}"))
    }
}
fn ndk<T>(v: std::result::Result<T, ndk::media_error::MediaError>) -> Result<T> {
    v.map_err(|e| e.to_string())
}

pub fn probe(
    path: &Path,
    selected: Option<u32>,
    audio_selected: Option<u32>,
    check: &dyn Fn() -> Result<()>,
) -> Result<VideoProbe> {
    let _attach = attach()?;
    // Let MediaExtractor sniff owned source bytes. Filenames are not a format gate.
    let ex = Extractor::open(path)?;
    let count = unsafe { ffi::AMediaExtractor_getTrackCount(ex.0.as_ptr()) };
    if count == 0 || count > 32 {
        return Err("invalid media track count".into());
    }
    let mut video = None;
    let mut tracks = Vec::new();
    let mut chosen_audio = None;
    let mut has_audio = false;
    for i in 0..count {
        check()?;
        let mut f = ex.format(i as u32)?;
        let mime = f.str("mime").unwrap_or("").to_string();
        if mime.starts_with("video/") && selected.map_or(aem_media::VIDEO_MIMES.contains(&mime.as_str()), |n| n == i as u32) && video.is_none() {
            video = Some(i as u32);
        }
        if mime.starts_with("audio/") {
            has_audio = true;
            let rate = f.i32("sample-rate").unwrap_or(0);
            let channels = f.i32("channel-count").unwrap_or(0);
            let supported = aem_media::NATIVE_AUDIO_MIMES.contains(&mime.as_str())
                && (8000..=192000).contains(&rate)
                && matches!(channels, 1 | 2);
            tracks.push(json!({"track":i,"mime":mime,"sample_rate":rate,"channels":channels,"duration_us":f.i64("durationUs"),"supported":supported}));
            if supported && audio_selected.is_none_or(|n| n == i as u32) && chosen_audio.is_none() {
                chosen_audio = Some(i as u32);
            }
        }
    }
    if audio_selected != Some(u32::MAX) && has_audio && chosen_audio.is_none() {
        return Err("video has no supported selected mono/stereo audio track; explicitly use with_audio:false to discard audio".into());
    }
    if audio_selected.is_some_and(|n| n != u32::MAX) && chosen_audio != audio_selected {
        return Err("selected video audio track is unavailable".into());
    }
    let track = video.ok_or("selected video track missing")?;
    let mut format = ex.format(track)?;
    let mime = format.str("mime").unwrap_or("").to_string();
    if !aem_media::VIDEO_MIMES.contains(&mime.as_str()) {
        return Err(format!("unsupported video codec: {mime}"));
    }
    let vui = match mime.as_str() {
        "video/avc" => aem_media::avc_metadata(format.buffer("csd-0").ok_or("H.264 SPS missing")?)?,
        "video/hevc" => aem_media::hevc_metadata(format.buffer("csd-0").ok_or("HEVC SPS missing")?)?,
        "video/x-vnd.on2.vp9" => {
            // VP9 profile zero is always 8-bit 4:2:0. Verify the first keyframe
            // even when the extractor omits the profile key.
            let check_ex = Extractor::open(path)?; check_ex.select(track)?;
            let mut first = vec![0u8; 1024 * 1024];
            let n = unsafe { ffi::AMediaExtractor_readSampleData(check_ex.0.as_ptr(), first.as_mut_ptr().cast(), first.len()) };
            if n <= 0 || n as usize > first.len() { return Err("invalid VP9 first packet".into()); }
            aem_media::vp9_metadata(&first[..n as usize])?
        }
        _ => aem_media::AvcMetadata::default(),
    };
    let container = if aem_media::source_extension(path)?=="mkv" {Some(aem_media::matroska_metadata(path)?)}else{None};
    let container_track=container.as_ref().and_then(|m|{
        let number=format.i32("track-id").unwrap_or(track as i32+1) as u64;
        m.tracks.iter().find(|t|t.number==number)
    });
    if container_track.is_some_and(|t|t.primaries==Some(9)||t.transfer.is_some_and(|v|matches!(v,16|18))||t.bit_depth.is_some_and(|v|v>8)) {
        return Err("HDR/10-bit Matroska video unsupported".into());
    }
    let container_standard=match container_track.and_then(|t|t.matrix) {
        Some(1)=>Some(1),Some(5)=>Some(2),Some(6)=>Some(4),Some(2)|None=>None,
        _=>return Err("unsupported Matroska colour matrix".into()),
    };
    let container_range=match container_track.and_then(|t|t.range) {
        Some(1)=>Some(2),Some(2)=>Some(1),Some(0)|None=>None,
        _=>return Err("unsupported Matroska colour range".into()),
    };
    if format
        .i32("color-transfer")
        .is_some_and(|v| matches!(v, 6 | 7))
        || format.i32("color-standard") == Some(6)
    {
        return Err("HDR/BT.2020 video is unsupported".into());
    }
    if format
        .i32("sar-width")
        .zip(format.i32("sar-height"))
        .is_some_and(|(w, h)| w != h)
    {
        return Err("non-square video pixels are unsupported".into());
    }
    let width = format.i32("width").unwrap_or(0) as u32;
    let height = format.i32("height").unwrap_or(0) as u32;
    let rotation = format.i32("rotation-degrees").unwrap_or(0) as u32;
    let declared = format.i64("durationUs").unwrap_or(0);
    let rate = format.i32("frame-rate").unwrap_or(0);
    let standard = vui
        .color_standard
        .or(container_standard)
        .map(|v| v as i32)
        .or_else(|| format.i32("color-standard"))
        .filter(|v| *v != 0)
        .unwrap_or(if width >= 1280 || height > 576 { 1 } else { 4 }) as u32;
    let range = vui
        .color_range
        .or(container_range)
        .map(|v| v as i32)
        .or_else(|| format.i32("color-range"))
        .filter(|v| *v != 0)
        .unwrap_or(2) as u32;
    if width == 0
        || height == 0
        || width > 1920
        || height > 1920
        || u64::from(width) * u64::from(height) > 1920 * 1080
    {
        return Err("video exceeds 1080p decode budget".into());
    }
    ex.select(track)?;
    let mut timestamps = Vec::new();
    let started = Instant::now();
    while !ex.eos() {
        check()?;
        if started.elapsed() > Duration::from_secs(30) {
            return Err("video timestamp scan timed out".into());
        }
        if unsafe { ffi::AMediaExtractor_getSampleFlags(ex.0.as_ptr()) } & 2 != 0 {
            return Err("encrypted video is unsupported".into());
        }
        let time = ex.pts();
        if time >= 0 {
            timestamps.push(time as u64);
        }
        if timestamps.len() > 500_000 {
            return Err("video exceeds timestamp cache budget".into());
        }
        if !ex.advance() {
            break;
        }
    }
    timestamps.sort_unstable();
    timestamps.dedup();
    if timestamps.is_empty() {
        return Err("video has no visible frames".into());
    }
    let mut deltas = timestamps
        .windows(2)
        .map(|v| v[1] - v[0])
        .collect::<Vec<_>>();
    deltas.sort_unstable();
    let step = deltas
        .get(deltas.len() / 2)
        .copied()
        .unwrap_or(if rate > 0 {
            1_000_000 / rate as u64
        } else {
            33_333
        });
    let last = *timestamps.last().unwrap();
    let end = if declared > last as i64 {
        declared as u64
    } else {
        last.checked_add(step).ok_or("video end overflow")?
    };
    let (dw, dh) = if rotation % 180 == 0 {
        (width, height)
    } else {
        (height, width)
    };
    let asset = VideoAsset {
        id: 1,
        path: "assets/probed-video.mp4".into(),
        bytes: std::fs::metadata(path).map_err(|e| e.to_string())?.len(),
        mime,
        track,
        width,
        height,
        rotation,
        display_width: dw,
        display_height: dh,
        video_start_us: timestamps[0],
        video_end_us: end,
        duration_us: end,
        frame_count: timestamps.len() as u32,
        variable_frame_rate: deltas.iter().any(|d| d.abs_diff(step) > 2),
        nominal_frame_rate: if rate > 0 {
            f64::from(rate)
        } else {
            1_000_000.0 / step as f64
        },
        color_standard: standard,
        color_range: range,
        audio_asset: None,
    };
    asset.validate().map_err(|e| format!("{e}; codec={}, color_standard={}, color_range={}",asset.mime,asset.color_standard,asset.color_range))?;
    let mut decoder = Decoder::new(path, asset.clone(), timestamps.clone())?;
    let first = decoder.frame(asset.video_start_us, check)?;
    if timestamps.len() > 1 {
        decoder.frame(last, check)?;
    }
    Ok(VideoProbe {
        asset,
        timestamps,
        audio_track: if audio_selected == Some(u32::MAX) {
            None
        } else {
            chosen_audio
        },
        first_rgba: first.rgba,
        tracks: json!(tracks),
    })
}

pub struct DecodedFrame {
    pub rgba: Vec<u8>,
    pub pts: u64,
    pub end: u64,
    pub width: u32,
    pub height: u32,
    pub decode_us: u64,
    pub source_transfer: &'static str,
    pub decoder_name: String,
}
pub struct Decoder {
    codec: MediaCodec,
    reader: Option<ImageReader>,
    extractor: Extractor,
    asset: VideoAsset,
    pts: Vec<u64>,
    last_output: Option<i64>,
    input_eos: bool,
    path: std::path::PathBuf,
    reader_kind: usize,
    decoder_name: String,
    _permit: Option<Permit>,
    _attach: jni::AttachGuard<'static>,
}
impl Decoder {
    pub fn new(path: &Path, asset: VideoAsset, pts: Vec<u64>) -> Result<Self> {
        Self::with_format(path, asset, pts, 0)
    }
    fn with_format(
        path: &Path,
        asset: VideoAsset,
        pts: Vec<u64>,
        reader_kind: usize,
    ) -> Result<Self> {
        let attached = attach()?;
        let permit = Permit::new()?;
        let ex = Extractor::open(path)?;
        ex.select(asset.track)?;
        let mut f = ex.format(asset.track)?;
        f.set_i32("rotation-degrees", 0);
        f.set_i32("color-standard", asset.color_standard as i32);
        f.set_i32("color-range", asset.color_range as i32);
        f.set_i32(
            "color-format",
            if reader_kind == 1 { 21 } else { 0x7f420888 },
        );
        let reader = if reader_kind == 1 {
            None
        } else {
            Some(
                ndk(ImageReader::new(
                    asset.width as i32,
                    asset.height as i32,
                    [
                        ImageFormat::YUV_420_888,
                        ImageFormat::YUV_420_888, // Byte-buffer mode has no image reader.
                        ImageFormat::RGBA_8888,
                        ImageFormat::RGB_565,
                    ][reader_kind],
                    3,
                ))
                .map_err(|e| format!("video reader allocation: {e}"))?,
            )
        };
        let codec = MediaCodec::from_decoder_type(&asset.mime).ok_or_else(||format!("no {} decoder on this device", asset.mime))?;
        let window = reader.as_ref().map(|r| ndk(r.window())).transpose()?;
        ndk(codec.configure(&f, window.as_ref(), MediaCodecDirection::Decoder))?;
        ndk(codec.start())?;
        let decoder_name = codec.name().unwrap_or_else(|_| "unknown".into());
        Ok(Self {
            codec,
            reader,
            extractor: ex,
            asset,
            pts,
            last_output: None,
            input_eos: false,
            path: path.into(),
            reader_kind,
            decoder_name,
            _permit: Some(permit),
            _attach: attached,
        })
    }
    pub fn frame(&mut self, target: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        loop {
            match self.frame_once(target, check) {
                Err(error) if error.starts_with("video surface format") && self.reader_kind < 3 => {
                    let mut next = self.reader_kind + 1;
                    // Stop before allocating the replacement so codec concurrency stays bounded.
                    let _ = self.codec.stop();
                    self._permit.take();
                    loop {
                        match Self::with_format(
                            &self.path,
                            self.asset.clone(),
                            self.pts.clone(),
                            next,
                        ) {
                            Ok(replacement) => {
                                *self = replacement;
                                break;
                            }
                            Err(_) if next < 3 => next += 1,
                            Err(e) => return Err(e),
                        }
                    }
                }
                result => return result,
            }
        }
    }
    fn frame_once(&mut self, target: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        if target < self.asset.video_start_us || target >= self.asset.video_end_us {
            return Err("source time outside visible video".into());
        }
        let index = self.pts.partition_point(|p| *p <= target) - 1;
        let wanted = self.pts[index];
        let end = self
            .pts
            .get(index + 1)
            .copied()
            .unwrap_or(self.asset.video_end_us);
        if self.last_output.is_none_or(|last| {
            wanted <= last.max(0) as u64 || wanted.saturating_sub(last.max(0) as u64) > 500_000
        }) {
            ndk(self.codec.flush())?;
            self.extractor.seek(wanted)?;
            self.input_eos = false;
            if let Some(reader) = &self.reader {
                loop {
                    match ndk(reader.acquire_next_image())? {
                        AcquireResult::Image(_) => {}
                        _ => break,
                    }
                }
            }
        }
        let started = Instant::now();
        loop {
            check()?;
            if started.elapsed() > Duration::from_secs(10) {
                return Err("video decoder timed out".into());
            }
            if !self.input_eos {
                if let Input::Buffer(mut input) =
                    ndk(self.codec.dequeue_input_buffer(Duration::ZERO))?
                {
                    let eos = self.extractor.eos();
                    let time = self.extractor.pts();
                    let buffer = input.buffer_mut();
                    let bytes = if eos {
                        0
                    } else {
                        let n = unsafe {
                            ffi::AMediaExtractor_readSampleData(
                                self.extractor.0.as_ptr(),
                                buffer.as_mut_ptr().cast(),
                                buffer.len(),
                            )
                        };
                        if n < 0 || n as usize > buffer.len() {
                            return Err("video sample read failed".into());
                        }
                        n as usize
                    };
                    ndk(self.codec.queue_input_buffer(
                        input,
                        0,
                        bytes,
                        if eos { 0 } else { time as u64 },
                        if eos { 4 } else { 0 },
                    ))?;
                    if eos {
                        self.input_eos = true;
                    } else {
                        self.extractor.advance();
                    }
                }
            }
            match ndk(self.codec.dequeue_output_buffer(Duration::from_millis(2)))? {
                Output::Buffer(output) => {
                    let info = *output.info();
                    let pts = info.presentation_time_us();
                    let eos = info.flags() & 4 != 0;
                    let matches = pts >= 0 && pts as u64 == wanted && !(eos && info.size() == 0);
                    if matches && self.reader.is_none() {
                        let converted = (|| {
                            let format = output.format();
                            if info.offset() < 0 || info.size() <= 0 {
                                return Err("video surface format: invalid output range".into());
                            }
                            let bytes = output
                                .buffer()
                                .get(
                                    info.offset() as usize
                                        ..info.offset() as usize + info.size() as usize,
                                )
                                .ok_or("video surface format: invalid output offset")?;
                            convert_buffer(bytes, &format, &self.asset)
                                .map_err(|e| format!("video surface format: {e}"))
                        })();
                        ndk(self.codec.release_output_buffer(output, false))?;
                        self.last_output = Some(pts);
                        return Ok(DecodedFrame {
                            rgba: converted?,
                            pts: wanted,
                            end,
                            width: self.asset.display_width,
                            height: self.asset.display_height,
                            decode_us: started.elapsed().as_micros() as u64,
                            source_transfer: "yuv420_buffer",
                            decoder_name: self.decoder_name.clone(),
                        });
                    }
                    ndk(self.codec.release_output_buffer(output, matches))?;
                    self.last_output = Some(pts);
                    if matches {
                        loop {
                            check()?;
                            if started.elapsed() > Duration::from_secs(10) {
                                return Err("video surface transfer timed out".into());
                            }
                            match ndk(self.reader.as_ref().unwrap().acquire_next_image())
                                .map_err(|e| format!("video surface format: {e}"))?
                            {
                                AcquireResult::Image(image) => {
                                    let stamp = ndk(image.timestamp())? / 1000;
                                    if stamp != pts {
                                        continue;
                                    }
                                    let format = self.codec.output_format();
                                    let standard = format
                                        .i32("color-standard")
                                        .filter(|v| matches!(v, 1 | 2 | 4))
                                        .unwrap_or(self.asset.color_standard as i32)
                                        as u32;
                                    let range = format
                                        .i32("color-range")
                                        .filter(|v| matches!(v, 1 | 2))
                                        .unwrap_or(self.asset.color_range as i32)
                                        as u32;
                                    let rgba = convert(&image, &self.asset, standard, range)?;
                                    return Ok(DecodedFrame {
                                        rgba,
                                        pts: wanted,
                                        end,
                                        width: self.asset.display_width,
                                        height: self.asset.display_height,
                                        decode_us: started.elapsed().as_micros() as u64,
                                        source_transfer: [
                                            "yuv420_888",
                                            "yuv420_buffer",
                                            "rgba8888",
                                            "rgb565",
                                        ][self.reader_kind],
                                        decoder_name: self.decoder_name.clone(),
                                    });
                                }
                                AcquireResult::NoBufferAvailable => {
                                    std::thread::sleep(Duration::from_millis(1))
                                }
                                AcquireResult::MaxImagesAcquired => {
                                    return Err("video image queue budget exceeded".into())
                                }
                            }
                        }
                    }
                    if eos || pts > wanted as i64 {
                        return Err(format!(
                            "video decoder skipped target PTS {wanted} (output {pts})"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}
fn convert(image: &Image, a: &VideoAsset, standard: u32, range: u32) -> Result<Vec<u8>> {
    let format = ndk(image.format())?;
    if matches!(format, ImageFormat::RGBA_8888 | ImageFormat::RGB_565) {
        let crop = ndk(image.crop_rect())?;
        if crop.right - crop.left != a.width as i32
            || crop.bottom - crop.top != a.height as i32
            || crop.left < 0
            || crop.top < 0
        {
            return Err("decoded RGB crop mismatch".into());
        }
        let bytes = ndk(image.plane_data(0))?;
        let row = ndk(image.plane_row_stride(0))?;
        let stride = ndk(image.plane_pixel_stride(0))?;
        if row <= 0 || stride < if format == ImageFormat::RGB_565 { 2 } else { 4 } {
            return Err("invalid RGB plane strides".into());
        }
        let mut rgba = vec![0; (a.display_width * a.display_height * 4) as usize];
        for y in 0..a.height {
            for x in 0..a.width {
                let at = (y + crop.top as u32) as usize * row as usize
                    + (x + crop.left as u32) as usize * stride as usize;
                let rgb = if format == ImageFormat::RGB_565 {
                    let pair = bytes.get(at..at + 2).ok_or("RGB565 plane too short")?;
                    let v = u16::from_le_bytes(pair.try_into().unwrap());
                    [
                        (((v >> 11) & 31) * 255 / 31) as u8,
                        (((v >> 5) & 63) * 255 / 63) as u8,
                        ((v & 31) * 255 / 31) as u8,
                        255,
                    ]
                } else {
                    let p = bytes.get(at..at + 4).ok_or("RGBA plane too short")?;
                    [p[0], p[1], p[2], 255]
                };
                let (dx, dy) = match a.rotation {
                    90 => (a.height - 1 - y, x),
                    180 => (a.width - 1 - x, a.height - 1 - y),
                    270 => (y, a.width - 1 - x),
                    _ => (x, y),
                };
                let to = (dy * a.display_width + dx) as usize * 4;
                rgba[to..to + 4].copy_from_slice(&rgb);
            }
        }
        return Ok(rgba);
    }
    if ndk(image.format())? != ImageFormat::YUV_420_888 || ndk(image.number_of_planes())? != 3 {
        return Err("device did not provide 8-bit YUV420 video".into());
    }
    let crop = ndk(image.crop_rect())?;
    let w = crop.right - crop.left;
    let h = crop.bottom - crop.top;
    if w != a.width as i32 || h != a.height as i32 || crop.left < 0 || crop.top < 0 {
        return Err("decoded crop differs from source metadata".into());
    }
    let planes = [
        ndk(image.plane_data(0))?,
        ndk(image.plane_data(1))?,
        ndk(image.plane_data(2))?,
    ];
    let rows = [
        ndk(image.plane_row_stride(0))?,
        ndk(image.plane_row_stride(1))?,
        ndk(image.plane_row_stride(2))?,
    ];
    let pixels = [
        ndk(image.plane_pixel_stride(0))?,
        ndk(image.plane_pixel_stride(1))?,
        ndk(image.plane_pixel_stride(2))?,
    ];
    if rows.iter().chain(pixels.iter()).any(|s| *s <= 0) {
        return Err("invalid YUV plane stride".into());
    }
    let sample = |p: usize, x: i32, y: i32| -> Result<i32> {
        let at = y as usize * rows[p] as usize + x as usize * pixels[p] as usize;
        planes[p]
            .get(at)
            .copied()
            .map(i32::from)
            .ok_or("YUV plane buffer too short".into())
    };
    convert_samples(a, crop.left, crop.top, standard, range, sample)
}
fn convert_buffer(bytes: &[u8], format: &MediaFormat, a: &VideoAsset) -> Result<Vec<u8>> {
    let kind = format
        .i32("color-format")
        .ok_or("missing buffer colour format")?;
    if !matches!(kind, 19 | 21) {
        return Err(format!("unsupported YUV buffer layout {kind}"));
    }
    let stride = format.i32("stride").unwrap_or(a.width as i32);
    let slice = format
        .i32("slice-height")
        .filter(|h| *h > 0)
        .unwrap_or(a.height as i32);
    let left = format.i32("crop-left").unwrap_or(0);
    let top = format.i32("crop-top").unwrap_or(0);
    let right = format.i32("crop-right").unwrap_or(a.width as i32 - 1);
    let bottom = format.i32("crop-bottom").unwrap_or(a.height as i32 - 1);
    if stride <= 0
        || stride > 8192
        || slice <= 0
        || slice > 8192
        || left < 0
        || top < 0
        || right >= stride
        || bottom >= slice
        || right - left + 1 != a.width as i32
        || bottom - top + 1 != a.height as i32
    {
        return Err("invalid buffer crop/stride".into());
    }
    let (stride, slice) = (stride as usize, slice as usize);
    let uv = stride * slice;
    let chroma_stride = if kind == 19 {
        stride.div_ceil(2)
    } else {
        stride
    };
    let chroma_plane = chroma_stride * slice.div_ceil(2);
    let sample = |p: usize, x: i32, y: i32| -> Result<i32> {
        let (x, y) = (x as usize, y as usize);
        let at = match p {
            0 => y * stride + x,
            1 => uv + y * chroma_stride + x * if kind == 19 { 1 } else { 2 },
            _ => {
                if kind == 19 {
                    uv + chroma_plane + y * chroma_stride + x
                } else {
                    uv + y * chroma_stride + x * 2 + 1
                }
            }
        };
        bytes
            .get(at)
            .copied()
            .map(i32::from)
            .ok_or("YUV buffer too short".into())
    };
    let standard = format
        .i32("color-standard")
        .filter(|v| matches!(v, 1 | 2 | 4))
        .unwrap_or(a.color_standard as i32) as u32;
    let range = format
        .i32("color-range")
        .filter(|v| matches!(v, 1 | 2))
        .unwrap_or(a.color_range as i32) as u32;
    convert_samples(a, left, top, standard, range, sample)
}
fn convert_samples(
    a: &VideoAsset,
    left: i32,
    top: i32,
    standard: u32,
    range: u32,
    sample: impl Fn(usize, i32, i32) -> Result<i32>,
) -> Result<Vec<u8>> {
    let (w, h) = (a.width as i32, a.height as i32);
    let mut rgba = vec![0; (a.display_width * a.display_height * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let sx = x + left;
            let sy = y + top;
            let yy = sample(0, sx, sy)?;
            let u = sample(1, sx / 2, sy / 2)? - 128;
            let v = sample(2, sx / 2, sy / 2)? - 128;
            let (r, g, b) = if range == 1 {
                if standard == 1 {
                    (
                        256 * yy + 403 * v,
                        256 * yy - 48 * u - 120 * v,
                        256 * yy + 475 * u,
                    )
                } else {
                    (
                        256 * yy + 359 * v,
                        256 * yy - 88 * u - 183 * v,
                        256 * yy + 454 * u,
                    )
                }
            } else {
                let c = 298 * (yy - 16);
                if standard == 1 {
                    (c + 459 * v, c - 55 * u - 136 * v, c + 541 * u)
                } else {
                    (c + 409 * v, c - 100 * u - 208 * v, c + 516 * u)
                }
            };
            let (dx, dy) = match a.rotation {
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (y, w - 1 - x),
                _ => (x, y),
            };
            let at = (dy as u32 * a.display_width + dx as u32) as usize * 4;
            rgba[at..at + 4].copy_from_slice(&[
                ((r + 128) >> 8).clamp(0, 255) as u8,
                ((g + 128) >> 8).clamp(0, 255) as u8,
                ((b + 128) >> 8).clamp(0, 255) as u8,
                255,
            ]);
        }
    }
    Ok(rgba)
}
