//! Editing session lifecycle, GPU resources and frame sampling.
//!
//! This is the portable replacement for the Android-only `Session` that used to
//! live in `aem-android`. It owns the engine, the sampled scene, the effect
//! plan builder, media jobs and the preview policy; the host supplies only
//! surface creation and media decoding through [`crate::platform::Platform`].
use crate::platform::{Platform, SurfaceTarget};
use crate::video_frames::VideoFrames;
use aem_core::{Engine, Observer, Project, Scene};
use aem_media::{AudioJobs, AudioMixer, Limits, PackageJobs, VideoJobs};
use aem_render::{
    FrameMeasurement, FrameRecorder, GpuTimer, Presenter, PreviewMode, PreviewPolicy, RenderTarget,
    Renderer,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, ThreadId},
    time::Instant,
};

pub type Result<T> = std::result::Result<T, String>;

static NEXT: AtomicI64 = AtomicI64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();

/// Monotonic counter shared by session handles and plugin editor tokens.
pub fn next_token() -> i64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn sessions() -> &'static Mutex<HashMap<i64, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Live GPU resources bound to one window.
pub struct Graphics {
    pub surface: wgpu::Surface<'static>,
    pub renderer: Renderer,
    pub scratch: RenderTarget,
    pub presenter: Presenter,
    pub config: wgpu::SurfaceConfiguration,
    pub timer: Option<GpuTimer>,
    /// Keeps the wgpu instance alive for as long as its surface.
    _instance: wgpu::Instance,
}

pub struct Session {
    pub composition_contexts: HashMap<String, CompositionContext>,
    pub composition_bundle: Vec<u8>,
    pub media_compositions: HashMap<String, String>,
    pub video_jobs: VideoJobs,
    pub video_frames: VideoFrames,
    pub audio_jobs: AudioJobs,
    pub package_jobs: PackageJobs,
    pub audio_mixer: Option<(u64, AudioMixer)>,
    pub audio_pcm: Vec<f32>,
    pub engine: Engine,
    pub scene: Scene,
    pub geometry: aem_core::PlaneCompositor,
    pub observer: Observer,
    pub observing: bool,
    pub frame: f64,
    pub root: PathBuf,
    pub graphics: Option<Graphics>,
    pub owner: ThreadId,
    pub presented: u64,
    pub last_cpu_us: u64,
    pub render_attempts: u64,
    pub video_pending_attempts: u64,
    pub video_prepare_us: u64,
    pub video_upload_us: u64,
    pub last_error: Option<String>,
    pub last_presented_frame: Option<f64>,
    pub last_presented_revision: u64,
    pub view_revision: u64,
    pub last_presented_view_revision: u64,
    pub surface_epoch: u64,
    pub preview: PreviewPolicy,
    pub recorder: Option<FrameRecorder>,
    pub effects: aem_render::effect_plan::PlanBuilder,
    pub plugin_root: PathBuf,
    pub editor: Option<aem_core::plugin_editor::PluginEditorSession>,
    pub editor_token: String,
    pub editor_renderer: Option<Renderer>,
    pub editor_target: Option<aem_render::CaptureTarget>,
    platform: Arc<dyn Platform>,
}

/// Per-composition UI context: playhead, selection and timeline view state.
#[derive(Clone, Default)]
pub struct CompositionContext {
    pub frame: f64,
    pub selection: Vec<u64>,
    pub timeline: Value,
    pub path: Vec<String>,
}

impl Session {
    pub fn new(project: Project, root: PathBuf, platform: Arc<dyn Platform>) -> Result<Self> {
        let engine = Engine::new(project).map_err(|e| e.to_string())?;
        let project = engine.project();
        let mut scene = Scene::new(project);
        let initial_error = match scene.sample(project, 0.0, None) {
            Ok(()) => None,
            Err(error @ aem_core::Error::Expression { .. }) => {
                let mut base = project.clone();
                for expression in &mut base.expressions {
                    expression.enabled = false;
                }
                scene
                    .sample(&base, 0.0, None)
                    .map_err(|e| e.to_string())?;
                Some(error.to_string())
            }
            Err(error) => return Err(error.to_string()),
        };
        let observer = Observer::new(project.width, project.height);
        let plugin_root = root
            .parent()
            .ok_or("project directory has no parent")?
            .join("plugins");
        let effects = aem_render::effect_plan::PlanBuilder::new(
            aem_effects::Registry::load(&plugin_root).map_err(|e| e.to_string())?,
        )?;
        let video_jobs = VideoJobs::with_audio_decoder(root.clone(), audio_decoder(&platform))?;
        let audio_jobs =
            AudioJobs::with_decoder(root.clone(), Limits::default(), audio_decoder(&platform))?;
        Ok(Self {
            composition_contexts: Default::default(),
            composition_bundle: Vec::new(),
            media_compositions: Default::default(),
            video_jobs,
            video_frames: VideoFrames::new(platform.clone()),
            audio_jobs,
            package_jobs: PackageJobs::default(),
            audio_mixer: None,
            audio_pcm: Vec::new(),
            engine,
            scene,
            geometry: aem_core::PlaneCompositor::new(),
            observer,
            observing: false,
            frame: 0.0,
            root,
            graphics: None,
            owner: thread::current().id(),
            presented: 0,
            last_cpu_us: 0,
            render_attempts: 0,
            video_pending_attempts: 0,
            video_prepare_us: 0,
            video_upload_us: 0,
            last_error: initial_error,
            last_presented_frame: None,
            last_presented_revision: 0,
            view_revision: 0,
            last_presented_view_revision: 0,
            surface_epoch: 0,
            preview: PreviewPolicy::default(),
            recorder: None,
            effects,
            plugin_root,
            editor: None,
            editor_token: String::new(),
            editor_renderer: None,
            editor_target: None,
            platform,
        })
    }

