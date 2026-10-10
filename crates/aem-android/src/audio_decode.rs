//! Android audio fallback for Opus, HE-AAC and platform demuxers. No UI ownership.
use crate::video_decode::{attach, Extractor};
use aem_core::AudioAsset;
use aem_media::Result;
use ndk::media::media_codec::{
    DequeuedInputBufferResult as Input, DequeuedOutputBufferInfoResult as Output, MediaCodec,
    MediaCodecDirection,
};
use ndk_sys as ffi;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Running(MediaCodec);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}
fn ndk<T>(r: std::result::Result<T, ndk::media_error::MediaError>) -> Result<T> {
    r.map_err(|e| e.to_string())
}

pub fn decode(
    path: &Path,
    pcm: &Path,
    selected: Option<u32>,
    limit: u64,
    check: &mut dyn FnMut(f64) -> Result<()>,
) -> Result<AudioAsset> {
    check(0.0)?;
    // A platform track index is not a Matroska track number. Keep a single
    // track namespace for Android imports and linked video audio.
    let ext = aem_media::source_extension(path)?;
    if ext != "mkv" {
        match aem_media::decode_audio(path, pcm, selected, limit, check) {
            Ok(a) => return Ok(a),
            Err(rust_error) => {
                check(0.0)?; // cancellation cannot silently retry with a different decoder
                             // A damaged portable format must not become a permissive,
                             // partial native decode. Fallback covers absent demuxers/codecs.
                if matches!(ext, "wav" | "flac" | "aiff")
                    || (ext == "ogg"
                        && !rust_error.contains("unsupported")
                        && !rust_error.contains("no supported"))
                    || (matches!(ext, "mp4" | "mov" | "3gp")
                        && !rust_error.contains("unsupported")
                        && !rust_error.contains("no supported"))
                {
                    return Err(rust_error);
                }
                return native(path, pcm, selected, limit, check).map_err(|e| {
                    format!("audio unsupported or invalid: Rust: {rust_error}; Android: {e}")
                });
            }
        }
    }
    native(path, pcm, selected, limit, check)
}

