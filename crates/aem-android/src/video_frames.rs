//! Exact PTS lookup with bounded forward prefetch, independent of render ticks.
use super::video_decode::Decoder;
use crate::video_cache::{FrameCache, LOOKAHEAD};
use crate::video_frame::DecodedFrame;
use aem_core::{Content, Project, Scene, VideoAsset};
use aem_media::Result;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};
#[derive(Clone, Copy)]
struct Request {
    time: u64,
    source_time: u64,
    index: usize,
    sequence: u64,
}
struct State {
    desired: Option<Request>,
    cache: FrameCache,
    error: Option<String>,
    stop: bool,
    prefetch: bool,
    failed_prefetch: Option<u64>,
    hits: u64,
    misses: u64,
    decoded: u64,
    cancelled: u64,
}
struct Stream {
    asset: VideoAsset,
    pts: Arc<Vec<u64>>,
    shared: Arc<(Mutex<State>, Condvar)>,
}
fn job(s: &State, pts: &[u64]) -> Option<(u64, bool)> {
    let r = s.desired?;
    if s.cache.find(r.time).is_none() {
        return s.error.is_none().then_some((r.time, false));
    }
    if !s.prefetch || s.failed_prefetch.is_some() {
        return None;
    }
    let expected = s.cache.find(r.time)?.bytes();
    if !s.cache.can_prefetch(expected) {
        return None;
    }
    pts.iter()
        .skip(r.index + 1)
        .take(LOOKAHEAD)
        .find(|t| s.failed_prefetch != Some(**t) && s.cache.find(**t).is_none())
        .map(|t| (*t, true))
}
impl Stream {
    fn new(root: PathBuf, asset: VideoAsset) -> Result<Self> {
        let pts = Arc::new(aem_media::load_video_index(&root, &asset)?);
        let shared = Arc::new((
            Mutex::new(State {
                desired: None,
                cache: FrameCache::new(),
                error: None,
                stop: false,
                prefetch: false,
                failed_prefetch: None,
                hits: 0,
                misses: 0,
                decoded: 0,
                cancelled: 0,
            }),
            Condvar::new(),
        ));
        let copy = shared.clone();
        let times = pts.clone();
        let media = asset.clone();
        std::thread::Builder::new()
            .name("motion-video-frames".into())
            .spawn(move || {
                let mut decoder = None;
                loop {
                    let (target, speculative) = {
                        let (lock, cv) = &*copy;
                        let s = lock.lock().unwrap_or_else(|e| e.into_inner());
                        let s = cv
                            .wait_while(s, |s| !s.stop && job(s, &times).is_none())
                            .unwrap_or_else(|e| e.into_inner());
                        if s.stop {
                            return;
                        }
                        job(&s, &times).unwrap()
                    };
                    let check = || {
                        let s = copy.0.lock().map_err(|_| "video frame state poisoned")?;
                        let valid = s.desired.is_some_and(|r| {
                            target == r.time
                                || (s.prefetch
                                    && target > r.time
                                    && target <= times[(r.index + LOOKAHEAD).min(times.len() - 1)])
                        });
                        if s.stop || !valid {
                            Err("video target superseded".into())
                        } else {
                            Ok(())
                        }
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || -> Result<Arc<DecodedFrame>> {
                            check()?;
                            if decoder.is_none() {
                                let base = root.canonicalize().map_err(|e| e.to_string())?;
                                let source = root
                                    .join(&media.path)
                                    .canonicalize()
                                    .map_err(|e| e.to_string())?;
                                if !source.starts_with(base)
                                    || source.metadata().map_err(|e| e.to_string())?.len()
                                        != media.bytes
                                {
                                    return Err("owned video source missing or changed".into());
                                }
                                decoder = Some(Decoder::new(
                                    &source,
                                    media.clone(),
                                    times.as_ref().clone(),
                                )?);
                            }
                            Ok(Arc::new(decoder.as_mut().unwrap().frame(target, &check)?))
                        },
                    ))
                    .unwrap_or_else(|_| Err("video decoder worker failed".into()));
                    let mut s = copy.0.lock().unwrap_or_else(|e| e.into_inner());
                    if s.stop {
                        return;
                    }
                    let desired = s.desired.unwrap();
                    let last = times[(desired.index + LOOKAHEAD).min(times.len() - 1)];
                    match result {
                        Ok(frame) => {
                            s.decoded += 1;
                            if frame.pts >= desired.time && frame.pts <= last {
                                if !s.cache.insert(frame, desired.time) {
                                    if target == desired.time {
                                        s.error = Some(
                                            "video source frame exceeds 8 MiB stream cache".into(),
                                        );
                                    } else {
                                        s.failed_prefetch = Some(target);
                                    }
                                }
                            } else {
                                s.cancelled += 1;
                            }
                        }
                        Err(e) if e == "video target superseded" => {
                            s.cancelled += 1;
                        }
                        Err(e) => {
                            decoder = None;
                            if target == desired.time {
                                s.error = Some(e);
                            } else if speculative {
                                s.failed_prefetch = Some(target);
                            }
                        }
                    }
                    // Do not await another VSync: fill the next available cache slot.
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { asset, pts, shared })
    }
    fn key(&self, time: u64) -> Result<(u64, usize)> {
        if time < self.asset.video_start_us || time >= self.asset.video_end_us {
            return Err("source time outside visible video".into());
        }
        let index = self
            .pts
            .partition_point(|p| *p <= time)
            .checked_sub(1)
            .ok_or("video PTS index missing")?;
        Ok((self.pts[index], index))
    }
    fn request(&self, time: u64, sequence: u64, prefetch: bool) -> Result<Value> {
        if sequence == 0 || sequence > i64::MAX as u64 {
            return Err("video sequence must be a positive signed 64-bit value".into());
        }
        let source_time = time;
        let (time, index) = self.key(time)?;
        let mut s = self
            .shared
            .0
            .lock()
            .map_err(|_| "video frame state poisoned")?;
        if let Some(old) = s.desired {
            if sequence < old.sequence {
                return Err("stale video request sequence".into());
            }
            if sequence == old.sequence && time != old.time {
                return Err("video sequence already used for another target".into());
            }
        }
        if s.desired.is_none_or(|r| r.sequence != sequence) {
            s.desired = Some(Request {
                time,
                source_time,
                index,
                sequence,
            });
            s.error = None;
            s.failed_prefetch = None;
            s.cache
                .retain_window(time, self.pts[(index + LOOKAHEAD).min(self.pts.len() - 1)]);
            if s.cache.find(time).is_some() {
                s.hits += 1;
            } else {
                s.misses += 1;
            }
        }
        if let Some(r) = &mut s.desired {
            r.source_time = source_time;
        }
        s.prefetch = prefetch;
        self.shared.1.notify_one();
        Ok(status(&s))
    }
    fn ready(&self, sequence: u64) -> Result<Arc<DecodedFrame>> {
        let s = self
            .shared
            .0
            .lock()
            .map_err(|_| "video frame state poisoned")?;
        let r = s
            .desired
            .filter(|r| r.sequence == sequence)
            .ok_or("video frame superseded")?;
        if let Some(e) = &s.error {
            return Err(e.clone());
        }
        s.cache.find(r.time).ok_or("video frame pending".into())
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let mut s = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        s.stop = true;
        s.cache = FrameCache::new();
        self.shared.1.notify_one();
    }
}
fn status(s: &State) -> Value {
    let r = s.desired.unwrap();
    let frame = s.cache.find(r.time);
    let state = if s.error.is_some() {
        "failed"
    } else if frame.is_some() {
        "ready"
    } else {
        "pending"
    };
    json!({"state":state,"sequence":r.sequence,"source_time_us":r.source_time,
        "pts_us":frame.as_ref().map(|f|f.pts),"end_us":frame.as_ref().map(|f|f.end),
        "decode_us":frame.as_ref().map(|f|f.decode_us),"codec_us":frame.as_ref().map(|f|f.codec_us),
        "transfer_us":frame.as_ref().map(|f|f.transfer_us),"pack_us":frame.as_ref().map(|f|f.pack_us),
        "cache_bytes":s.cache.bytes(),"cache_frames":s.cache.len(),"cache_hits":s.hits,"cache_misses":s.misses,"error":s.error})
}
#[derive(Default)]
pub struct VideoFrames {
    streams: HashMap<u64, Stream>,
    // Public sequences never share a namespace with automatic preview generations.
    sequences: HashMap<u64, (u64, f64, u64)>,
    counter: u64,
}
impl VideoFrames {
    pub fn clear(&mut self) {
        self.streams.clear();
        self.sequences.clear();
    }
    pub fn request(
        &mut self,
        project: &Project,
        root: &Path,
        object: u64,
        frame: f64,
        sequence: u64,
    ) -> Result<Value> {
        if !frame.is_finite() || frame < 0.0 || frame >= f64::from(project.frames) {
            return Err("video composition time outside project".into());
        }
        let layer = project
            .layers
            .iter()
            .find(|l| l.id == object)
            .ok_or("video object missing")?;
        let Content::Video { video } = &layer.content else {
            return Err("object is not video".into());
        };
        if sequence == 0 || sequence > i64::MAX as u64 {
            return Err("invalid video sequence".into());
        }
        if let Some((old, time, _)) = self.sequences.get(&object) {
            if sequence < *old {
                return Err("stale video request sequence".into());
            }
            if sequence == *old && frame != *time {
                return Err("video sequence reused for another composition time".into());
            }
        }
        self.sequences
            .retain(|id, _| project.layers.iter().any(|l| l.id == *id));
        let asset = project
            .video_assets
            .iter()
            .find(|a| a.id == video.asset)
            .ok_or("video asset missing")?;
        let source = video.source_time_us(layer.local_frame(frame), project.fps);
        if !layer.active(frame, project.frames)
            || source < asset.video_start_us as i64
            || source >= asset.video_end_us as i64
        {
            self.sequences.insert(object, (sequence, frame, 0));
            self.streams.remove(&object);
            return Ok(
                json!({"state":"outside","sequence":sequence,"source_time_us":source,"bytes":0,"object":object}),
            );
        }
        let generation = self.generation(object, source as u64)?;
        let mut result =
            self.request_source(root, asset, object, source as u64, generation, false)?;
        self.sequences.insert(object, (sequence, frame, generation));
        result["sequence"] = json!(sequence);
        Ok(result)
    }
    fn generation(&mut self, object: u64, time: u64) -> Result<u64> {
        if let Some(r) = self
            .streams
            .get(&object)
            .and_then(|s| s.shared.0.lock().ok()?.desired)
            .filter(|r| {
                self.streams[&object]
                    .key(time)
                    .is_ok_and(|(t, _)| r.time == t)
            })
        {
            return Ok(r.sequence);
        }
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or("video generation exhausted")?;
        Ok(self.counter)
    }
    fn request_source(
        &mut self,
        root: &Path,
        asset: &VideoAsset,
        object: u64,
        source: u64,
        generation: u64,
        prefetch: bool,
    ) -> Result<Value> {
        if self.streams.get(&object).is_some_and(|s| s.asset != *asset) {
            self.streams.remove(&object);
        }
        if !self.streams.contains_key(&object) {
            if self.streams.len() >= 4 {
                return Err("at most four video instances per reader; release idle frames".into());
            }
            self.streams
                .insert(object, Stream::new(root.into(), asset.clone())?);
        }
        let mut result = self.streams[&object].request(source, generation, prefetch)?;
        result["object"] = json!(object);
        result["width"] = json!(asset.display_width);
        result["height"] = json!(asset.display_height);
        Ok(result)
    }
    pub fn frame(
        &self,
        project: &Project,
        object: u64,
        sequence: u64,
    ) -> Result<Arc<DecodedFrame>> {
        let (current, time, generation) =
            self.sequences.get(&object).ok_or("video request missing")?;
        if *current != sequence {
            return Err("video request superseded".into());
        }
        let layer = project
            .layers
            .iter()
            .find(|l| l.id == object)
            .ok_or("video object removed")?;
        let Content::Video { video } = &layer.content else {
            return Err("object is no longer video".into());
        };
        let stream = self.streams.get(&object).ok_or("video reader missing")?;
        let desired = stream
            .shared
            .0
            .lock()
            .map_err(|_| "video frame state poisoned")?
            .desired
            .ok_or("video target missing")?;
        if !layer.active(*time, project.frames)
            || stream.asset.id != video.asset
            || stream
                .key(video.source_time_us(layer.local_frame(*time), project.fps) as u64)?
                .0
                != desired.time
        {
            return Err("video clip changed; request its current source time".into());
        }
        stream.ready(*generation)
    }
    pub fn prepare_scene(
        &mut self,
        project: &Project,
        root: &Path,
        scene: &Scene,
        _frame: f64,
    ) -> Result<Option<Vec<(u64, Arc<DecodedFrame>)>>> {
        self.prepare(project, root, scene, true)
    }
    pub fn prepare_scene_exact(
        &mut self,
        project: &Project,
        root: &Path,
        scene: &Scene,
    ) -> Result<Option<Vec<(u64, Arc<DecodedFrame>)>>> {
        self.prepare(project, root, scene, false)
    }
    fn prepare(
        &mut self,
        project: &Project,
        root: &Path,
        scene: &Scene,
        prefetch: bool,
    ) -> Result<Option<Vec<(u64, Arc<DecodedFrame>)>>> {
        let layers=scene.video_layers();
        self.streams.retain(|object,_|layers.iter().any(|l|l.id==*object));
        let mut frames = Vec::new();
        let mut pending = false;
        for layer in layers {
            if layer.video.is_none() {
                continue;
            }
            let source = layer.video.as_ref().unwrap();
            let asset = project
                .video_assets
                .iter()
                .find(|a| a.id == source.asset)
                .ok_or("video asset missing")?;
            let generation = self.generation(layer.id, source.source_time_us)?;
            let r = self.request_source(
                root,
                asset,
                layer.id,
                source.source_time_us,
                generation,
                prefetch,
            )?;
            match r["state"].as_str() {
                Some("ready") => {
                    frames.push((layer.id, self.streams[&layer.id].ready(generation)?))
                }
                Some("failed") => {
                    return Err(r["error"].as_str().unwrap_or("video decode failed").into())
                }
                _ => pending = true,
            }
        }
        if pending {
            Ok(None)
        } else {
            Ok(Some(frames))
        }
    }
    pub fn metrics(&self) -> Value {
        let mut bytes = 0;
        let mut frames = 0;
        let mut hits = 0;
        let mut misses = 0;
        let mut decoded = 0;
        let mut cancelled = 0;
        for stream in self.streams.values() {
            let s = stream.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            bytes += s.cache.bytes();
            frames += s.cache.len();
            hits += s.hits;
            misses += s.misses;
            decoded += s.decoded;
            cancelled += s.cancelled;
        }
        json!({"cacheBytes":bytes,"cacheFrames":frames,"cacheBudgetBytes":4 * crate::video_cache::CACHE_BYTES,
            "cacheHits":hits,"cacheMisses":misses,"decodedFrames":decoded,"cancelledFrames":cancelled,"streams":self.streams.len()})
    }
}