    /// The host platform that owns this session's media and surface backends.
    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    pub fn replace_project(&mut self, engine: Engine, root: PathBuf) -> Result<()> {
        let platform = self.platform.clone();
        let audio_jobs = AudioJobs::with_decoder(
            root.clone(),
            Limits::default(),
            audio_decoder(&platform),
        )?;
        let video_jobs = VideoJobs::with_audio_decoder(root.clone(), audio_decoder(&platform))?;
        if let Some(g) = &mut self.graphics {
            g.renderer
                .configure_assets(engine.project(), &root)
                .map_err(|e| e.to_string())?;
        }
        if let Some(mut editor) = self.editor.take() {
            editor
                .close(&mut self.engine, false)
                .map_err(|e| e.to_string())?;
        }
        self.editor_renderer = None;
        self.editor_target = None;
        self.effects.alpha_images.clear();
        self.scene = Scene::new(engine.project());
        self.observer = Observer::new(engine.project().width, engine.project().height);
        self.composition_contexts.clear();
        self.media_compositions.clear();
        self.engine = engine;
        self.audio_jobs = audio_jobs;
        self.video_jobs = video_jobs;
        // Frozen package exports stay queryable across project replacement, but
        // their open source handles belong to the old snapshot.
        self.video_frames.clear();
        self.audio_mixer = None;
        self.root = root;
        self.frame = 0.0;
        self.observing = false;
        self.last_presented_frame = None;
        self.last_error = None;
        self.view_revision += 1;
        // Imported expression source stays editable when frame zero fails.
        if let Err(error) = self.sample() {
            self.last_error = Some(error);
        }
        Ok(())
    }

    pub fn check_thread(&self) -> Result<()> {
        if self.owner != thread::current().id() {
            Err("native session must run on its owning worker thread".into())
        } else {
            Ok(())
        }
    }

    pub fn detach(&mut self) {
        self.video_frames.clear();
        if let Some(g) = self.graphics.take() {
            g.renderer.device.poll(wgpu::Maintain::Wait);
            drop(g);
        }
    }

    /// Bind a host-created surface. Everything after surface creation is shared
    /// with the Android preview path.
    pub fn attach(&mut self, target: SurfaceTarget) -> Result<()> {
        self.detach();
        let SurfaceTarget {
            instance,
            surface,
            config,
            mut renderer,
        } = target;
        renderer.set_effect_registry(self.effects.registry.clone());
        renderer
            .set_scratch_budget(self.effects.scratch_budget())
            .map_err(|e| e.to_string())?;
        surface.configure(&renderer.device, &config);
        renderer
            .configure_assets(self.engine.project(), &self.root)
            .map_err(|e| e.to_string())?;
        let (render_width, render_height) = self.preview.render_dimensions(
            self.engine.project().width,
            self.engine.project().height,
            config.width,
            config.height,
        );
        let scratch = renderer
            .render_target(render_width, render_height)
            .map_err(|e| e.to_string())?;
        let presenter = Presenter::new(&renderer, &scratch.view, config.format);
        // Auto quality needs GPU cost even without diagnostic recording; only
        // four reusable 32-byte timing buffers are read, never video pixels.
        let timer = GpuTimer::new(&renderer.device, &renderer.queue);
        self.graphics = Some(Graphics {
            surface,
            renderer,
            scratch,
            presenter,
            config,
            _instance: instance,
            timer,
        });
        self.last_error = None;
        self.surface_epoch += 1;
        self.last_presented_frame = None;
        Ok(())
    }

