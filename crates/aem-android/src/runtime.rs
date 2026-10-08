//! Session lifecycle, GPU resources, Surface rendering and frame sampling.
//! Java-facing adapters and their wire-format conversions live in runtime/.
use aem_core::{Command, Engine, Observer, Project, Scene};
use aem_render::{
    FrameMeasurement, FrameRecorder, GpuTimer, Presenter, PreviewMode, PreviewPolicy, RenderTarget,
    Renderer,
};
use bridge::*;
use buffers::BufferAccess;
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, WindowHandle,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    thread::{self, ThreadId},
    time::Instant,
};
#[path = "audio_decode.rs"]
mod audio_decode;
#[path = "runtime/media_audio.rs"]
mod audio_runtime;
#[path = "media_capabilities.rs"]
mod media_capabilities;
#[path = "video_decode.rs"]
mod video_decode;
#[path = "video_frames.rs"]
mod video_frames;
#[path = "runtime/media_video.rs"]
mod video_runtime;

#[path = "runtime/composition.rs"]
mod composition_runtime;

mod bridge;
mod buffers;
mod editing;
mod effects;
mod export;
mod geometry;
mod preview;
mod project;
mod snapshot;

type Result<T> = std::result::Result<T, String>;
static NEXT: AtomicI64 = AtomicI64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();
fn sessions() -> &'static Mutex<HashMap<i64, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct AndroidWindow(NativeWindow);
impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}
impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        // AndroidDisplayHandle carries no borrowed external display pointer.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

