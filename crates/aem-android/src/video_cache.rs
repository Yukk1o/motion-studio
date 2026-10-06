//! Source-sized caches: small streams retain the existing 8 MiB budget.
//! Large streams reserve room for one RGBA frame, bounded at 36 MiB each.
//! Decoder/ImageReader buffers and an in-flight pack are counted separately.
use crate::video_frame::DecodedFrame;
use std::{collections::VecDeque, sync::Arc};
pub const CACHE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CACHE_BYTES: usize = 36 * 1024 * 1024;
pub const LOOKAHEAD: usize = 2;
pub struct FrameCache {
    frames: VecDeque<Arc<DecodedFrame>>,
    bytes: usize,
    budget: usize,
}
impl FrameCache {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            bytes: 0,
            budget: CACHE_BYTES,
        }
    }
    pub fn for_size(width: u32, height: u32) -> Self {
        let mut cache = Self::new();
        cache.budget = (u64::from(width) * u64::from(height))
            .saturating_mul(4)
            .clamp(CACHE_BYTES as u64, MAX_CACHE_BYTES as u64) as usize;
        cache
    }
    pub fn budget(&self) -> usize {
        self.budget
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn find(&self, time: u64) -> Option<Arc<DecodedFrame>> {
        self.frames
            .iter()
            .find(|f| f.pts <= time && time < f.end)
            .cloned()
    }
    pub fn retain_window(&mut self, first: u64, last: u64) {
        self.frames.retain(|f| f.pts >= first && f.pts <= last);
        self.bytes = self.frames.iter().map(|f| f.bytes()).sum();
    }
    pub fn can_prefetch(&self, expected: usize) -> bool {
        self.frames.len() < LOOKAHEAD + 1 && self.bytes + expected <= self.budget
    }
    pub fn insert(&mut self, frame: Arc<DecodedFrame>, required: u64) -> bool {
        if frame.bytes() > self.budget {
            return false;
        }
        if self.find(frame.pts).is_some() {
            return true;
        }
        while self.bytes + frame.bytes() > self.budget || self.frames.len() >= LOOKAHEAD + 1 {
            let Some(i) = self.frames.iter().position(|f| f.pts != required) else {
                return false;
            };
            self.bytes -= self.frames.remove(i).unwrap().bytes();
        }
        self.bytes += frame.bytes();
        self.frames.push_back(frame);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_frame::VideoPixels;
    fn frame(pts: u64, end: u64, bytes: usize) -> Arc<DecodedFrame> {
        Arc::new(DecodedFrame {
            pixels: VideoPixels::Rgba(vec![0; bytes]),
            pts,
            end,
            width: 1,
            height: 1,
            decode_us: 0,
            codec_us: 0,
            transfer_us: 0,
            pack_us: 0,
            source_transfer: "fixture",
            decoder_name: String::new(),
        })
    }
    #[test]
    fn vfr_intervals_are_exact_and_seek_invalidates_old_window() {
        let mut cache = FrameCache::new();
        cache.insert(frame(100, 117, 4), 100);
        cache.insert(frame(117, 200, 4), 100);
        assert_eq!(cache.find(116).unwrap().pts, 100);
        assert_eq!(cache.find(117).unwrap().pts, 117);
        assert!(cache.find(200).is_none());
        cache.retain_window(117, 300);
        assert!(cache.find(116).is_none());
        assert_eq!(cache.bytes(), 4);
        cache.retain_window(0, 99);
        assert_eq!(cache.len(), 0);
    }
    #[test]
    fn cache_keeps_required_frame_and_stays_bounded() {
        let mut cache = FrameCache::new();
        assert!(cache.insert(frame(0, 1, CACHE_BYTES / 2), 0));
        assert!(cache.insert(frame(1, 2, CACHE_BYTES / 2), 0));
        assert!(!cache.can_prefetch(1));
        assert!(cache.insert(frame(2, 3, CACHE_BYTES / 2), 0));
        assert!(cache.find(0).is_some());
        assert!(cache.find(1).is_none());
        assert_eq!(cache.bytes(), CACHE_BYTES);
        assert!(!cache.insert(frame(3, 4, CACHE_BYTES + 1), 0));
    }
    #[test]
    fn large_source_cache_keeps_full_4k_pixels_without_unbounded_prefetch() {
        let rgba = 4096 * 2160 * 4;
        let mut cache = FrameCache::for_size(4096, 2160);
        assert!(cache.insert(frame(0, 100, rgba), 0));
        assert!(cache.find(50).is_some());
        assert!(!cache.can_prefetch(rgba));
        assert!(cache.bytes() <= cache.budget());
        cache.retain_window(100, 300);
        assert!(cache.insert(frame(100, 200, rgba), 100));
        assert_eq!(cache.len(), 1);
        assert!(cache.budget() <= MAX_CACHE_BYTES);
        assert_eq!(FrameCache::for_size(256, 144).budget(), CACHE_BYTES);
    }
}