    pub fn render(&mut self, frame: f64) -> Result<bool> {
        let began = Instant::now();
        self.render_attempts += 1;
        self.frame = frame;
        self.sample()?;
        let tier = self.preview.tier();
        let Some(g) = &mut self.graphics else {
            return Ok(false);
        };
        g.renderer.retain_video_instances(&self.scene);
        g.renderer.set_image_prefetch(
            aem_render::image_resources::upcoming_assets(self.engine.project(), frame),
        );
        let image_resolution = if self.preview.mode == PreviewMode::High {
            aem_render::image_resources::Resolution::Full
        } else {
            aem_render::image_resources::Resolution::Preview(
                aem_render::image_resources::MAX_PREVIEW_EDGE,
            )
        };
        let images_ready = g
            .renderer
            .prepare_scene_assets(&self.scene, image_resolution, true)
            .map_err(|e| e.to_string())?;
        let preparing = Instant::now();
        let frames =
            self.video_frames
                .prepare_scene(self.engine.project(), &self.root, &self.scene, frame)?;
        self.video_prepare_us = preparing.elapsed().as_micros() as u64;
        let Some(frames) = frames else {
            self.video_pending_attempts += 1;
            self.video_upload_us = 0;
            return Ok(false);
        };
        if !images_ready {
            self.video_upload_us = 0;
            return Ok(false);
        }
        let uploading = Instant::now();
        for (object, image) in frames {
            let source = self
                .scene
                .video_layers()
                .into_iter()
                .find(|l| l.id == object)
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .asset;
            image.upload(&mut g.renderer, object, source)?;
        }
        self.video_upload_us = uploading.elapsed().as_micros() as u64;
        g.renderer.device.poll(wgpu::Maintain::Poll);
        g.renderer.check_health().map_err(|e| e.to_string())?;
        let mut gpu_work_us = None;
        if let Some(timer) = &mut g.timer {
            for timing in timer.collect().into_iter().flatten() {
                gpu_work_us = Some(timing.total_us);
                if let Some(recorder) = &mut self.recorder {
                    recorder.timing(timing);
                }
            }
        }
        let (rw, rh) = self.preview.render_dimensions(
            self.scene.width,
            self.scene.height,
            g.config.width,
            g.config.height,
        );
        if (g.scratch.width, g.scratch.height) != (rw, rh) {
            let target = g
                .renderer
                .render_target(rw, rh)
                .map_err(|e| e.to_string())?;
            g.presenter = Presenter::new(&g.renderer, &target.view, g.config.format);
            g.scratch = target;
        }
        let acquiring = Instant::now();
        let output = match g.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                g.surface.configure(&g.renderer.device, &g.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(error) => return Err(format!("surface rendering failed: {error}")),
        };
        let acquire_us = acquiring.elapsed().as_micros() as u64;
        let sequence = self.presented + 1;
        let slot = g.timer.as_mut().and_then(|t| t.begin(sequence));
        let mut encoder = g
            .renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Motion Studio composition and presentation"),
            });
        let timestamps = slot.map(|i| g.timer.as_ref().unwrap().writes(i, 0));
        let encoded = if self.preview.mode == PreviewMode::High {
            g.renderer.encode(
                &self.scene,
                &g.scratch.view,
                rw,
                rh,
                &mut encoder,
                timestamps,
            )
        } else {
            g.renderer.encode_preview(
                &self.scene,
                &g.scratch.view,
                rw,
                rh,
                &mut encoder,
                timestamps,
            )
        };
        if let Err(error) = encoded {
            if let (Some(timer), Some(i)) = (&mut g.timer, slot) {
                timer.cancel_unsubmitted(i);
            }
            return Err(error.to_string());
        }
        let view = output.texture.create_view(&Default::default());
        let scale = (g.config.width as f32 / self.scene.width as f32)
            .min(g.config.height as f32 / self.scene.height as f32);
        let vw = self.scene.width as f32 * scale;
        let vh = self.scene.height as f32 * scale;
        g.presenter.encode(
            &mut encoder,
            &view,
            slot.map(|i| g.timer.as_ref().unwrap().writes(i, 1)),
            Some([
                (g.config.width as f32 - vw) / 2.0,
                (g.config.height as f32 - vh) / 2.0,
                vw,
                vh,
            ]),
            self.scene.background,
        );
        if let Some(i) = slot {
            g.timer.as_ref().unwrap().resolve(i, &mut encoder);
        }
        let submitting = Instant::now();
        g.renderer.queue.submit(Some(encoder.finish()));
        let submit_us = submitting.elapsed().as_micros() as u64;
        if let Some(i) = slot {
            g.timer.as_ref().unwrap().map(i);
        }
        g.renderer.check_health().map_err(|e| e.to_string())?;
        let presenting = Instant::now();
        output.present();
        let present_call_us = presenting.elapsed().as_micros() as u64;
        self.presented += 1;
        let render_wall_us = began.elapsed().as_micros() as u64;
        self.last_cpu_us = render_wall_us
            .saturating_sub(acquire_us)
            .saturating_sub(present_call_us);
        if let Some(recorder) = &mut self.recorder {
            recorder.record(FrameMeasurement {
                sequence,
                frame,
                elapsed_us: recorder.elapsed_us(),
                cpu_prepare_us: self.last_cpu_us,
                acquire_us,
                submit_us,
                present_call_us,
                render_wall_us,
                render_width: rw,
                render_height: rh,
                preview_fps: tier.fps(),
                gpu: None,
            });
        }
        self.last_presented_frame = Some(frame);
        self.last_presented_revision = self.engine.revision();
        self.last_presented_view_revision = self.view_revision;
        let previous_tier = self.preview.tier();
        self.preview.observe_render(
            self.last_cpu_us,
            gpu_work_us,
            acquire_us.saturating_add(present_call_us),
        );
        if previous_tier != self.preview.tier() {
            self.view_revision += 1;
        }
        g.renderer
            .prefetch_scene_assets(image_resolution)
            .map_err(|e| e.to_string())?;
        self.last_error = None;
        Ok(true)
    }

    pub fn sample(&mut self) -> Result<()> {
        let result = self
            .scene
            .sample(
                self.engine.project(),
                self.frame,
                if self.observing {
                    Some(&self.observer)
                } else {
                    None
                },
            )
            .map_err(|e| e.to_string());
        if let Err(error) = &result {
            self.last_error = Some(error.clone());
        } else if self
            .last_error
            .as_ref()
            .is_some_and(|e| e.starts_with("expression "))
        {
            self.last_error = None;
        }
        result
    }

    pub fn composition_error(&self, code: &str, message: &str, details: Value) -> String {
        aem_core::composition::CompositionError {
            code: code.into(),
            composition: self.engine.project().composition_id.clone(),
            message: message.into(),
            details,
        }
        .to_string()
    }
}

