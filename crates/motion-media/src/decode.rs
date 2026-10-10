use crate::{mp4, Result};
use motion_model::AudioAsset;
use std::{
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3},
    formats::FormatOptions,
    io::MediaSourceStream,
    probe::Hint,
};

/// PCM cache is little-endian interleaved f32 at the native source rate.
/// Decode only a packet at a time; the complete PCM lives on disk.
pub type DecodeAudio = std::sync::Arc<
    dyn Fn(&Path, &Path, Option<u32>, u64, &mut dyn FnMut(f64) -> Result<()>) -> Result<AudioAsset>
        + Send
        + Sync,
>;

pub fn decode_audio(
    path: &Path,
    pcm: &Path,
    selected_track: Option<u32>,
    max_pcm_bytes: u64,
    check: &mut dyn FnMut(f64) -> Result<()>,
) -> Result<AudioAsset> {
    let mut signature = [0; 12];
    let mut source = File::open(path).map_err(|e| e.to_string())?;
    source
        .read_exact(&mut signature)
        .map_err(|e| e.to_string())?;
    let bytes = source.metadata().map_err(|e| e.to_string())?.len();
    drop(source);
    if signature[..4] == [0x1a, 0x45, 0xdf, 0xa3] {
        return Err(
            "Matroska audio requires the platform decoder to preserve its source clock".into(),
        );
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let mut format = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &opts, &Default::default())
        .map_err(|e| e.to_string())?
        .format;
    if format.tracks().len() > 32 {
        return Err("too many audio tracks".into());
    }
    let track = format
        .tracks()
        .iter()
        .find(|t| {
            selected_track.map_or(
                symphonia::default::get_codecs()
                    .get_codec(t.codec_params.codec)
                    .is_some(),
                |id| t.id == id,
            )
        })
        .ok_or("no supported selected audio track")?;
    let id = track.id;
    let params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions { verify: true })
        .map_err(|e| e.to_string())?;
    // AAC's AudioSpecificConfig carries the layout even when MP4 headers do not.
    let rate = if matches!(params.codec, CODEC_TYPE_AAC | CODEC_TYPE_ALAC) {
        decoder.last_decoded().spec().rate
    } else {
        params.sample_rate.ok_or("missing audio sample rate")?
    };
    let channels = if matches!(params.codec, CODEC_TYPE_AAC | CODEC_TYPE_ALAC) {
        decoder.last_decoded().spec().channels.count() as u32
    } else {
        params.channels.ok_or("missing channel layout")?.count() as u32
    };
    if !(8_000..=192_000).contains(&rate) || !matches!(channels, 1 | 2) {
        return Err("audio requires mono/stereo at 8–192 kHz".into());
    }
    let mp4_container = &signature[4..8] == b"ftyp";
    let mime = if mp4_container {
        "audio/mp4"
    } else if &signature[..4] == b"fLaC" {
        "audio/flac"
    } else if &signature[..4] == b"OggS" {
        "audio/ogg"
    } else if &signature[..4] == b"RIFF" && &signature[8..] == b"WAVE" {
        "audio/wav"
    } else if &signature[..4] == b"FORM" {
        "audio/aiff"
    } else if params.codec == CODEC_TYPE_FLAC {
        "audio/flac"
    } else if params.codec == CODEC_TYPE_MP3 {
        "audio/mpeg"
    } else {
        return Err("unsupported audio container".into());
    };
    let edit = if mp4_container {
        mp4::audio_edit(path, id, rate)?
    } else {
        mp4::Edit::default()
    };
    // Symphonia 0.5.5 includes AIFF's eight-byte SSND prefix in its
    // estimated duration. COMM is the authoritative number of sample frames.
    let expected_frames = if mime == "audio/aiff" {
        Some(aiff_frames(path)?)
    } else {
        params.n_frames
    };
    let max_frames = max_pcm_bytes / (u64::from(channels) * 4);
    if edit.leading > max_frames
        || (if mime == "audio/mp4" {
            edit.frames.map(|n| n.saturating_add(edit.leading))
        } else {
            expected_frames
        })
        .is_some_and(|n| n > max_frames)
    {
        return Err("decoded audio cache exceeds limit".into());
    }
    if edit.leading > u64::from(rate) * 3600 {
        return Err("audio leading gap exceeds one hour".into());
    }
    let mut output =
        BufWriter::with_capacity(64 * 1024, File::create(pcm).map_err(|e| e.to_string())?);
    let mut total = 0u64;
    let mut decoded = 0u64;
    let mut audible = 0u64;
    let zeros = [0; 8192];
    let mut lead_bytes = edit.leading * u64::from(channels) * 4;
    while lead_bytes > 0 {
        check(0.0)?;
        let n = lead_bytes.min(zeros.len() as u64);
        output
            .write_all(&zeros[..n as usize])
            .map_err(|e| e.to_string())?;
        lead_bytes -= n;
    }
    total += edit.leading;
    loop {
        check(expected_frames.map_or(0.0, |n| (decoded as f64 / n.max(1) as f64).min(1.0)))?;
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => return Err(format!("audio demux failed: {e}")),
        };
        if packet.track_id() != id {
            continue;
        }
        let audio = decoder
            .decode(&packet)
            .map_err(|e| format!("audio decode failed: {e}"))?;
        if audio.spec().rate != rate || audio.spec().channels.count() as u32 != channels {
            return Err("audio format changes within source".into());
        }
        if audio.frames() > 65_536 {
            return Err("audio packet exceeds decode memory budget".into());
        }
        // Vorbis's initial overlap packet legitimately emits zero samples.
        if audio.frames() == 0 {
            continue;
        }
        let mut buffer = SampleBuffer::<f32>::new(audio.frames() as u64, *audio.spec());
        buffer.copy_interleaved_ref(audio);
        let frames = (buffer.len() / channels as usize) as u64;
        let skip = edit.skip.saturating_sub(decoded).min(frames);
        let count =
            (frames - skip).min(edit.frames.map_or(u64::MAX, |n| n.saturating_sub(audible)));
        if total + count > max_frames || total + count > u64::from(rate) * 3600 {
            return Err("decoded audio duration/cache budget exceeded".into());
        }
        let from = (skip * u64::from(channels)) as usize;
        let to = from + (count * u64::from(channels)) as usize;
        for &s in &buffer.samples()[from..to] {
            if !s.is_finite() {
                return Err("audio decoder returned a non-finite sample".into());
            }
            output
                .write_all(&s.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        decoded += frames;
        audible += count;
        total += count;
        if total > u64::from(rate) * 3600 {
            return Err("audio duration exceeds one hour".into());
        }
    }
    if audible == 0 || edit.frames.is_some_and(|n| audible < n) {
        return Err("audio is empty or truncated".into());
    }
    // Lossless sources have exact declared sample counts; Ogg packet padding
    // and Matroska duration estimates need not equal the decoded count.
    if matches!(
        mime,
        "audio/wav" | "audio/flac" | "audio/aiff" | "audio/mpeg"
    ) && expected_frames.is_some_and(|n| audible != n)
    {
        return Err(format!("audio decoded length {audible} does not match source {expected_frames:?}; file may be truncated"));
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err("audio decoder verification failed".into());
    }
    output.flush().map_err(|e| e.to_string())?;
    output.get_ref().sync_all().map_err(|e| e.to_string())?;
    Ok(AudioAsset {
        id: 1,
        path: "assets/probed-audio".into(),
        mime: mime.into(),
        bytes,
        track: id,
        sample_rate: rate,
        channels,
        sample_frames: total,
        duration_us: total * 1_000_000 / u64::from(rate),
    })
}

fn aiff_frames(path: &Path) -> Result<u64> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    f.seek(SeekFrom::Start(12)).map_err(|e| e.to_string())?;
    for _ in 0..4096 {
        let at = f.stream_position().map_err(|e| e.to_string())?;
        if at + 8 > size {
            break;
        }
        let mut h = [0u8; 8];
        f.read_exact(&mut h).map_err(|e| e.to_string())?;
        let len = u64::from(u32::from_be_bytes(h[4..].try_into().unwrap()));
        if at + 8 + len > size {
            return Err("truncated AIFF chunk".into());
        }
        if &h[..4] == b"COMM" && len >= 18 {
            let mut data = [0u8; 6];
            f.read_exact(&mut data).map_err(|e| e.to_string())?;
            return Ok(u64::from(u32::from_be_bytes(data[2..].try_into().unwrap())));
        }
        f.seek(SeekFrom::Start(at + 8 + len + len % 2))
            .map_err(|e| e.to_string())?;
    }
    Err("AIFF sample count missing or chunk budget exceeded".into())
}
