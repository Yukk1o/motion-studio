//! Optional libav probing and YUV decoding. Export encoding remains pending.
use av::{codec, color, format, frame, media, Error, Packet, Rational};
use ffmpeg_next as av;
use motion_core::VideoAsset;
use motion_host::{
    platform::VideoDecoder,
    video_decode_policy,
    video_frame::{DecodedFrame, VideoPixels},
    Result,
};
use motion_media::VideoProbe;
use motion_render::{VideoPlane, Yuv420Frame};
use std::{path::Path, time::Instant};

fn input(path: &Path) -> Result<format::context::Input> {
    av::init().map_err(|e| e.to_string())?;
    // Sniff source bytes: owned imports need not retain their original extension.
    format::input(path).map_err(|e| format!("cannot open video: {e}"))
}
fn video_stream(
    input: &format::context::Input,
    selected: Option<u32>,
) -> Result<format::stream::Stream<'_>> {
    let stream = match selected {
        Some(index) => input.stream(index as usize),
        None => input.streams().best(media::Type::Video),
    }
    .ok_or("selected video track missing")?;
    if stream.parameters().medium() != media::Type::Video {
        return Err("selected track is not video".into());
    }
    Ok(stream)
}
fn mime(id: codec::Id) -> Result<&'static str> {
    match id {
        codec::Id::H264 => Ok("video/avc"),
        codec::Id::HEVC => Ok("video/hevc"),
        codec::Id::VP8 => Ok("video/x-vnd.on2.vp8"),
        codec::Id::VP9 => Ok("video/x-vnd.on2.vp9"),
        _ => Err(format!("unsupported video codec: {id:?}")),
    }
}
fn micros(pts: i64, base: Rational) -> Result<u64> {
    if pts < 0 || base.numerator() <= 0 || base.denominator() <= 0 {
        return Err("invalid video timestamp or time base".into());
    }
    let n = i128::from(pts) * i128::from(base.numerator()) * 1_000_000;
    u64::try_from((n + i128::from(base.denominator()) / 2) / i128::from(base.denominator()))
        .map_err(|_| "video timestamp overflow".into())
}
fn rotation(stream: &format::stream::Stream<'_>) -> Result<u32> {
    let mut degrees = stream
        .metadata()
        .get("rotate")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    if let Some(data) = stream
        .side_data()
        .find(|d| d.kind() == codec::packet::side_data::Type::DisplayMatrix)
    {
        if data.data().len() != 36 {
            return Err("invalid video display matrix".into());
        }
        let mut matrix = [0i32; 9];
        for (v, bytes) in matrix.iter_mut().zip(data.data().chunks_exact(4)) {
            *v = i32::from_ne_bytes(bytes.try_into().unwrap());
        }
        let determinant = i64::from(matrix[0]) * i64::from(matrix[4])
            - i64::from(matrix[1]) * i64::from(matrix[3]);
        if determinant <= 0 {
            return Err("mirrored video display matrix unsupported".into());
        }
        // FFmpeg's angle is counterclockwise; the renderer uses clockwise.
        degrees = -unsafe { av::ffi::av_display_rotation_get(matrix.as_ptr()) };
    }
    let quarter = (degrees / 90.0).round();
    if !degrees.is_finite() || (degrees - quarter * 90.0).abs() > 0.01 {
        return Err("unsupported video rotation".into());
    }
    Ok(((quarter as i64).rem_euclid(4) * 90) as u32)
}
fn audio_tracks(
    input: &format::context::Input,
    selected: Option<u32>,
) -> Result<(Option<u32>, serde_json::Value)> {
    let mut tracks = Vec::new();
    let mut chosen = None;
    for stream in input
        .streams()
        .filter(|s| s.parameters().medium() == media::Type::Audio)
    {
        let decoder = codec::context::Context::from_parameters(stream.parameters())
            .and_then(|c| c.decoder().audio());
        let (rate, channels) = decoder
            .as_ref()
            .map(|d| (d.rate(), d.channels()))
            .unwrap_or((0, 0));
        let supported = (8000..=192000).contains(&rate) && matches!(channels, 1 | 2);
        let index = stream.index() as u32;
        if supported && chosen.is_none() && selected.is_none_or(|n| n == index) {
            chosen = Some(index);
        }
        tracks.push(serde_json::json!({"track":index,"codec":format!("{:?}",stream.parameters().id()),"sample_rate":rate,"channels":channels,"supported":supported}));
    }
    if selected == Some(u32::MAX) {
        chosen = None;
    } else if (!tracks.is_empty() && chosen.is_none())
        || selected.is_some_and(|n| Some(n) != chosen)
    {
        return Err(
            "selected audio track unavailable; use with_audio:false to discard audio".into(),
        );
    }
    Ok((chosen, serde_json::json!(tracks)))
}

