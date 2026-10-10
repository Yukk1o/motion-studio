use motion_render::{Renderer, Yuv420Frame};
use std::borrow::Cow;

pub enum VideoPixels {
    Yuv(Yuv420Frame),
    Rgba(Vec<u8>),
}
pub struct DecodedFrame {
    pub pixels: VideoPixels,
    pub pts: u64,
    pub end: u64,
    pub width: u32,
    pub height: u32,
    pub decode_us: u64,
    pub codec_us: u64,
    pub transfer_us: u64,
    pub pack_us: u64,
    pub source_transfer: &'static str,
    pub decoder_name: String,
}
impl DecodedFrame {
    pub fn bytes(&self) -> usize {
        match &self.pixels {
            VideoPixels::Yuv(f) => f.bytes(),
            VideoPixels::Rgba(p) => p.len(),
        }
    }
    pub fn rgba(&self) -> Result<Cow<'_, [u8]>, String> {
        match &self.pixels {
            VideoPixels::Yuv(f) => f.to_rgba().map(Cow::Owned).map_err(|e| e.to_string()),
            VideoPixels::Rgba(p) => Ok(Cow::Borrowed(p)),
        }
    }
    pub fn upload(&self, renderer: &mut Renderer, object: u64, source: u64) -> Result<(), String> {
        match &self.pixels {
            VideoPixels::Yuv(f) => renderer.upload_video_yuv(object, source, self.pts, f),
            VideoPixels::Rgba(p) => {
                renderer.upload_video_frame(object, source, self.pts, self.width, self.height, p)
            }
        }
        .map_err(|e| e.to_string())
    }
}
