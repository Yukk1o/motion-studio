use crate::Result;
use std::{fs::File, io::Read, path::Path};

pub const VIDEO_MIMES: &[&str] = &[
    "video/avc",
    "video/hevc",
    "video/x-vnd.on2.vp8",
    "video/x-vnd.on2.vp9",
];
pub const NATIVE_AUDIO_MIMES: &[&str] = &[
    "audio/mp4a-latm",
    "audio/opus",
    "audio/vorbis",
    "audio/flac",
    "audio/mpeg",
    "audio/raw",
    "audio/3gpp",
    "audio/amr-wb",
];

/// Container detection is independent of the provider name, URI and MIME hint.
/// Unknown platform-supported containers retain a neutral extension.
pub fn source_extension(path: &Path) -> Result<&'static str> {
    let mut h = [0u8; 12];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut h))
        .map_err(|e| e.to_string())?;
    Ok(if n >= 12 && &h[4..8] == b"ftyp" {
        if &h[8..12] == b"qt  " {
            "mov"
        } else if &h[8..11] == b"3gp" {
            "3gp"
        } else {
            "mp4"
        }
    } else if n >= 4 && h[..4] == [0x1a, 0x45, 0xdf, 0xa3] {
        "mkv"
    } else if n >= 4 && &h[..4] == b"OggS" {
        "ogg"
    } else if n >= 4 && &h[..4] == b"fLaC" {
        "flac"
    } else if n >= 12 && &h[..4] == b"RIFF" && &h[8..] == b"WAVE" {
        "wav"
    } else if n >= 12 && &h[..4] == b"FORM" && matches!(&h[8..], b"AIFF" | b"AIFC") {
        "aiff"
    } else {
        "media"
    })
}
