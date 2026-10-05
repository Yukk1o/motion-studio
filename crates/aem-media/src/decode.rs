use crate::{mp4, Result};
use aem_core::AudioAsset;
use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    path::Path,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_MP3, CODEC_TYPE_PCM_S16LE},
    formats::FormatOptions,
    io::MediaSourceStream,
    probe::Hint,
};

/// PCM cache is little-endian interleaved f32 at the native source rate.
/// Decode only a packet at a time; the complete PCM lives on disk.
pub(crate) fn decode(
    path: &Path,
    pcm: &Path,
    selected_track: Option<u32>,
    max_pcm_bytes: u64,
    mut check: impl FnMut(f64) -> Result<()>,
) -> Result<AudioAsset> {
    let mut signature = [0; 12];
    let mut source = File::open(path).map_err(|e| e.to_string())?;
    source
        .read_exact(&mut signature)
        .map_err(|e| e.to_string())?;
    let bytes = source.metadata().map_err(|e| e.to_string())?.len();
    drop(source);
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
                matches!(
                    t.codec_params.codec,
                    CODEC_TYPE_AAC | CODEC_TYPE_MP3 | CODEC_TYPE_PCM_S16LE
                ),
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
    let rate = if params.codec == CODEC_TYPE_AAC {
        decoder.last_decoded().spec().rate
    } else {
        params.sample_rate.ok_or("missing audio sample rate")?
    };
    let channels = if params.codec == CODEC_TYPE_AAC {
        decoder.last_decoded().spec().channels.count() as u32
    } else {
        params.channels.ok_or("missing channel layout")?.count() as u32
    };
    if !matches!(rate, 44_100 | 48_000) || !matches!(channels, 1 | 2) {
        return Err("audio requires mono/stereo at 44.1/48 kHz".into());
    }
    let mime = match params.codec {
        CODEC_TYPE_AAC if &signature[4..8] == b"ftyp" => "audio/mp4",
        CODEC_TYPE_MP3 => "audio/mpeg",
        CODEC_TYPE_PCM_S16LE if &signature[..4] == b"RIFF" && &signature[8..] == b"WAVE" => {
            "audio/wav"
        }
        _ => return Err("supported audio formats: M4A/AAC-LC, MP3, WAV/16-bit PCM".into()),
    };
    let edit = if mime == "audio/mp4" {
        mp4::audio_edit(path, id, rate)?
    } else {
        mp4::Edit::default()
    };
    let max_frames = max_pcm_bytes / (u64::from(channels) * 4);
    if edit.leading > max_frames
        || (if mime == "audio/mp4" {
            edit.frames.map(|n| n.saturating_add(edit.leading))
        } else {
            params.n_frames
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
        check(
            params
                .n_frames
                .map_or(0.0, |n| (decoded as f64 / n.max(1) as f64).min(1.0)),
        )?;
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
    if mime != "audio/mp4" && params.n_frames.is_some_and(|n| audible != n) {
        return Err("audio decoded length does not match source; file may be truncated".into());
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
