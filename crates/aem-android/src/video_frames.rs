//! One pending target and one decoded frame per instance. Latest target wins.
use super::video_decode::{DecodedFrame, Decoder};
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
    sequence: u64,
}
struct State {
    desired: Option<Request>,
    frame: Option<Arc<DecodedFrame>>,
    error: Option<String>,
    done: u64,
    stop: bool,
}
struct Stream {
    asset: u64,
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl Stream {
    fn new(root: PathBuf, asset: VideoAsset) -> Result<Self> {
        let shared = Arc::new((
            Mutex::new(State {
                desired: None,
                frame: None,
                error: None,
                done: 0,
                stop: false,
            }),
            Condvar::new(),
        ));
        let copy = shared.clone();
        let id = asset.id;
        std::thread::Builder::new()
            .name("motion-video-frames".into())
            .spawn(move || {
                let mut decoder = None;
                loop {
                    let req = {
                        let (lock, cv) = &*copy;
                        let s = lock.lock().unwrap_or_else(|e| e.into_inner());
                        let s = cv
                            .wait_while(s, |s| {
                                !s.stop && s.desired.is_none_or(|r| r.sequence == s.done)
                            })
                            .unwrap_or_else(|e| e.into_inner());
                        if s.stop {
                            return;
                        }
                        s.desired.unwrap()
                    };
                    let check = || {
                        let s = copy.0.lock().map_err(|_| "video frame state poisoned")?;
                        if s.stop || s.desired.is_none_or(|r| r.sequence != req.sequence) {
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
                                    .join(&asset.path)
                                    .canonicalize()
                                    .map_err(|e| e.to_string())?;
                                if !source.starts_with(base)
                                    || source.metadata().map_err(|e| e.to_string())?.len()
                                        != asset.bytes
                                {
                                    return Err("owned video source missing or changed".into());
                                }
                                decoder = Some(Decoder::new(
                                    &source,
                                    asset.clone(),
                                    aem_media::load_video_index(&root, &asset)?,
                                )?);
                            }
                            Ok(Arc::new(decoder.as_mut().unwrap().frame(req.time, &check)?))
                        },
                    ))
                    .unwrap_or_else(|_| Err("video decoder worker failed".into()));
                    let mut state = copy.0.lock().unwrap_or_else(|e| e.into_inner());
                    if state.stop {
                        return;
                    }
                    if state.desired.is_none_or(|r| r.sequence != req.sequence) {
                        drop(state);
                        // Superseding a target does not invalidate MediaCodec. Resume or
                        // seek on the next request instead of recreating it during scrubbing.
                        if result
                            .as_ref()
                            .is_err_and(|e| e != "video target superseded")
                        {
                            decoder = None;
                        }
                        continue;
                    }
                    state.done = req.sequence;
                    match result {
                        Ok(frame) => {
                            state.frame = Some(frame);
                            state.error = None;
                        }
                        Err(e) => {
                            state.frame = None;
                            state.error = Some(e);
                            decoder = None;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { asset: id, shared })
    }
    fn request(&self, time: u64, sequence: u64) -> Result<Value> {
        if sequence == 0 || sequence > i64::MAX as u64 {
            return Err("video sequence must be a positive signed 64-bit value".into());
        }
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
            s.desired = Some(Request { time, sequence });
            s.error = None;
            if s.frame
                .as_ref()
                .is_some_and(|f| f.pts <= time && time < f.end)
            {
                s.done = sequence;
            }
            self.shared.1.notify_one();
        }
        Ok(status(&s))
    }
    fn ready(&self, sequence: u64) -> Result<Arc<DecodedFrame>> {
        let s = self
            .shared
            .0
            .lock()
            .map_err(|_| "video frame state poisoned")?;
        if s.desired.is_none_or(|r| r.sequence != sequence) || s.done != sequence {
            return Err("video frame pending or superseded".into());
        }
        if let Some(e) = &s.error {
            return Err(e.clone());
        }
        s.frame.clone().ok_or("video frame unavailable".into())
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let mut s = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        s.stop = true;
        s.frame = None;
        self.shared.1.notify_one();
    }
}
fn status(s: &State) -> Value {
    let r = s.desired.unwrap();
    let state = if s.done != r.sequence {
        "pending"
    } else if s.error.is_some() {
        "failed"
    } else {
        "ready"
    };
    json!({"state":state,"sequence":r.sequence,"source_time_us":r.time,"pts_us":s.frame.as_ref().filter(|_|state=="ready").map(|f|f.pts),
        "end_us":s.frame.as_ref().filter(|_|state=="ready").map(|f|f.end),"decode_us":s.frame.as_ref().filter(|_|state=="ready").map(|f|f.decode_us),"error":s.error})
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
        let mut result = self.request_source(root, asset, object, source as u64, generation)?;
        self.sequences.insert(object, (sequence, frame, generation));
        result["sequence"] = json!(sequence);
        Ok(result)
    }
    fn generation(&mut self, object: u64, time: u64) -> Result<u64> {
        if let Some(r) = self
            .streams
            .get(&object)
            .and_then(|s| s.shared.0.lock().ok()?.desired)
            .filter(|r| r.time == time)
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
    ) -> Result<Value> {
        if self
            .streams
            .get(&object)
            .is_some_and(|s| s.asset != asset.id)
        {
            self.streams.remove(&object);
        }
        if !self.streams.contains_key(&object) {
            if self.streams.len() >= 4 {
                return Err("at most four video instances per reader; release idle frames".into());
            }
            self.streams
                .insert(object, Stream::new(root.into(), asset.clone())?);
        }
        let mut result = self.streams[&object].request(source, generation)?;
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
            || stream.asset != video.asset
            || video.source_time_us(layer.local_frame(*time), project.fps) != desired.time as i64
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
        self.streams.retain(|object, _| {
            scene
                .layers
                .iter()
                .any(|l| l.id == *object && l.video.is_some())
        });
        let mut frames = Vec::new();
        let mut pending = false;
        for layer in &scene.layers {
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
            let r =
                self.request_source(root, asset, layer.id, source.source_time_us, generation)?;
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
}
