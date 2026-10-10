//! Bounded metadata validation for the existing 8-bit SDR render pipeline.
use crate::{avc::Bits, AvcMetadata, Result};

pub fn hevc_metadata(csd: &[u8]) -> Result<AvcMetadata> {
    if csd.len() > 1024 * 1024 {
        return Err("HEVC configuration exceeds budget".into());
    }
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < csd.len() {
        let prefix = if csd[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if csd[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        starts.push((i, i + prefix));
        i += prefix;
    }
    let (index, &(_, start)) = starts
        .iter()
        .enumerate()
        .find(|(_, (_, p))| csd.get(*p).is_some_and(|b| (b >> 1) & 63 == 33))
        .ok_or("HEVC SPS missing")?;
    let end = starts.get(index + 1).map_or(csd.len(), |p| p.0);
    if end < start + 3 {
        return Err("truncated HEVC SPS".into());
    }
    let mut rbsp = Vec::new();
    let mut zeros = 0;
    for &b in &csd[start + 2..end] {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        rbsp.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    let mut b = Bits { data: &rbsp, at: 0 };
    b.read(4)?;
    let sub = b.read(3)? as usize;
    b.flag()?;
    if sub > 6 {
        return Err("invalid HEVC temporal layers".into());
    }
    b.read(3)?;
    let profile = b.read(5)?;
    if profile != 1 {
        return Err("HEVC requires Main profile, 8-bit 4:2:0 SDR".into());
    }
    for _ in 0..2 {
        b.read(32)?;
    }
    b.read(16)?;
    b.read(8)?;
    let mut flags = Vec::new();
    for _ in 0..sub {
        flags.push((b.flag()?, b.flag()?));
    }
    if sub > 0 {
        for _ in sub..8 {
            b.read(2)?;
        }
    }
    for (p, l) in flags {
        if p {
            for _ in 0..2 {
                b.read(32)?;
            }
            b.read(24)?;
        }
        if l {
            b.read(8)?;
        }
    }
    if b.ue()? > 15 || b.ue()? != 1 {
        return Err("unsupported HEVC SPS/chroma format".into());
    }
    for _ in 0..2 {
        if b.ue()? > 8192 {
            return Err("HEVC dimension exceeds budget".into());
        }
    }
    if b.flag()? {
        for _ in 0..4 {
            b.ue()?;
        }
    }
    if b.ue()? != 0 || b.ue()? != 0 {
        return Err("HEVC requires 8-bit samples".into());
    }
    let poc = b.ue()? + 4;
    if poc > 16 {
        return Err("invalid HEVC POC size".into());
    }
    let first = if b.flag()? { 0 } else { sub };
    for _ in first..=sub {
        for _ in 0..3 {
            b.ue()?;
        }
    }
    for _ in 0..6 {
        b.ue()?;
    }
    if b.flag()? && b.flag()? {
        for size in 0..4 {
            for _ in (0..6).step_by(if size == 3 { 3 } else { 1 }) {
                if !b.flag()? {
                    b.ue()?;
                } else {
                    if size > 1 {
                        b.se()?;
                    }
                    for _ in 0..(if size == 0 { 16 } else { 64 }) {
                        b.se()?;
                    }
                }
            }
        }
    }
    b.flag()?;
    b.flag()?;
    if b.flag()? {
        b.read(8)?;
        b.ue()?;
        b.ue()?;
        b.flag()?;
    }
    let count = b.ue()?;
    if count > 64 {
        return Err("HEVC reference set budget exceeded".into());
    }
    let mut previous: Vec<i32> = Vec::new();
    for r in 0..count {
        let mut deltas = Vec::new();
        if r > 0 && b.flag()? {
            let negative = b.flag()?;
            let delta = b.ue()?;
            if delta > 32767 {
                return Err("HEVC reference delta exceeds budget".into());
            }
            let delta = (delta as i32 + 1) * if negative { -1 } else { 1 };
            for d in previous.iter().copied().chain(std::iter::once(0)) {
                let used = b.flag()?;
                let keep = used || b.flag()?;
                if keep && d + delta != 0 {
                    deltas.push(d + delta);
                }
            }
        } else {
            let neg = b.ue()?;
            let pos = b.ue()?;
            if neg + pos > 64 {
                return Err("HEVC reference budget exceeded".into());
            }
            for (n, sign) in [(neg, -1), (pos, 1)] {
                let mut d = 0i32;
                for _ in 0..n {
                    let v = b.ue()?;
                    if v > 32767 {
                        return Err("HEVC delta exceeds budget".into());
                    }
                    d += sign * (v as i32 + 1);
                    b.flag()?;
                    deltas.push(d);
                }
            }
        }
        if deltas.len() > 64 {
            return Err("HEVC reference budget exceeded".into());
        }
        deltas.sort_by_key(|v| if *v < 0 { (0, -*v) } else { (1, *v) });
        previous = deltas;
    }
    if b.flag()? {
        let n = b.ue()?;
        if n > 32 {
            return Err("HEVC long term budget exceeded".into());
        }
        for _ in 0..n {
            b.read(poc as usize)?;
            b.flag()?;
        }
    }
    b.flag()?;
    b.flag()?;
    if !b.flag()? {
        return Ok(AvcMetadata::default());
    }
    let mut out = AvcMetadata::default();
    if b.flag()? {
        match b.read(8)? {
            0 | 1 => {}
            255 => {
                let w = b.read(16)?;
                let h = b.read(16)?;
                if w == 0 || w != h {
                    return Err("non-square video pixels unsupported".into());
                }
            }
            _ => return Err("non-square video pixels unsupported".into()),
        }
    }
    if b.flag()? {
        b.flag()?;
    }
    if b.flag()? {
        b.read(3)?;
        out.color_range = Some(if b.flag()? { 1 } else { 2 });
        if b.flag()? {
            let primaries = b.read(8)?;
            let transfer = b.read(8)?;
            let matrix = b.read(8)?;
            if primaries == 9 || matches!(transfer, 16 | 18) || matches!(matrix, 9 | 10) {
                return Err("HDR/BT.2020 video unsupported".into());
            }
            out.color_standard = match matrix {
                1 => Some(1),
                5 => Some(2),
                6 => Some(4),
                2 => None,
                _ => return Err("unsupported HEVC colour matrix".into()),
            };
        }
    }
    Ok(out)
}

pub fn vp9_metadata(packet: &[u8]) -> Result<AvcMetadata> {
    // Uncompressed header uses MSB-first bits; profile zero fixes bit depth/chroma.
    let mut b = Bits {
        data: packet,
        at: 0,
    };
    if b.read(2)? != 2 {
        return Err("invalid VP9 frame marker".into());
    }
    let low = b.read(1)?;
    let high = b.read(1)?;
    if low + 2 * high != 0 {
        return Err("VP9 requires profile 0, 8-bit 4:2:0".into());
    }
    if b.flag()? || b.flag()? {
        return Err("VP9 source must start with a keyframe".into());
    }
    b.flag()?;
    b.flag()?;
    if b.read(24)? != 0x498342 {
        return Err("invalid VP9 keyframe sync".into());
    }
    let space = b.read(3)?;
    let range = if b.flag()? { 1 } else { 2 };
    let standard = match space {
        0 => None,
        1 | 3 => Some(4),
        2 => Some(1),
        4 => Some(2),
        _ => return Err("unsupported VP9 colour space".into()),
    };
    Ok(AvcMetadata {
        color_standard: standard,
        color_range: Some(range),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(mp4: &[u8]) -> Vec<u8> {
        let p = mp4.windows(4).position(|s| s == b"hvcC").unwrap() + 4;
        let arrays = mp4[p + 22];
        let mut at = p + 23;
        let mut csd = Vec::new();
        for _ in 0..arrays {
            at += 1;
            let count = u16::from_be_bytes(mp4[at..at + 2].try_into().unwrap());
            at += 2;
            for _ in 0..count {
                let n = u16::from_be_bytes(mp4[at..at + 2].try_into().unwrap()) as usize;
                at += 2;
                csd.extend_from_slice(&[0, 0, 0, 1]);
                csd.extend_from_slice(&mp4[at..at + n]);
                at += n;
            }
        }
        csd
    }
    #[test]
    fn actual_hevc_sps_preserves_colour_and_rejects_10_bit() {
        let csd = config(include_bytes!("../tests/fixtures/formats/hevc.mp4"));
        assert_eq!(
            hevc_metadata(&csd).unwrap(),
            AvcMetadata {
                color_standard: Some(1),
                color_range: Some(2)
            }
        );
        let ten = config(include_bytes!(
            "../tests/fixtures/formats/reject-hevc10.mp4"
        ));
        assert!(hevc_metadata(&ten).unwrap_err().contains("8-bit"));
        for n in 0..csd.len().min(40) {
            assert!(hevc_metadata(&csd[..n]).is_err());
        }
    }
}