pub fn probe(
    path: &Path,
    selected: Option<u32>,
    audio_selected: Option<u32>,
    check: &dyn Fn() -> Result<()>,
) -> Result<VideoProbe> {
    check()?;
    let mut input = input(path)?;
    if input.nb_streams() == 0 || input.nb_streams() > 32 {
        return Err("invalid media track count".into());
    }
    let stream = video_stream(&input, selected)?;
    let track = stream.index();
    let base = stream.time_base();
    let mime = mime(stream.parameters().id())?.to_string();
    let video = codec::context::Context::from_parameters(stream.parameters())
        .and_then(|c| c.decoder().video())
        .map_err(|e| e.to_string())?;
    let (width, height) = (video.width(), video.height());
    let aspect = video.aspect_ratio();
    if aspect.numerator() > 0 && aspect.numerator() != aspect.denominator() {
        return Err("non-square video pixels unsupported".into());
    }
    if width == 0
        || height == 0
        || width > motion_core::MAX_VIDEO_DIMENSION
        || height > motion_core::MAX_VIDEO_DIMENSION
        || u64::from(width) * u64::from(height) > motion_core::MAX_VIDEO_PIXELS
    {
        return Err("video exceeds source pixel budget".into());
    }
    let rotation = rotation(&stream)?;
    let rate = f64::from(stream.avg_frame_rate());
    let declared = micros(stream.duration().max(0), base)?;
    let (audio_track, tracks) = audio_tracks(&input, audio_selected)?;
    let bytes = path.metadata().map_err(|e| e.to_string())?.len();
    let began = Instant::now();
    let budget = motion_media::timestamp_scan_budget(bytes);
    let mut timestamps = Vec::new();
    let mut packet_end = 0;
    loop {
        check()?;
        if began.elapsed() > budget {
            return Err("video timestamp scan timed out".into());
        }
        let mut packet = Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(Error::Eof) => break,
            Err(e) => return Err(format!("read video index: {e}")),
        }
        if packet.stream() != track {
            continue;
        }
        let pts = packet
            .pts()
            .ok_or("video packet has no presentation timestamp")?;
        if pts < 0 {
            continue;
        }
        let time = micros(pts, base)?;
        timestamps.push(time);
        packet_end = packet_end.max(time.saturating_add(micros(packet.duration().max(0), base)?));
        if timestamps.len() > motion_core::MAX_VIDEO_FRAMES as usize {
            return Err("video timestamp cache budget exceeded".into());
        }
    }
    timestamps.sort_unstable();
    timestamps.dedup();
    let first = *timestamps.first().ok_or("video has no visible frames")?;
    let last = *timestamps.last().unwrap();
    let mut deltas: Vec<_> = timestamps.windows(2).map(|w| w[1] - w[0]).collect();
    deltas.sort_unstable();
    let step = deltas
        .get(deltas.len() / 2)
        .copied()
        .unwrap_or(if rate.is_finite() && rate > 0.0 {
            (1_000_000.0 / rate).round().max(1.0) as u64
        } else {
            33_333
        });
    let variable = deltas.iter().any(|d| d.abs_diff(step) > 2);
    // Track duration is a duration, not an absolute timestamp. Preserve leading edits.
    let end = packet_end
        .max(first.saturating_add(declared))
        .max(last.saturating_add(step));
    let (display_width, display_height) = if rotation % 180 == 0 {
        (width, height)
    } else {
        (height, width)
    };
    let mut asset = VideoAsset {
        id: 1,
        path: "assets/probed-video.mp4".into(),
        bytes,
        mime,
        track: track as u32,
        width,
        height,
        rotation,
        display_width,
        display_height,
        video_start_us: first,
        video_end_us: end,
        duration_us: end,
        frame_count: timestamps.len() as u32,
        variable_frame_rate: variable,
        nominal_frame_rate: motion_media::source_frame_rate(rate, &timestamps, step, variable),
        color_standard: if width >= 1280 || height > 576 { 1 } else { 4 },
        color_range: 2,
        audio_asset: None,
    };
    asset.validate().map_err(|e| e.to_string())?;
    let mut decoder = Decoder::open(path, asset.clone(), timestamps.clone())?;
    let first_frame = decoder.decode_to(first, check)?;
    (asset.color_standard, asset.color_range) = colors(&first_frame)?;
    decoder.asset = asset.clone();
    let first_rgba = match convert(&first_frame, &asset)? {
        VideoPixels::Yuv(p) => p.to_rgba().map_err(|e| e.to_string())?,
        VideoPixels::Rgba(p) => p,
    };
    if timestamps.len() > 1 {
        decoder.frame(last, check)?;
    }
    Ok(VideoProbe {
        asset,
        timestamps,
        audio_track,
        first_rgba,
        tracks,
    })
}