/// Adapt the host's audio fallback to the callback shape `aem-media` expects.
fn audio_decoder(platform: &Arc<dyn Platform>) -> aem_media::DecodeAudio {
    let platform = platform.clone();
    Arc::new(
        move |path: &std::path::Path,
              pcm: &std::path::Path,
              selected: Option<u32>,
              limit: u64,
              check: &mut dyn FnMut(f64) -> aem_media::Result<()>| {
            platform.decode_audio(path, pcm, selected, limit, check)
        },
    )
}

/// Run `operation` against a live session on its owning thread.
pub fn with_session<T>(id: i64, operation: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let session = registry.get_mut(&id).ok_or("native session is closed")?;
    session.check_thread()?;
    operation(session)
}

/// Aggregate resource report for every session rooted under `directory`.
pub fn resource_info(directory: &std::path::Path) -> Result<Value> {
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    let registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let mut count = 0u64;
    let mut graphics = 0u64;
    let mut assets = 0u64;
    let mut targets = 0u64;
    for session in registry.values().filter(|s| {
        s.root
            .canonicalize()
            .is_ok_and(|path| path.starts_with(&root))
    }) {
        count += 1;
        if let Some(g) = &session.graphics {
            graphics += 1;
            assets += g.renderer.texture_bytes();
            targets += g.scratch.texture_bytes();
        }
    }
    Ok(json!({"sessions":count,"graphics":graphics,"assetTextureBytes":assets,"renderTargetBytes":targets,
        "scope":"Application-owned sessions and textures in the requested project directory; not driver/system allocations"}))
}

/// Create a session and register it. Returns `0` on failure and stores the
/// reason for [`creation_error`], matching the Android bridge contract.
pub fn open(root: PathBuf, project: Project, platform: Arc<dyn Platform>) -> Result<i64> {
    let session = Session::new(project, root, platform)?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, session);
    Ok(id)
}

thread_local! {
    static CREATION_ERROR: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Reason the most recent [`open`] on this thread failed.
pub fn creation_error() -> String {
    CREATION_ERROR.with(|e| e.borrow().clone())
}

pub(crate) fn set_creation_error(error: String) {
    CREATION_ERROR.with(|e| *e.borrow_mut() = error);
}

/// Close a session and release its GPU resources.
pub fn close(id: i64) {
    let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
    if registry
        .get(&id)
        .is_some_and(|s| s.owner == thread::current().id())
    {
        if let Some(mut s) = registry.remove(&id) {
            s.detach();
        }
    }
}