struct Graphics {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    scratch: RenderTarget,
    presenter: Presenter,
    config: wgpu::SurfaceConfiguration,
    _instance: wgpu::Instance,
    timer: Option<GpuTimer>,
}
struct Session {
    composition_contexts: HashMap<String, composition_runtime::CompositionContext>,
    composition_bundle: Vec<u8>,
    media_compositions: HashMap<String, String>,
    video_jobs: aem_media::VideoJobs,
    video_frames: video_frames::VideoFrames,
    audio_jobs: aem_media::AudioJobs,
    audio_mixer: Option<(u64, aem_media::AudioMixer)>,
    audio_pcm: Vec<f32>,
    engine: Engine,
    scene: Scene,
    geometry: aem_core::PlaneCompositor,
    observer: Observer,
    observing: bool,
    frame: f64,
    root: PathBuf,
    graphics: Option<Graphics>,
    owner: ThreadId,
    presented: u64,
    last_cpu_us: u64,
    render_attempts: u64,
    video_pending_attempts: u64,
    video_prepare_us: u64,
    video_upload_us: u64,
    last_error: Option<String>,
    last_presented_frame: Option<f64>,
    last_presented_revision: u64,
    view_revision: u64,
    last_presented_view_revision: u64,
    surface_epoch: u64,
    preview: PreviewPolicy,
    recorder: Option<FrameRecorder>,
    effects: aem_render::effect_plan::PlanBuilder,
    plugin_root: PathBuf,
    editor: Option<aem_core::plugin_editor::PluginEditorSession>,
    editor_token: String,
    editor_renderer: Option<Renderer>,
    editor_target: Option<aem_render::CaptureTarget>,
}
impl Session {
    fn replace_project(&mut self, engine: Engine, root: PathBuf) -> Result<()> {
        let audio_jobs = aem_media::AudioJobs::with_decoder(
            root.clone(),
            aem_media::Limits::default(),
            std::sync::Arc::new(audio_decode::decode),
        )?;
        let video_jobs = aem_media::VideoJobs::with_audio_decoder(
            root.clone(),
            std::sync::Arc::new(audio_decode::decode),
        )?;
        if let Some(g) = &mut self.graphics {
            g.renderer
                .replace_assets(engine.project(), &root)
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
        self.video_frames.clear();
        self.audio_mixer = None;
        self.root = root;
        self.frame = 0.0;
        self.observing = false;
        self.last_presented_frame = None;
        self.last_error = None;
        self.view_revision += 1;
        // Imported expression source must remain editable even when frame zero fails.
        // Strict preview/export sampling still reports the expression error.
        if let Err(error) = self.sample() {
            self.last_error = Some(error);
        }
        Ok(())
    }
    fn new(project: Project, root: PathBuf) -> Result<Self> {
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
                scene.sample(&base, 0.0, None).map_err(|e| e.to_string())?;
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
        Ok(Self {
            composition_contexts: Default::default(),
            composition_bundle: Vec::new(),
            media_compositions: Default::default(),
            video_jobs: aem_media::VideoJobs::with_audio_decoder(
                root.clone(),
                std::sync::Arc::new(audio_decode::decode),
            )?,
            video_frames: video_frames::VideoFrames::default(),
            audio_jobs: aem_media::AudioJobs::with_decoder(
                root.clone(),
                aem_media::Limits::default(),
                std::sync::Arc::new(audio_decode::decode),
            )?,
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
        })
    }
    fn check_thread(&self) -> Result<()> {
        if self.owner != thread::current().id() {
            Err("native session must run on its owning worker thread".into())
        } else {
            Ok(())
        }
    }
    fn detach(&mut self) {
        self.video_frames.clear();
        if let Some(g) = self.graphics.take() {
            g.renderer.device.poll(wgpu::Maintain::Wait);
            drop(g);
        }
    }
    fn attach(&mut self, window: NativeWindow, width: u32, height: u32) -> Result<()> {
        self.detach();
        // Vulkan surface creation can connect the Android buffer producer even
        // when no Vulkan adapter is available. Keep only one backend's surface
        // alive, otherwise the GLES fallback cannot connect the same window.
        let mut candidate = None;
        let mut failures = Vec::new();
        for backend in [wgpu::Backends::VULKAN, wgpu::Backends::GL] {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: backend,
                ..Default::default()
            });
            // Surface owns its window via the safe raw-window-handle wrapper.
            let surface = match instance.create_surface(AndroidWindow(window.clone())) {
                Ok(surface) => surface,
                Err(error) => {
                    failures.push(error.to_string());
                    continue;
                }
            };
            match pollster::block_on(Renderer::new_profiled(
                &instance,
                Some(&surface),
                wgpu::TextureFormat::Rgba8UnormSrgb,
                true,
            )) {
                Ok(renderer) => {
                    candidate = Some((instance, surface, renderer));
                    break;
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        let (instance, surface, mut renderer) = candidate
            .ok_or_else(|| format!("no Android surface backend: {}", failures.join("; ")))?;
        renderer.set_effect_registry(self.effects.registry.clone());
        let caps = surface.get_capabilities(&renderer.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("no supported surface pixel format")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                *caps.alpha_modes.first().ok_or("no surface alpha mode")?
            },
            view_formats: vec![],
        };
        surface.configure(&renderer.device, &config);
        renderer
            .synchronize_assets(self.engine.project(), &self.root)
            .map_err(|e| e.to_string())?;
        let (render_width, render_height) = self.preview.render_dimensions(
            self.engine.project().width,
            self.engine.project().height,
            width,
            height,
        );
        let scratch = renderer
            .render_target(render_width, render_height)
            .map_err(|e| e.to_string())?;
        let presenter = Presenter::new(&renderer, &scratch.view, format);
        // Auto quality needs GPU cost even when no diagnostic recording is active.
        // Only four reusable 32-byte timing buffers are read, never video pixels.
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
    fn render(&mut self, frame: f64) -> Result<bool> {
        let began = Instant::now();
        self.render_attempts += 1;
        self.frame = frame;
        self.sample()?;
        let tier = self.preview.tier();
        let Some(g) = &mut self.graphics else {
            return Ok(false);
        };
        g.renderer.retain_video_instances(&self.scene);
        let preparing = Instant::now();
        let frames = self.video_frames.prepare_scene(
            self.engine.project(),
            &self.root,
            &self.scene,
            frame,
        )?;
        self.video_prepare_us = preparing.elapsed().as_micros() as u64;
        let Some(frames) = frames else {
            self.video_pending_attempts += 1;
            self.video_upload_us = 0;
            return Ok(false);
        };
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
        let mut encoder =
            g.renderer
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
        self.last_error = None;
        Ok(true)
    }
    fn sample(&mut self) -> Result<()> {
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
}
fn with_session<T>(id: i64, operation: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let session = registry.get_mut(&id).ok_or("native session is closed")?;
    session.check_thread()?;
    operation(session)
}