pub struct Decoder {
    input: format::context::Input,
    video: codec::decoder::Video,
    base: Rational,
    asset: VideoAsset,
    timestamps: Vec<u64>,
    draining: bool,
    last_decoded: Option<i64>,
    seeks: u64,
}
impl Decoder {
    pub fn open(path: &Path, asset: VideoAsset, timestamps: Vec<u64>) -> Result<Self> {
        asset.validate().map_err(|e| e.to_string())?;
        if timestamps.len() != asset.frame_count as usize
            || timestamps.first() != Some(&asset.video_start_us)
            || timestamps.windows(2).any(|w| w[0] >= w[1])
            || timestamps.last().is_none_or(|p| *p >= asset.video_end_us)
        {
            return Err("invalid video timestamp index".into());
        }
        let input = input(path)?;
        let stream = video_stream(&input, Some(asset.track))?;
        let base = stream.time_base();
        let video = codec::context::Context::from_parameters(stream.parameters())
            .and_then(|c| c.decoder().video())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            input,
            video,
            base,
            asset,
            timestamps,
            draining: false,
            last_decoded: None,
            seeks: 0,
        })
    }
    fn next(&mut self, check: &dyn Fn() -> Result<()>) -> Result<Option<frame::Video>> {
        loop {
            check()?;
            let mut frame = frame::Video::empty();
            match self.video.receive_frame(&mut frame) {
                Ok(()) => return Ok(Some(frame)),
                Err(Error::Eof) => return Ok(None),
                Err(Error::Other { errno }) if errno == av::util::error::EAGAIN => {}
                Err(e) => return Err(format!("decode video: {e}")),
            }
            if self.draining {
                return Err("video decoder did not drain at EOF".into());
            }
            let mut packet = Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.asset.track as usize => self
                    .video
                    .send_packet(&packet)
                    .map_err(|e| format!("submit video: {e}"))?,
                Ok(()) => {}
                Err(Error::Eof) => {
                    self.video.send_eof().map_err(|e| e.to_string())?;
                    self.draining = true;
                }
                Err(e) => return Err(format!("read video: {e}")),
            }
        }
    }
    fn decode_to(&mut self, wanted: u64, check: &dyn Fn() -> Result<()>) -> Result<frame::Video> {
        check()?;
        if video_decode_policy::needs_seek(self.last_decoded, wanted, None) {
            let target = i64::try_from(wanted).map_err(|_| "seek timestamp overflow")?;
            // Input::seek uses AV_TIME_BASE (microseconds) with stream index -1.
            self.input
                .seek(target, ..target)
                .map_err(|e| format!("seek video: {e}"))?;
            self.video.flush();
            self.draining = false;
            self.last_decoded = None;
            self.seeks += 1;
        }
        while let Some(frame) = self.next(check)? {
            let pts = frame
                .timestamp()
                .or(frame.pts())
                .ok_or("decoded frame has no timestamp")?;
            if pts < 0 {
                continue;
            }
            let time = micros(pts, self.base)?;
            self.last_decoded = Some(time as i64);
            if time > wanted {
                return Err("decoded frame does not match presentation index".into());
            }
            if time == wanted {
                check()?;
                return Ok(frame);
            }
        }
        Err("video ended before requested frame".into())
    }
    pub fn frame(&mut self, target: u64, check: &dyn Fn() -> Result<()>) -> Result<DecodedFrame> {
        let began = Instant::now();
        let index = self
            .timestamps
            .partition_point(|p| *p <= target)
            .saturating_sub(1);
        let wanted = self.timestamps[index];
        let decoded = self.decode_to(wanted, check)?;
        let codec_us = began.elapsed().as_micros() as u64;
        let packed = Instant::now();
        let pixels = convert(&decoded, &self.asset)?;
        check()?;
        Ok(DecodedFrame {
            pixels,
            pts: wanted,
            end: self
                .timestamps
                .get(index + 1)
                .copied()
                .unwrap_or(self.asset.video_end_us),
            width: self.asset.display_width,
            height: self.asset.display_height,
            decode_us: began.elapsed().as_micros() as u64,
            codec_us,
            transfer_us: 0,
            pack_us: packed.elapsed().as_micros() as u64,
            source_transfer: "libav-yuv",
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
fn colors(frame: &frame::Video) -> Result<(u32, u32)> {
    use color::{Space, TransferCharacteristic as Transfer};
    if matches!(
        frame.color_transfer_characteristic(),
        Transfer::SMPTE2084 | Transfer::ARIB_STD_B67
    ) || matches!(frame.color_space(), Space::BT2020NCL | Space::BT2020CL)
    {
        return Err("HDR/BT.2020 video unsupported".into());
    }
    let standard = match frame.color_space() {
        Space::BT709 => 1,
        Space::BT470BG => 2,
        Space::SMPTE170M => 4,
        Space::Unspecified => {
            if frame.width() >= 1280 || frame.height() > 576 {
                1
            } else {
                4
            }
        }
        _ => return Err("unsupported SDR color matrix".into()),
    };
    let range =
        if frame.color_range() == color::Range::JPEG || frame.format() == format::Pixel::YUVJ420P {
            1
        } else {
            2
        };
    Ok((standard, range))
}
fn convert(frame: &frame::Video, asset: &VideoAsset) -> Result<VideoPixels> {
    if frame.width() != asset.width || frame.height() != asset.height {
        return Err("video changed dimensions".into());
    }
    let (standard, range) = colors(frame)?;
    if (standard, range) != (asset.color_standard, asset.color_range) {
        return Err("video changed color space".into());
    }
    let (u, v, stride, pixel_stride) = match frame.format() {
        format::Pixel::YUV420P | format::Pixel::YUVJ420P if frame.planes() >= 3 => (
            frame.data(1),
            frame.data(2),
            [frame.stride(1), frame.stride(2)],
            1,
        ),
        format::Pixel::NV12 | format::Pixel::NV21 if frame.planes() >= 2 => {
            let chroma = frame.data(1);
            let other = chroma.get(1..).ok_or("invalid interleaved video plane")?;
            let (u, v) = if frame.format() == format::Pixel::NV12 {
                (chroma, other)
            } else {
                (other, chroma)
            };
            (u, v, [frame.stride(1); 2], 2)
        }
        _ => {
            return Err(format!(
                "unsupported video pixels {:?}; requires 8-bit 4:2:0 SDR",
                frame.format()
            ))
        }
    };
    Yuv420Frame::pack(
        asset.width,
        asset.height,
        [0, 0],
        asset.rotation,
        standard,
        range,
        [
            VideoPlane {
                data: frame.data(0),
                row_stride: frame.stride(0),
                pixel_stride: 1,
            },
            VideoPlane {
                data: u,
                row_stride: stride[0],
                pixel_stride,
            },
            VideoPlane {
                data: v,
                row_stride: stride[1],
                pixel_stride,
            },
        ],
    )
    .map(VideoPixels::Yuv)
    .map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl EncoderSettings {
    pub fn bitrate(&self) -> i64 {
        (i64::from(self.width) * i64::from(self.height) * i64::from(self.fps) / 8)
            .clamp(2_000_000, 20_000_000)
    }
}
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;
pub const OUTPUT_LAYOUT: av::ChannelLayout = av::ChannelLayout::STEREO;
