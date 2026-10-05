//! Bounded container metadata. The platform still demuxes compressed packets.
//! Android may omit AAC CodecDelay and return extended colour-standard values.
use crate::Result;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Default)]
pub struct MatroskaTrack {
    pub number: u64,
    pub codec_delay_ns: u64,
    pub matrix: Option<u32>,
    pub primaries: Option<u32>,
    pub transfer: Option<u32>,
    pub range: Option<u32>,
    pub bit_depth: Option<u32>,
}
pub struct MatroskaMetadata {
    pub time_scale_ns: u64,
    pub tracks: Vec<MatroskaTrack>,
}

fn vint(read: &mut impl Read, id: bool) -> Result<(u64, usize, bool)> {
    let mut first = [0u8; 1];
    read.read_exact(&mut first).map_err(|e| e.to_string())?;
    let n = first[0].leading_zeros() as usize + 1;
    if n > if id { 4 } else { 8 } {
        return Err("invalid EBML integer".into());
    }
    let mut value = u64::from(if id {
        first[0]
    } else {
        first[0] & ((0xffu16 >> n) as u8)
    });
    for _ in 1..n {
        let mut b = [0u8; 1];
        read.read_exact(&mut b).map_err(|e| e.to_string())?;
        value = (value << 8) | u64::from(b[0]);
    }
    Ok((value, n, !id && value == (1u64 << (7 * n)) - 1))
}
fn children(mut bytes: &[u8]) -> Result<Vec<(u64, &[u8])>> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        if out.len() >= 4096 {
            return Err("Matroska element budget exceeded".into());
        }
        let (id, _, _) = vint(&mut bytes, true)?;
        let (len, _, unknown) = vint(&mut bytes, false)?;
        if unknown || len > bytes.len() as u64 {
            return Err("invalid Matroska metadata size".into());
        }
        let (payload, remaining) = bytes.split_at(len as usize);
        out.push((id, payload));
        bytes = remaining;
    }
    Ok(out)
}
fn uint(data: &[u8]) -> Result<u64> {
    if data.is_empty() || data.len() > 8 {
        return Err("invalid Matroska unsigned value".into());
    }
    Ok(data.iter().fold(0, |v, b| (v << 8) | u64::from(*b)))
}
fn tracks(bytes: &[u8]) -> Result<Vec<MatroskaTrack>> {
    let mut out = Vec::new();
    for (id, entry) in children(bytes)? {
        if id != 0xae {
            continue;
        }
        if out.len() >= 32 {
            return Err("Matroska track budget exceeded".into());
        }
        let mut t = MatroskaTrack::default();
        for (id, value) in children(entry)? {
            match id {
                0xd7 => t.number = uint(value)?,
                0x56aa => t.codec_delay_ns = uint(value)?,
                0xe0 => {
                    for (id, video) in children(value)? {
                        if id != 0x55b0 {
                            continue;
                        }
                        for (id, value) in children(video)? {
                            if !matches!(id, 0x55b1 | 0x55b2 | 0x55b9 | 0x55ba | 0x55bb) {
                                continue;
                            }
                            let v = u32::try_from(uint(value)?)
                                .map_err(|_| "Matroska colour integer exceeds budget")?;
                            match id {
                                0x55b1 => t.matrix = Some(v),
                                0x55b2 => t.bit_depth = Some(v),
                                0x55b9 => t.range = Some(v),
                                0x55ba => t.transfer = Some(v),
                                0x55bb => t.primaries = Some(v),
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if t.number == 0 || t.codec_delay_ns > 3_600_000_000_000 {
            return Err("invalid Matroska track metadata".into());
        }
        out.push(t);
    }
    Ok(out)
}

pub fn matroska_metadata(path: &Path) -> Result<MatroskaMetadata> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    let mut end = size;
    let mut parsed_tracks = None;
    let mut scale = 1_000_000;
    let mut info = false;
    let mut in_segment = false;
    for _ in 0..4096 {
        if f.stream_position().map_err(|e| e.to_string())? >= end {
            break;
        }
        let (id, _, _) = vint(&mut f, true)?;
        let (len, _, unknown) = vint(&mut f, false)?;
        let start = f.stream_position().map_err(|e| e.to_string())?;
        let stop = if unknown {
            end
        } else {
            start
                .checked_add(len)
                .filter(|p| *p <= end)
                .ok_or("Matroska element outside source")?
        };
        if id == 0x18538067 {
            in_segment = true;
            end = stop;
            continue;
        }
        if in_segment && matches!(id, 0x1549a966 | 0x1654ae6b) {
            if unknown || len > 1024 * 1024 {
                return Err("Matroska metadata exceeds budget".into());
            }
            let mut bytes = vec![0u8; len as usize];
            f.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            if id == 0x1654ae6b {
                parsed_tracks = Some(tracks(&bytes)?);
            } else {
                for (id, data) in children(&bytes)? {
                    if id == 0x2ad7b1 {
                        scale = uint(data)?;
                    }
                }
                info = true;
            }
            if info && parsed_tracks.is_some() {
                break;
            }
        }
        if unknown {
            break;
        }
        f.seek(SeekFrom::Start(stop)).map_err(|e| e.to_string())?;
    }
    if scale == 0 || scale > 1_000_000_000 {
        return Err("unsupported Matroska timestamp precision".into());
    }
    Ok(MatroskaMetadata {
        time_scale_ns: scale,
        tracks: parsed_tracks.ok_or("Matroska tracks missing")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_container_delay_and_matrix_are_read_without_decoding_media() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formats");
        let m = matroska_metadata(&root.join("avc.mkv")).unwrap();
        assert_eq!(m.tracks.len(), 2);
        assert_eq!(m.time_scale_ns, 1_000_000);
        assert_eq!(m.tracks[1].codec_delay_ns, 21_333_333);
        let m = matroska_metadata(&root.join("vp8.webm")).unwrap();
        assert_eq!(m.tracks[0].matrix, Some(1));
    }
}
