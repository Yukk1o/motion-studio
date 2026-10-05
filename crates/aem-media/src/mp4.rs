//! Read the selected audio track's simple edit list. Symphonia 0.5 parses but
//! does not apply it. Preserve a leading empty edit and remove AAC priming.
use crate::Result;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Default)]
pub(crate) struct Edit {
    pub skip: u64,
    pub leading: u64,
    pub frames: Option<u64>,
}
fn u32be(b: &[u8]) -> Result<u32> {
    Ok(u32::from_be_bytes(
        b.get(..4).ok_or("short MP4 field")?.try_into().unwrap(),
    ))
}
fn u64be(b: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(
        b.get(..8).ok_or("short MP4 field")?.try_into().unwrap(),
    ))
}
fn children(mut b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        if out.len() >= 4096 || b.len() < 8 {
            return Err("invalid MP4 atom count/length".into());
        }
        let mut n = u64::from(u32be(b)?);
        let kind = b[4..8].try_into().unwrap();
        let mut header = 8;
        if n == 1 {
            n = u64be(&b[8..])?;
            header = 16;
        }
        if n == 0 {
            n = b.len() as u64;
        }
        let n = usize::try_from(n).map_err(|_| "MP4 atom length overflow")?;
        if n < header || n > b.len() {
            return Err("MP4 atom outside parent".into());
        }
        out.push((kind, &b[header..n]));
        b = &b[n..];
    }
    Ok(out)
}
fn find<'a>(b: &'a [u8], name: &[u8; 4]) -> Result<Option<&'a [u8]>> {
    Ok(children(b)?
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v))
}
pub(crate) fn audio_edit(path: &Path, track: u32, rate: u32) -> Result<Edit> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut at = 0;
    for _ in 0..4096 {
        if at == len {
            return Ok(Edit::default());
        }
        if len.saturating_sub(at) < 8 {
            return Err("truncated MP4 header".into());
        }
        f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        let mut h = [0; 16];
        f.read_exact(&mut h[..8]).map_err(|e| e.to_string())?;
        let mut n = u64::from(u32be(&h)?);
        let mut header = 8;
        if n == 1 {
            f.read_exact(&mut h[8..]).map_err(|e| e.to_string())?;
            n = u64be(&h[8..])?;
            header = 16;
        }
        if n == 0 {
            n = len - at;
        }
        if n < header || n > len - at {
            return Err("invalid MP4 atom size".into());
        }
        if &h[4..8] == b"moov" {
            if n - header > 16 * 1024 * 1024 {
                return Err("MP4 metadata exceeds 16 MiB budget".into());
            }
            let mut moov = vec![0; (n - header) as usize];
            f.read_exact(&mut moov).map_err(|e| e.to_string())?;
            let mvhd = find(&moov, b"mvhd")?.ok_or("missing movie header")?;
            let movie_rate = u32be(
                mvhd.get(if mvhd.first() == Some(&1) { 20.. } else { 12.. })
                    .ok_or("short movie header")?,
            )?;
            if movie_rate == 0 {
                return Err("zero MP4 timebase".into());
            }
            // Symphonia exposes zero-based track ordinals, not tkhd's track_ID.
            for (index, (_, trak)) in children(&moov)?
                .into_iter()
                .filter(|(k, _)| k == b"trak")
                .enumerate()
            {
                if index as u32 != track {
                    continue;
                }
                let mdia = find(trak, b"mdia")?.ok_or("missing media header")?;
                let mdhd = find(mdia, b"mdhd")?.ok_or("missing media timebase")?;
                let media_rate = u32be(
                    mdhd.get(if mdhd.first() == Some(&1) { 20.. } else { 12.. })
                        .ok_or("short media header")?,
                )?;
                if media_rate == 0 {
                    return Err("zero audio timebase".into());
                }
                let Some(edts) = find(trak, b"edts")? else {
                    return Ok(Edit::default());
                };
                let Some(elst) = find(edts, b"elst")? else {
                    return Ok(Edit::default());
                };
                if elst.len() < 8 || !matches!(elst[0], 0 | 1) {
                    return Err("unsupported MP4 edit version".into());
                }
                let count = u32be(&elst[4..])? as usize;
                let wide = elst[0] == 1;
                let stride = if wide { 20 } else { 12 };
                if count == 0 {
                    return Ok(Edit::default());
                }
                if count > 2 || elst.len() != 8 + count * stride {
                    return Err("complex MP4 audio edits are not supported".into());
                }
                let mut edit = Edit::default();
                let mut have_media = false;
                for entry in elst[8..].chunks_exact(stride) {
                    let (duration, media, r) = if wide {
                        (
                            u64be(entry)?,
                            i64::from_be_bytes(entry[8..16].try_into().unwrap()),
                            &entry[16..],
                        )
                    } else {
                        (
                            u64::from(u32be(entry)?),
                            i64::from(i32::from_be_bytes(entry[4..8].try_into().unwrap())),
                            &entry[8..],
                        )
                    };
                    if r != [0, 1, 0, 0] {
                        return Err("audio edit speed must equal one".into());
                    }
                    let frames = duration
                        .checked_mul(u64::from(rate))
                        .ok_or("edit duration overflow")?
                        / u64::from(movie_rate);
                    if media == -1 && !have_media {
                        edit.leading = edit
                            .leading
                            .checked_add(frames)
                            .ok_or("edit duration overflow")?;
                    } else if media >= 0 && !have_media {
                        edit.skip = (media as u64)
                            .checked_mul(u64::from(rate))
                            .ok_or("edit start overflow")?
                            / u64::from(media_rate);
                        edit.frames = Some(frames);
                        have_media = true;
                    } else {
                        return Err("complex MP4 audio edits are not supported".into());
                    }
                }
                if !have_media {
                    return Err("audio edit has no media".into());
                }
                return Ok(edit);
            }
            return Err("selected MP4 track missing".into());
        }
        at += n;
    }
    Err("MP4 atom budget exceeded".into())
}
