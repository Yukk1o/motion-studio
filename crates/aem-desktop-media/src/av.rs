//! libav probe, decode and encode.
//!
//! Only compiled with the `ffmpeg` feature. The decoder keeps the same
//! contract as the Android MediaCodec backend: `frame` honours `check` between
//! packets, seeks happen on the decoder thread rather than the render thread,
//! and frames are handed to the renderer as YUV so the GPU does the colour
//! conversion exactly as it does on Android.
use aem_core::VideoAsset;
use aem_host::platform::VideoDecoder;
use aem_host::video_frame::{DecodedFrame, VideoPixels};
use aem_host::Result;
use aem_media::VideoProbe;
use aem_render::Yuv420Frame;
use ffmpeg_next::{
    codec::packet::Packet,
    decoder::Decoder as AvDecoder,
    format::context::Input,
    software::decoding::decoder::Video as VideoDecoderBackend,
    software::pixel_format::Pixel,
    util::frame,
    ChannelLayout,
};
use std::{path::Path, time::Instant};

/// Reuse the host's pure seek decisions so desktop scrubbing behaves identically.
use aem_host::video_decode_policy;

/// Containers and codecs the import contract promises.
fn container_supported(path: &Path) -> Result<()> {
    let extension = aem_media::source_extension(path)?;
    if matches!(extension.as_str(), "mp4" | "mov" | "3gp" | "mkv" | "webm") {
        Ok(())
    } else {
        Err(format!("unsupported video container .{extension}"))
    }
}

/// Probe a source and build the importable asset description.
pub fn probe(
    path: &Path,
    selected: Option<u32>,
    audio_selected: Option<u32>,
    check: &dyn Fn() -> Result<()>,
) -> Result<VideoProbe> {
    check()?;
    container_supported(path)?;
    let input = Input::open(path).map_err(|e| format!("cannot open video: {e}"))?;
    let metadata = input.metadata();
    let stream = pick_video_stream(metadata, selected)?;
    check()?;
    let time_base = stream.time_base();
    let duration = stream.duration() as u64 * time_base.numer() as u64
        / time_base.denom().max(1) as u64;
    let (width, height) = (stream.width(), stream.height());
    if width == 0 || height == 0 {
        return Err("video stream has no dimensions".into());
    }
    if width > aem_core::MAX_VIDEO_DIMENSION
        || height > aem_core::MAX_VIDEO_DIMENSION
        || u64::from(width) * u64::from(height) > aem_core::MAX_VIDEO_PIXELS
    {
        return Err("video exceeds the maximum source dimensions".into());
    }
    let mut timestamps = Vec::new();
    for packet in input
        .packets()
        .map_err(|e| format!("cannot read video packets: {e}"))?
    {
        check()?;
        if packet.is_key() {
            timestamps.push(pts_us(&packet, time_base));
        }
    }
    timestamps.sort_unstable();
    timestamps.dedup();
    if timestamps.is_empty() {
        return Err("video contains no key frames".into());
    }
    let last = *timestamps.last().unwrap();
    let first = timestamps[0];
    if duration == 0 {
        return Err("video reports a zero duration".into());
    }
    let bytes = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let audio_stream = metadata
        .streams()
        .best(
            aem_media::NATIVE_AUDIO_MIMES
                .iter()
                .filter_map(|mime| ffmpeg_next::codec::Id::from_str_name(mime).ok())
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .ok();
    let audio_track = audio_stream.map(|s| s.index());
    let _ = audio_selected;
    let display_width = width.min(aem_core::MAX_VIDEO_DIMENSION);
    let display_height = height.min(aem_core::MAX_VIDEO_DIMENSION);
    let asset = VideoAsset {
        id: 1,
        path: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        bytes,
        width,
        height,
        duration_us: duration * 1_000_000,
        frame_rate: 0,
        video_start_us: first,
        video_end_us: last,
        audio_track,
        display_width,
        display_height,
    };
    let tracks = serde_json::json!({
        "video": {"index": stream.index(), "codec": stream.codec_name(), "width": width, "height": height},
    });
    // Decode the first key frame so the import preview has real pixels.
    let first_rgba = decode_first_frame(input, stream.index(), timestamps[0], check)?;
    Ok(VideoProbe {
        asset,
        timestamps,
        audio_track,
        first_rgba,
        tracks,
    })
}

fn pick_video_stream<'a>(
    metadata: &'a ffmpeg_next::format::context::InputMetadata,
    selected: Option<u32>,
) -> Result<ffmpeg_next::format::stream::Stream<'a>> {
    let by_index = |wanted: Option<u32>| -> Result<_> {
        let index = wanted.unwrap_or(0);
        metadata
            .streams()
            .best(std::iter::once(index), |s| {
                s.parameters().medium() == ffmpeg_next::media::Type::Video
            })
            .ok_or("no video stream in this file")
    };
    by_index(selected)
}

fn pts_us(packet: &Packet, time_base: ffmpeg_next::Rational) -> u64 {
    let pts = packet.pts().max(0) as u64;
    pts * time_base.numer() as u64 / time_base.denom().max(1) as u64
}

fn decode_first_frame(
    input: Input,
    stream_index: usize,
    target: u64,
    check: &dyn Fn() -> Result<()>,
) -> Result<Vec<u8>> {
    let mut input = input;
    let mut decoder = VideoDecoderBackend::new();
    input
        .seek(timestamp(target), ffmpeg_next::format::seek::Whence::Backward)
        .map_err(|e| format!("cannot seek to first frame: {e}"))?;
    let mut decoded = Vec::new();
    for (stream, packet) in input.packets().map_err(|e| e.to_string())? {
        check()?;
        if stream.index() != stream_index {
            continue;
        }
        if decoder.send_packet(&packet).is_err() {
            break;
        }
        if let Ok(frame) = decoder.receive_frame() {
            decoded = frame
                .to_vec()
                .map_err(|e| format!("cannot read decoded frame: {e}"))?;
            break;
        }
    }
    Ok(decoded)
}

