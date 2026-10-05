//! Bounded SPS metadata reader. Android extractors can omit explicit VUI colour tags.
//! This reads bitstream metadata only; MediaCodec still performs picture decoding.
use crate::Result;

#[derive(Debug, Default, PartialEq)]
pub struct AvcMetadata {
    pub color_standard: Option<u32>,
    pub color_range: Option<u32>,
}
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn read(&mut self, n: usize) -> Result<u32> {
        if n > 32 || self.at + n > self.data.len() * 8 {
            return Err("truncated SPS".into());
        }
        let mut out = 0;
        for _ in 0..n {
            out = out * 2 + u32::from((self.data[self.at / 8] >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Ok(out)
    }
    fn flag(&mut self) -> Result<bool> {
        Ok(self.read(1)? != 0)
    }
    fn ue(&mut self) -> Result<u32> {
        let mut n = 0;
        while self.read(1)? == 0 {
            n += 1;
            if n > 24 {
                return Err("SPS integer exceeds metadata budget".into());
            }
        }
        Ok((1 << n) - 1 + self.read(n)?)
    }
    fn se(&mut self) -> Result<i32> {
        let v = self.ue()?;
        Ok(if v % 2 == 0 {
            -(v as i32 / 2)
        } else {
            v.div_ceil(2) as i32
        })
    }
}

pub fn avc_metadata(csd: &[u8]) -> Result<AvcMetadata> {
    if csd.len() > 1024 * 1024 {
        return Err("H.264 codec configuration exceeds budget".into());
    }
    let start = csd
        .windows(4)
        .position(|v| v == [0, 0, 0, 1])
        .map(|n| n + 4)
        .or_else(|| csd.windows(3).position(|v| v == [0, 0, 1]).map(|n| n + 3))
        .ok_or("missing Annex B SPS")?;
    let nal = &csd[start..];
    if nal.len() < 4 || nal[0] & 31 != 7 || !matches!(nal[1], 66 | 77 | 100) {
        return Err("video requires 8-bit baseline/main/high H.264".into());
    }
    let end = nal[1..]
        .windows(3)
        .position(|v| v == [0, 0, 1])
        .map_or(nal.len(), |n| n + 1);
    let mut rbsp = Vec::with_capacity(end);
    let mut zeros = 0;
    for b in &nal[1..end] {
        if zeros >= 2 && *b == 3 {
            zeros = 0;
            continue;
        }
        rbsp.push(*b);
        zeros = if *b == 0 { zeros + 1 } else { 0 };
    }
    let mut bits = Bits { data: &rbsp, at: 0 };
    let profile = bits.read(8)?;
    bits.read(16)?;
    if bits.ue()? > 31 {
        return Err("invalid SPS id".into());
    }
    if profile == 100 {
        let chroma = bits.ue()?;
        if chroma != 1 || bits.ue()? != 0 || bits.ue()? != 0 {
            return Err("video requires 8-bit YUV420".into());
        }
        bits.flag()?;
        if bits.flag()? {
            for list in 0..8 {
                if bits.flag()? {
                    let mut current = 8;
                    let mut previous = 8;
                    for _ in 0..if list < 6 { 16 } else { 64 } {
                        if current != 0 {
                            current = (previous + bits.se()?).rem_euclid(256);
                        }
                        if current != 0 {
                            previous = current;
                        }
                    }
                }
            }
        }
    }
    if bits.ue()? > 12 {
        return Err("invalid SPS frame-number size".into());
    }
    match bits.ue()? {
        0 => {
            if bits.ue()? > 12 {
                return Err("invalid SPS picture-order size".into());
            }
        }
        1 => {
            bits.flag()?;
            bits.se()?;
            bits.se()?;
            let count = bits.ue()?;
            if count > 255 {
                return Err("SPS picture-order budget exceeded".into());
            }
            for _ in 0..count {
                bits.se()?;
            }
        }
        2 => {}
        _ => return Err("invalid SPS picture-order mode".into()),
    }
    bits.ue()?;
    bits.flag()?;
    bits.ue()?;
    bits.ue()?;
    if !bits.flag()? {
        bits.flag()?;
    } // frame_mbs_only_flag / mb_adaptive_frame_field_flag
    bits.flag()?;
    if bits.flag()? {
        for _ in 0..4 {
            bits.ue()?;
        }
    }
    let mut metadata = AvcMetadata::default();
    if !bits.flag()? {
        return Ok(metadata);
    }
    if bits.flag()? {
        match bits.read(8)? {
            0 | 1 => {}
            255 => {
                let w = bits.read(16)?;
                let h = bits.read(16)?;
                if w == 0 || w != h {
                    return Err("non-square video pixels unsupported".into());
                }
            }
            _ => return Err("non-square video pixels unsupported".into()),
        }
    }
    if bits.flag()? {
        bits.flag()?;
    }
    if bits.flag()? {
        bits.read(3)?;
        metadata.color_range = Some(if bits.flag()? { 1 } else { 2 });
        if bits.flag()? {
            let primaries = bits.read(8)?;
            let transfer = bits.read(8)?;
            let matrix = bits.read(8)?;
            if primaries == 9 || matches!(transfer, 16 | 18) || matches!(matrix, 9 | 10) {
                return Err("HDR/BT.2020 video unsupported".into());
            }
            metadata.color_standard = match matrix {
                1 => Some(1),
                5 => Some(2),
                6 => Some(4),
                2 => None,
                _ => return Err("unsupported video colour matrix".into()),
            };
        }
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sps() -> Vec<u8> {
        let mp4 = include_bytes!("../tests/fixtures/video/silent-24fps.mp4");
        let p = mp4.windows(4).position(|s| s == b"avcC").unwrap() + 4;
        let n = u16::from_be_bytes(mp4[p + 6..p + 8].try_into().unwrap()) as usize;
        let mut csd = vec![0, 0, 0, 1];
        csd.extend_from_slice(&mp4[p + 8..p + 8 + n]);
        csd
    }
    #[test]
    fn explicit_709_matrix_with_unspecified_primaries_is_preserved() {
        assert_eq!(
            avc_metadata(&sps()).unwrap(),
            AvcMetadata {
                color_standard: Some(1),
                color_range: Some(2)
            }
        );
    }
    #[test]
    fn truncated_metadata_never_becomes_a_colour_default() {
        let csd = sps();
        for n in 0..csd.len() / 2 {
            assert!(avc_metadata(&csd[..n]).is_err(), "length {n}");
        }
    }
    #[test]
    fn unsupported_profiles_and_oversized_config_are_rejected() {
        let mut csd = sps();
        csd[5] = 110;
        assert!(avc_metadata(&csd).is_err());
        assert!(avc_metadata(&vec![0; 1024 * 1024 + 1]).is_err());
    }
}