fn native(
    path: &Path,
    pcm: &Path,
    selected: Option<u32>,
    limit: u64,
    check: &mut dyn FnMut(f64) -> Result<()>,
) -> Result<AudioAsset> {
    let _attached = attach()?;
    ACTIVE
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 2).then_some(n + 1)
        })
        .map_err(|_| "two native audio decoder limit reached")?;
    let _permit = Permit;
    let ex = Extractor::open(path)?;
    let count = unsafe { ffi::AMediaExtractor_getTrackCount(ex.0.as_ptr()) };
    if count == 0 || count > 32 {
        return Err("invalid audio track count".into());
    }
    let mut chosen = None;
    for i in 0..count {
        if selected.is_some_and(|n| n != i as u32) {
            continue;
        }
        let mut f = ex.format(i as u32)?;
        if f.str("mime")
            .is_some_and(|m| aem_media::NATIVE_AUDIO_MIMES.contains(&m))
        {
            chosen = Some(i as u32);
            break;
        }
    }
    let track = chosen.ok_or("no supported selected Android audio track")?;
    let mut f = ex.format(track)?;
    let mime = f.str("mime").ok_or("audio MIME missing")?.to_string();
    let container = if aem_media::source_extension(path)? == "mkv" {
        Some(aem_media::matroska_metadata(path)?)
    } else {
        None
    };
    let track_number = f.i32("track-id").unwrap_or(track as i32 + 1) as u64;
    let container_delay = container
        .as_ref()
        .and_then(|m| m.tracks.iter().find(|t| t.number == track_number))
        .map_or(0, |t| t.codec_delay_ns);
    // Extractor packet PTS omit Matroska CodecDelay. AAC needs its priming
    // samples trimmed; Vorbis/Opus already consume their initial overlap.
    let clock_delay_ns = container_delay;
    let precision_ns = container.as_ref().map_or(0, |m| m.time_scale_ns);
    let mut rate = f.i32("sample-rate").unwrap_or(0) as u32;
    let mut channels = f.i32("channel-count").unwrap_or(0) as u32;
    let valid = |r, c| (8000..=192000).contains(&r) && matches!(c, 1 | 2);
    if !valid(rate, channels) {
        return Err("audio requires mono/stereo at 8-192 kHz".into());
    }
    let declared = f.i64("durationUs").unwrap_or(0);
    if declared > 3_600_000_000 {
        return Err("audio duration exceeds one hour".into());
    }
    // Decoders consume initial delay (e.g. Opus pre-skip); do not trim twice.
    // Some extractors omit end padding, so report the actual kept PCM length.
    f.set_i32("pcm-encoding", 4);
    ex.select(track)?;
    let codec = MediaCodec::from_decoder_type(&mime)
        .ok_or_else(|| format!("no {mime} decoder on this device"))?;
    ndk(codec.configure(&f, None, MediaCodecDirection::Decoder))?;
    ndk(codec.start())?;
    let codec = Running(codec);
    let mut output = BufWriter::with_capacity(65536, File::create(pcm).map_err(|e| e.to_string())?);
    let mut total = 0u64;
    let mut decoded_frames = 0u64;
    let mut input_eos = false;
    let mut last_progress = Instant::now();
    let mut encoding = 2;
    let zeros = [0u8; 8192];
    loop {
        check(if declared > 0 {
            total as f64 * 1_000_000.0 / f64::from(rate) / declared as f64
        } else {
            0.0
        })?;
        if last_progress.elapsed() > Duration::from_secs(10) {
            return Err("audio decoder timed out".into());
        }
        if !input_eos {
            if let Input::Buffer(mut input) = ndk(codec.0.dequeue_input_buffer(Duration::ZERO))? {
                let eos = ex.eos();
                let pts = ex.pts();
                let buf = input.buffer_mut();
                if !eos && unsafe { ffi::AMediaExtractor_getSampleFlags(ex.0.as_ptr()) } & 2 != 0 {
                    return Err("encrypted audio unsupported".into());
                }
                let n = if eos {
                    0
                } else {
                    unsafe {
                        ffi::AMediaExtractor_readSampleData(
                            ex.0.as_ptr(),
                            buf.as_mut_ptr().cast(),
                            buf.len(),
                        )
                    }
                };
                if n < 0 || n as usize > buf.len() {
                    return Err("invalid audio input packet size".into());
                }
                ndk(codec.0.queue_input_buffer(
                    input,
                    0,
                    n as usize,
                    if eos { 0 } else { pts as u64 },
                    if eos { 4 } else { 0 },
                ))?;
                last_progress = Instant::now();
                if eos {
                    input_eos = true;
                } else {
                    ex.advance();
                }
            }
        }
        match ndk(codec.0.dequeue_output_buffer(Duration::from_millis(2)))? {
            Output::Buffer(buffer) => {
                let info = *buffer.info();
                let eos = info.flags() & 4 != 0;
                let result = (|| -> Result<()> {
                    if info.size() == 0 {
                        return Ok(());
                    }
                    let format = buffer.format();
                    let r = format.i32("sample-rate").unwrap_or(rate as i32) as u32;
                    let c = format.i32("channel-count").unwrap_or(channels as i32) as u32;
                    if !valid(r, c) || (total > 0 && (r != rate || c != channels)) {
                        return Err("unsupported or changing audio output format".into());
                    }
                    rate = r;
                    channels = c;
                    encoding = format.i32("pcm-encoding").unwrap_or(encoding);
                    let size = match encoding {
                        2 => 2,
                        4 => 4,
                        _ => return Err("unsupported Android PCM encoding".into()),
                    };
                    if info.offset() < 0 || info.size() < 0 {
                        return Err("invalid audio output bounds".into());
                    }
                    let data = buffer
                        .buffer()
                        .get(info.offset() as usize..info.offset() as usize + info.size() as usize)
                        .ok_or("invalid audio output buffer")?;
                    let stride = size * channels as usize;
                    if data.len() % stride != 0 || data.len() / stride > 65536 {
                        return Err("invalid audio output block".into());
                    }
                    // Opus's first, shortened output already has the post-skip
                    // timestamp. Later packets still use the extractor clock.
                    let timestamp_delay = if mime == "audio/opus" && decoded_frames == 0 {
                        0
                    } else {
                        clock_delay_ns
                    };
                    let at = (i128::from(info.presentation_time_us()) * 1000
                        - i128::from(timestamp_delay))
                        * i128::from(rate);
                    let at = (at + 500_000_000).div_euclid(1_000_000_000);
                    let jitter = (precision_ns * u64::from(rate)).div_ceil(1_000_000_000) + 2;
                    let delay_frames = if mime == "audio/mp4a-latm" {
                        (clock_delay_ns * u64::from(rate) + 500_000_000) / 1_000_000_000
                    } else {
                        0
                    };
                    let frames = (data.len() / stride) as u64;
                    let skip = if at < -(i128::from(jitter)) {
                        (-at).min(i128::from(frames)) as u64
                    } else {
                        0
                    };
                    let skip = skip.max(delay_frames.saturating_sub(decoded_frames).min(frames));
                    decoded_frames += frames;
                    let at = at.max(0) as u64;
                    // Packet timestamps may round by one sample. Preserve larger
                    // leading/interior gaps on the shared source clock.
                    let gap = if at > total + jitter { at - total } else { 0 };
                    let overlap = if total > at + jitter { total - at } else { 0 };
                    let skip = skip.saturating_add(overlap).min(frames);
                    let kept = frames - skip;
                    if (total + gap + kept) > u64::from(rate) * 3600
                        || (total + gap + kept) * u64::from(channels) * 4 > limit
                    {
                        return Err("audio cache/duration budget exceeded".into());
                    }
                    let mut gap_bytes = gap * u64::from(channels) * 4;
                    while gap_bytes > 0 {
                        check(0.0)?;
                        let n = gap_bytes.min(zeros.len() as u64) as usize;
                        output.write_all(&zeros[..n]).map_err(|e| e.to_string())?;
                        gap_bytes -= n as u64;
                    }
                    for sample in data[skip as usize * stride..].chunks_exact(size) {
                        let s = if size == 2 {
                            f32::from(i16::from_le_bytes(sample.try_into().unwrap())) / 32768.0
                        } else {
                            f32::from_le_bytes(sample.try_into().unwrap())
                        };
                        if !s.is_finite() {
                            return Err("non-finite Android audio sample".into());
                        }
                        output
                            .write_all(&s.to_le_bytes())
                            .map_err(|e| e.to_string())?;
                    }
                    total += gap + kept;
                    Ok(())
                })();
                ndk(codec.0.release_output_buffer(buffer, false))?;
                result?;
                last_progress = Instant::now();
                if eos {
                    break;
                }
            }
            Output::OutputFormatChanged => {
                let f = codec.0.output_format();
                encoding = f.i32("pcm-encoding").unwrap_or(2);
            }
            _ => {}
        }
    }
    if total == 0 {
        return Err("audio is empty".into());
    }
    output.flush().map_err(|e| e.to_string())?;
    output.get_ref().sync_all().map_err(|e| e.to_string())?;
    let ext = aem_media::source_extension(path)?;
    let container = match ext {
        "mp4" | "mov" | "3gp" => "audio/mp4",
        "mkv" => "audio/x-matroska",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        _ => match mime.as_str() {
            "audio/mp4a-latm" => "audio/aac",
            "audio/opus" => "audio/opus",
            "audio/mpeg" => "audio/mpeg",
            "audio/3gpp" => "audio/3gpp",
            "audio/amr-wb" => "audio/amr-wb",
            _ => return Err("unsupported owned audio container".into()),
        },
    };
    let a = AudioAsset {
        id: 1,
        path: "assets/probed-audio.media".into(),
        bytes: std::fs::metadata(path).map_err(|e| e.to_string())?.len(),
        mime: container.into(),
        track,
        sample_rate: rate,
        channels,
        sample_frames: total,
        duration_us: total * 1_000_000 / u64::from(rate),
    };
    a.validate().map_err(|e| e.to_string())?;
    Ok(a)
}