fn timestamp(micros: u64) -> i64 {
    // libav seeks on a stream time base; microseconds are the shared unit and
    // the default stream time base is a microsecond.
    micros as i64
}

/// One decoded source, positioned by presentation timestamp.
pub struct Decoder {
    input: Input,
    video: usize,
    decoder: VideoDecoderBackend,
    time_base: ffmpeg_next::Rational,
    pending: Option<frame::Video>,
    next_pts: u64,
    end_pts: u64,
    asset: VideoAsset,
    paths: Vec<u64>,
    pub seeks: u64,
    last_output: Option<i64>,
}

impl Decoder {
    pub fn open(path: &Path, asset: VideoAsset, paths: Vec<u64>) -> Result<Self> {
        let input = Input::open(path).map_err(|e| format!("cannot open video: {e}"))?;
        let stream = pick_video_stream(input.metadata(), None)?;
        let video = stream.index();
        let time_base = stream.time_base();
        let decoder = VideoDecoderBackend::new();
        let end_pts = *paths.last().unwrap_or(&0);
        Ok(Self {
            input,
            video,
            decoder,
            time_base,
            pending: None,
            next_pts: 0,
            end_pts,
            asset,
            paths,
            seeks: 0,
            last_output: None,
        })
    }

    /// Find the frame covering `target` and convert it to a host frame.
    pub fn frame(&mut self, target: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        let began = Instant::now();
        check()?;
        let wanted = *self
            .paths
            .partition_point(|p| *p <= target)
            .checked_sub(1)
            .ok_or("video PTS index missing")?;
        // Reuse the host's seek policy so scrubbing feels the same on both
        // platforms: a small forward miss drains the pipeline instead of
        // discarding decoded work.
        let index = self
            .pending
            .as_ref()
            .filter(|f| f.pts() >= 0)
            .map(|f| pts_us_frame(f, self.time_base) as i64);
        if video_decode_policy::needs_seek(index, self.paths[wanted], None) {
            self.input
                .seek(
                    self.paths[wanted] as i64,
                    ffmpeg_next::format::seek::Whence::AvFrame,
                )
                .map_err(|e| format!("seek failed: {e}"))?;
            self.decoder.flush();
            self.next_pts = self.paths[wanted];
            self.seeks += 1;
        }
        while self.next_pts < self.paths[wanted] || self.pending.is_none() {
            check()?;
            let Some((stream, packet)) = self
                .input
                .packets()
                .map_err(|e| format!("read failed: {e}"))?
                .find(|(s, _)| s.index() == self.video)
            else {
                break;
            };
            let packet_pts = pts_us(&packet, self.time_base);
            if self.decoder.send_packet(&packet).is_ok() {
                if let Ok(decoded) = self.decoder.receive_frame() {
                    self.pending = Some(decoded);
                    self.next_pts = packet_pts;
                }
            }
            if packet_pts > self.paths[wanted] {
                break;
            }
        }
        let decoded = self
            .pending
            .take()
            .ok_or("video frame pending".to_string())?;
        check()?;
        let pixels = convert(&decoded, &self.asset)?;
        let pts = pts_us_frame(&decoded, self.time_base);
        let end = self
            .paths
            .iter()
            .copied()
            .find(|p| *p > pts)
            .unwrap_or(self.end_pts);
        self.last_output = Some(pts as i64);
        Ok(DecodedFrame {
            bytes: pixels.bytes(),
            pixels,
            pts,
            end: end.max(pts + 1),
            width: self.asset.display_width,
            height: self.asset.display_height,
            decode_us: began.elapsed().as_micros() as u64,
            codec_us: 0,
            transfer_us: 0,
            pack_us: 0,
            source_transfer: "libav",
            decoder_name: "libavcodec".into(),
        })
    }
}

impl VideoDecoder for Decoder {
    fn frame(&mut self, target_us: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        Decoder::frame(self, target_us, check)
    }

    fn seeks(&self) -> u64 {
        self.seeks
    }
}

fn pts_us_frame(frame: &frame::Video, time_base: ffmpeg_next::Rational) -> u64 {
    let pts = frame.pts().max(0) as u64;
    pts * time_base.numer() as u64 / time_base.denom().max(1) as u64
}

/// Convert a decoded frame to the host's YUV plane layout.
///
/// Keeping the frame in YUV avoids a full-resolution RGB conversion in the
/// decoder and lets the existing GPU conversion path run unchanged.
fn convert(frame: &frame::Video, asset: &VideoAsset) -> Result<VideoPixels> {
    let data = frame
        .to_vec()
        .map_err(|e| format!("cannot read decoded frame: {e}"))?;
    match frame.format() {
        Pixel::YUV420P => {
            let width = frame.width().min(asset.display_width);
            let height = frame.height().min(asset.display_height);
            let planes = Yuv420Frame::pack(width, height, &data);
            Ok(VideoPixels::Yuv(planes))
        }
        _ => Ok(VideoPixels::Rgba(data)),
    }
}

/// Encoder settings shared by the desktop video exporter.
#[derive(Clone, Copy)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl EncoderSettings {
    /// Match the Android exporter's bitrate policy: 2–20 Mbps scaled by area.
    pub fn bitrate(&self) -> i64 {
        let raw = i64::from(self.width) * i64::from(self.height) * i64::from(self.fps) / 8;
        raw.clamp(2_000_000, 20_000_000)
    }
}

/// Audio layout for the AAC track written by the exporter.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;
pub const OUTPUT_LAYOUT: ChannelLayout = ChannelLayout::STEREO;