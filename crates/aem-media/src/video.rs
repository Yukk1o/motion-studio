//! Streaming source transactions; platform probing is supplied by the Android backend.
use crate::{contained_dir, Result};
use aem_core::{AudioAsset, Command, Content, Engine, Layer, LayerTimeline, VideoAsset, VideoClip};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

static NONCE: AtomicU64 = AtomicU64::new(1);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
pub struct VideoProbe {
    pub asset: VideoAsset,
    pub timestamps: Vec<u64>,
    pub audio_track: Option<u32>,
    pub first_rgba: Vec<u8>,
    pub tracks: Value,
}
pub type ProbeVideo = Arc<
    dyn Fn(&Path, Option<u32>, Option<u32>, &dyn Fn() -> Result<()>) -> Result<VideoProbe>
        + Send
        + Sync,
>;
#[derive(Clone)]
pub struct VideoImportOptions {
    pub request_id: String,
    pub at_frame: u32,
    pub name: String,
    pub track: Option<u32>,
    pub audio_track: Option<u32>,
    pub with_audio: bool,
}
#[derive(Clone, Serialize)]
pub struct VideoTaskStatus {
    pub request_id: String,
    pub operation: String,
    pub state: String,
    pub phase: String,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl VideoTaskStatus {
    fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "cancelled")
    }
}
struct Stage(PathBuf);
impl Stage {
    fn new(root: &Path) -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = root.join(format!(
            ".video-import-{stamp}-{}",
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Prepared {
    stage: Stage,
    probe: VideoProbe,
    audio: Option<AudioAsset>,
    options: VideoImportOptions,
}
struct Task {
    status: VideoTaskStatus,
    prepared: Option<Prepared>,
    cancel: Arc<AtomicBool>,
}
struct Worker;
impl Drop for Worker {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub struct VideoJobs {
    root: PathBuf,
    audio_decode: crate::DecodeAudio,
    tasks: Mutex<HashMap<String, Arc<Mutex<Task>>>>,
}
impl VideoJobs {
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::with_audio_decoder(root, Arc::new(crate::decode_audio))
    }
    pub fn with_audio_decoder(root: PathBuf, audio_decode: crate::DecodeAudio) -> Result<Self> {
        Ok(Self {
            root: root.canonicalize().map_err(|e| e.to_string())?,
            audio_decode,
            tasks: Mutex::new(HashMap::new()),
        })
    }
    pub fn contains(&self, id: &str) -> bool {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(id)
    }
    fn task(&self, id: &str) -> Result<Arc<Mutex<Task>>> {
        self.tasks
            .lock()
            .map_err(|_| "video task registry poisoned")?
            .get(id)
            .cloned()
            .ok_or("video task missing".into())
    }
    fn reserve(&self, id: &str, operation: &str) -> Result<(Arc<Mutex<Task>>, Worker)> {
        if id.is_empty() || id.len() > 128 {
            return Err("invalid media request id".into());
        }
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| "video task registry poisoned")?;
        if tasks.contains_key(id) || tasks.len() >= 64 {
            return Err("duplicate request id or video task budget exceeded".into());
        }
        if tasks
            .values()
            .filter(|t| !t.lock().unwrap().status.terminal())
            .count()
            >= 2
        {
            return Err("at most two unfinished video tasks per session".into());
        }
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .map_err(|_| "video import worker budget exhausted")?;
        let t = Arc::new(Mutex::new(Task {
            status: VideoTaskStatus {
                request_id: id.into(),
                operation: operation.into(),
                state: "running".into(),
                phase: "queued".into(),
                progress: 0.0,
                metadata: None,
                edit_result: None,
                error: None,
            },
            prepared: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }));
        tasks.insert(id.into(), t.clone());
        Ok((t, Worker))
    }
    pub fn status(&self, id: &str) -> Result<VideoTaskStatus> {
        Ok(self
            .task(id)?
            .lock()
            .map_err(|_| "video task poisoned")?
            .status
            .clone())
    }
    pub fn cancel(&self, id: &str) -> Result<VideoTaskStatus> {
        let task = self.task(id)?;
        let mut t = task.lock().map_err(|_| "video task poisoned")?;
        if !t.status.terminal() {
            t.cancel.store(true, Ordering::Release);
            t.prepared.take();
            t.status.state = "cancelled".into();
            t.status.phase = "cancelled".into();
        }
        Ok(t.status.clone())
    }
    pub fn release(&self, id: &str) -> Result<()> {
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| "video task registry poisoned")?;
        if !tasks
            .get(id)
            .ok_or("video task missing")?
            .lock()
            .map_err(|_| "video task poisoned")?
            .status
            .terminal()
        {
            return Err("finish or cancel video task before release".into());
        }
        tasks.remove(id);
        Ok(())
    }
    pub fn start(
        &self,
        open: impl FnOnce() -> Result<(Box<dyn Read + Send>, Option<u64>)> + Send + 'static,
        options: VideoImportOptions,
        probe_only: bool,
        probe: ProbeVideo,
    ) -> Result<VideoTaskStatus> {
        if options.name.len() > 1024 {
            return Err("video name exceeds limit".into());
        }
        let (task, worker) = self.reserve(
            &options.request_id,
            if probe_only {
                "probe_video"
            } else {
                "import_video"
            },
        )?;
        let initial = task.lock().unwrap().status.clone();
        let root = self.root.clone();
        let audio_decode = self.audio_decode.clone();
        let t = task.clone();
        let spawn=std::thread::Builder::new().name("motion-video-import".into()).spawn(move||{
            let _worker=worker;
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<Prepared>{
                update(&t,"opening",0.0)?;let(mut reader,size)=open()?;
                if size.is_some_and(|s|s>aem_core::storage::MAX_MEDIA_ASSET){return Err("video exceeds 512 MiB".into());}
                if fs2::available_space(&root).map_err(|e|e.to_string())?<size.unwrap_or(0).saturating_add(1024*1024){return Err("insufficient video source storage".into());}
                let stage=Stage::new(&root)?;let source=stage.0.join("source.mp4");let mut file=File::create(&source).map_err(|e|e.to_string())?;
                let mut buf=[0u8;65536];let mut bytes=0u64;
                loop {update(&t,"copying",size.map_or(0.0,|s|bytes as f64/s.max(1) as f64))?;
                    let n=reader.read(&mut buf).map_err(|e|e.to_string())?;if n==0{break;}bytes+=n as u64;
                    if bytes>aem_core::storage::MAX_MEDIA_ASSET{return Err("video exceeds 512 MiB".into());}
                    if bytes%(1024*1024)<n as u64&&fs2::available_space(&root).map_err(|e|e.to_string())?<1024*1024{return Err("insufficient video import storage".into());}
                    file.write_all(&buf[..n]).map_err(|e|e.to_string())?;
                }
                if bytes==0||size.is_some_and(|s|s!=bytes){return Err("video source length mismatch".into());}
                file.sync_all().map_err(|e|e.to_string())?;drop(file);drop(reader);
                update(&t,"probing",0.0)?;let mut p=probe(&source,options.track,if options.with_audio {options.audio_track}else{Some(u32::MAX)},&||update(&t,"probing",0.0))?;
                let audio=if options.with_audio {if let Some(track)=p.audio_track {
                    update(&t,"decoding_audio",0.0)?;
                    let pcm=stage.0.join("decoded.pcm");let a=audio_decode(&source,&pcm,Some(track),crate::Limits::default().cache_bytes,&mut |v|update(&t,"decoding_audio",v))?;
                    crate::mixer::build_waveform(&pcm,&a,||update(&t,"waveform",0.0))?;Some(a)
                }else{None}}else{None};
                if let Some(a)=&audio{p.asset.duration_us=p.asset.duration_us.max(a.duration_us);}
                p.asset.validate().map_err(|e|e.to_string())?;
                save_index(&stage.0.join("index.pts"),&p.asset,&p.timestamps)?;
                image::save_buffer(stage.0.join("thumbnail.png"),&p.first_rgba,p.asset.display_width,p.asset.display_height,image::ColorType::Rgba8).map_err(|e|e.to_string())?;
                Ok(Prepared{stage,probe:p,audio,options})
            }));
            let mut t=t.lock().unwrap_or_else(|e|e.into_inner());if t.cancel.load(Ordering::Acquire){return;}
            match result {Ok(Ok(p))=>{t.status.metadata=Some(json!({"video":p.probe.asset,"audio":p.audio,"audio_tracks":p.probe.tracks}));t.status.progress=1.0;
                if probe_only{t.status.state="succeeded".into();t.status.phase="complete".into();}else{t.status.state="ready".into();t.status.phase="awaiting_commit".into();t.prepared=Some(p);}},
                other=>{t.status.state="failed".into();t.status.phase="failed".into();t.status.error=Some(match other{Ok(Err(e))=>e,_=>"video worker failed".into()});}}
        });
        if let Err(e) = spawn {
            let mut t = task.lock().unwrap();
            t.status.state = "failed".into();
            t.status.error = Some(e.to_string());
            return Err(e.to_string());
        }
        Ok(initial)
    }
    pub fn commit(&self, id: &str, engine: &mut Engine) -> Result<VideoTaskStatus> {
        let task = self.task(id)?;
        let mut t = task.lock().map_err(|_| "video task poisoned")?;
        if t.status.terminal() {
            return Ok(t.status.clone());
        }
        if t.status.state != "ready" {
            return Err("video import is not ready".into());
        }
        let mut p = t.prepared.take().ok_or("prepared video missing")?;
        let mut published = Vec::new();
        let result = (|| -> Result<Value> {
            let project = engine.project();
            let at = p.options.at_frame;
            if at >= project.frames {
                return Err("video insertion outside composition".into());
            }
            let aid = project
                .assets
                .iter()
                .map(|a| a.id)
                .chain(project.audio_assets.iter().map(|a| a.id))
                .chain(project.video_assets.iter().map(|a| a.id))
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("asset ID exhausted")?;
            let object = project
                .layers
                .iter()
                .map(|l| l.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("object ID exhausted")?;
            let frames = (p.probe.asset.duration_us * u64::from(project.fps)).div_ceil(1_000_000);
            let out = (u64::from(at) + frames).min(u64::from(project.frames)) as u32;
            let stem = p
                .stage
                .0
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .trim_start_matches('.');
            let ext = crate::source_extension(&p.stage.0.join("source.mp4"))?;
            let path = format!("assets/{stem}.{ext}");
            p.probe.asset.id = aid;
            p.probe.asset.path = path.clone();
            let mut commands = Vec::new();
            if let Some(a) = &mut p.audio {
                a.id = aid.checked_add(1).ok_or("asset ID exhausted")?;
                a.path = path.clone();
                p.probe.asset.audio_asset = Some(a.id);
                commands.push(Command::RegisterAudioAsset { asset: a.clone() });
            }
            let mut layer = Layer::solid(
                object,
                &p.options.name,
                [
                    p.probe.asset.display_width as f32,
                    p.probe.asset.display_height as f32,
                ],
                [project.width as f32 / 2.0, project.height as f32 / 2.0, 0.0],
                [1.0; 4],
            );
            layer.content = Content::Video {
                video: VideoClip::new(aid),
            };
            layer.timeline = Some(LayerTimeline {
                in_frame: at,
                out_frame: out,
                offset_frame: at as i32,
            });
            commands.push(Command::RegisterVideoAsset {
                asset: p.probe.asset.clone(),
            });
            commands.push(Command::Add { layer });
            contained_dir(&self.root, &self.root.join("assets"))?;
            let cache = video_cache_path(&self.root, &p.probe.asset)?;
            contained_dir(&self.root, cache.parent().unwrap())?;
            let mut moves = vec![
                (p.stage.0.join("source.mp4"), self.root.join(&path)),
                (p.stage.0.join("index.pts"), cache.clone()),
                (p.stage.0.join("thumbnail.png"), cache.with_extension("png")),
            ];
            if let Some(a) = &p.audio {
                let pcm = crate::cache_path(&self.root, a)?;
                contained_dir(&self.root, pcm.parent().unwrap())?;
                moves.push((p.stage.0.join("decoded.pcm"), pcm.clone()));
                moves.push((p.stage.0.join("decoded.wave"), pcm.with_extension("wave")));
            }
            for (from, to) in moves {
                fs::rename(from, &to).map_err(|e| e.to_string())?;
                published.push(to);
            }
            engine
                .apply_batch_saved(commands, &self.root)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"op":"import_media","kind":"video","asset":aid,"object":object,"in_frame":at,"out_frame":out,"source_offset_us":0,"has_audio":p.audio.is_some(),"audio_asset":p.probe.asset.audio_asset,"truncated_to_composition":u64::from(out-at)<frames}),
            )
        })();
        match result {
            Ok(edit) => {
                t.status.state = "succeeded".into();
                t.status.phase = "complete".into();
                t.status.edit_result = Some(edit);
                t.status.metadata = Some(
                    json!({"video":p.probe.asset,"audio":p.audio,"audio_tracks":p.probe.tracks}),
                );
            }
            Err(e) => {
                for path in published {
                    let _ = fs::remove_file(path);
                }
                t.status.state = "failed".into();
                t.status.phase = "failed".into();
                t.status.error = Some(e);
            }
        }
        Ok(t.status.clone())
    }
    pub fn prepare_cache(
        &self,
        id: &str,
        expected: VideoAsset,
        probe: ProbeVideo,
    ) -> Result<VideoTaskStatus> {
        let (task, worker) = self.reserve(id, "prepare_video")?;
        let initial = task.lock().unwrap().status.clone();
        let t = task.clone();
        let root = self.root.clone();
        let spawn = std::thread::Builder::new()
            .name("motion-video-cache".into())
            .spawn(move || {
                let _worker = worker;
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                        update(&t, "probing", 0.0)?;
                        let source = root
                            .join(&expected.path)
                            .canonicalize()
                            .map_err(|e| e.to_string())?;
                        if !source.starts_with(&root) {
                            return Err("video source outside project".into());
                        }
                        let mut p = probe(&source, Some(expected.track), Some(u32::MAX), &|| {
                            update(&t, "probing", 0.0)
                        })?;
                        p.asset.id = expected.id;
                        p.asset.path = expected.path.clone();
                        p.asset.audio_asset = expected.audio_asset;
                        p.asset.duration_us = expected.duration_us;
                        if p.asset != expected {
                            return Err("owned video metadata changed".into());
                        }
                        let stage = Stage::new(&root)?;
                        save_index(&stage.0.join("index.pts"), &expected, &p.timestamps)?;
                        image::save_buffer(
                            stage.0.join("thumbnail.png"),
                            &p.first_rgba,
                            expected.display_width,
                            expected.display_height,
                            image::ColorType::Rgba8,
                        )
                        .map_err(|e| e.to_string())?;
                        let cache = video_cache_path(&root, &expected)?;
                        contained_dir(&root, cache.parent().unwrap())?;
                        let mut state = t.lock().map_err(|_| "video task poisoned")?;
                        if state.cancel.load(Ordering::Acquire) {
                            return Err("video cache cancelled".into());
                        }
                        fs::rename(stage.0.join("index.pts"), &cache).map_err(|e| e.to_string())?;
                        fs::rename(stage.0.join("thumbnail.png"), cache.with_extension("png"))
                            .map_err(|e| e.to_string())?;
                        state.status.state = "succeeded".into();
                        state.status.phase = "complete".into();
                        state.status.progress = 1.0;
                        state.status.metadata = Some(json!({"video":expected}));
                        Ok(())
                    }));
                let mut state = t.lock().unwrap_or_else(|e| e.into_inner());
                if state.cancel.load(Ordering::Acquire) {
                    return;
                }
                if let Err(error) =
                    result.unwrap_or_else(|_| Err("video cache worker failed".into()))
                {
                    state.status.state = "failed".into();
                    state.status.phase = "failed".into();
                    state.status.error = Some(error);
                }
            });
        if let Err(e) = spawn {
            let mut t = task.lock().unwrap();
            t.status.state = "failed".into();
            t.status.error = Some(e.to_string());
            return Err(e.to_string());
        }
        Ok(initial)
    }
}
impl Drop for VideoJobs {
    fn drop(&mut self) {
        for t in self
            .tasks
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            let mut t = t.lock().unwrap_or_else(|e| e.into_inner());
            t.cancel.store(true, Ordering::Release);
            t.prepared.take();
        }
    }
}
fn update(task: &Mutex<Task>, phase: &str, progress: f64) -> Result<()> {
    let mut t = task.lock().map_err(|_| "video task poisoned")?;
    if t.cancel.load(Ordering::Acquire) {
        return Err("video import cancelled".into());
    }
    t.status.phase = phase.into();
    t.status.progress = progress;
    Ok(())
}
pub fn video_cache_path(root: &Path, asset: &VideoAsset) -> Result<PathBuf> {
    aem_core::storage::validate_relative_path(&asset.path).map_err(|e| e.to_string())?;
    let stem = Path::new(&asset.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid video source name")?;
    Ok(root.join("cache/video-v1").join(format!("{stem}.pts")))
}
pub fn save_index(path: &Path, asset: &VideoAsset, pts: &[u64]) -> Result<()> {
    validate_index(asset, pts)?;
    let mut f = File::create(path).map_err(|e| e.to_string())?;
    f.write_all(b"MSVID001")
        .and_then(|_| f.write_all(&asset.bytes.to_le_bytes()))
        .map_err(|e| e.to_string())?;
    for t in pts {
        f.write_all(&t.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    f.sync_all().map_err(|e| e.to_string())
}
pub fn load_video_index(root: &Path, asset: &VideoAsset) -> Result<Vec<u64>> {
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    let path = video_cache_path(root, asset)?
        .canonicalize()
        .map_err(|_| "video cache missing; call prepare_video")?;
    if !path.starts_with(base) {
        return Err("video cache outside project".into());
    }
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let size = f.metadata().map_err(|e| e.to_string())?.len();
    if size != 16 + u64::from(asset.frame_count) * 8
        || size > 16 + u64::from(aem_core::MAX_VIDEO_FRAMES) * 8
    {
        return Err("video cache invalid; call prepare_video".into());
    }
    let mut bytes = vec![0; size as usize];
    f.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    if &bytes[..8] != b"MSVID001"
        || u64::from_le_bytes(bytes[8..16].try_into().unwrap()) != asset.bytes
    {
        return Err("video cache identity mismatch".into());
    }
    let pts = bytes[16..]
        .chunks_exact(8)
        .map(|v| u64::from_le_bytes(v.try_into().unwrap()))
        .collect::<Vec<_>>();
    validate_index(asset, &pts)?;
    Ok(pts)
}
fn validate_index(a: &VideoAsset, pts: &[u64]) -> Result<()> {
    if pts.len() != a.frame_count as usize
        || pts.first() != Some(&a.video_start_us)
        || pts.last().is_none_or(|t| *t >= a.video_end_us)
        || pts.windows(2).any(|v| v[0] >= v[1])
    {
        return Err("invalid video timestamp index".into());
    }
    Ok(())
}
