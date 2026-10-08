use aem_core::{Command, Engine, Observer, Project, Scene};
use aem_render::{
    FrameMeasurement, FrameRecorder, GpuTimer, Presenter, PreviewMode, PreviewPolicy, RenderTarget,
    Renderer,
};
use jni::{
    objects::{JByteBuffer, JClass, JObject, JString},
    sys::{jboolean, jbyteArray, jdouble, jint, jlong, jstring},
    JNIEnv,
};
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, WindowHandle,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    thread::{self, ThreadId},
    time::Instant,
};
#[path = "audio_runtime.rs"]
mod audio_runtime;
#[path = "video_decode.rs"]
mod video_decode;
#[path = "audio_decode.rs"]
mod audio_decode;
#[path = "media_capabilities.rs"]
mod media_capabilities;
#[path = "video_frames.rs"]
mod video_frames;
#[path = "video_runtime.rs"]
mod video_runtime;

#[path = "composition_runtime.rs"]
mod composition_runtime;

type Result<T> = std::result::Result<T, String>;
static NEXT: AtomicI64 = AtomicI64::new(1);
static SESSIONS: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();
thread_local! {static CREATION_ERROR:RefCell<String>=const {RefCell::new(String::new())};}
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
    composition_contexts: HashMap<String,composition_runtime::CompositionContext>,
    composition_bundle:Vec<u8>,
    media_compositions:HashMap<String,String>,
    video_jobs: aem_media::VideoJobs,
    video_frames: video_frames::VideoFrames,
    audio_jobs: aem_media::AudioJobs,
    package_jobs: aem_media::PackageJobs,
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
        let audio_jobs = aem_media::AudioJobs::with_decoder(root.clone(), aem_media::Limits::default(), std::sync::Arc::new(audio_decode::decode))?;
        let video_jobs = aem_media::VideoJobs::with_audio_decoder(root.clone(), std::sync::Arc::new(audio_decode::decode))?;
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
        self.composition_contexts.clear();self.media_compositions.clear();
        self.engine = engine;
        self.audio_jobs = audio_jobs;
        self.video_jobs = video_jobs;
        // Keep frozen package exports queryable across project replacement.
        // Their open source handles and output paths belong to the old snapshot.
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
            composition_contexts:Default::default(),composition_bundle:Vec::new(),media_compositions:Default::default(),
            video_jobs: aem_media::VideoJobs::with_audio_decoder(root.clone(), std::sync::Arc::new(audio_decode::decode))?,
            video_frames: video_frames::VideoFrames::default(),
            audio_jobs: aem_media::AudioJobs::with_decoder(root.clone(), aem_media::Limits::default(), std::sync::Arc::new(audio_decode::decode))?,
            package_jobs: aem_media::PackageJobs::default(),
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
    fn editor_operation(&mut self, v: &Value) -> Result<Value> {
        let op = v["op"].as_str().ok_or("missing editor operation")?;
        if op == "editor_open" {
            if self.editor.is_some() {
                return Err("close the current plugin editor first".into());
            }
            let object = v["object"].as_u64().ok_or("missing editor layer")?;
            let instance = v["instance"].as_u64().ok_or("missing editor effect")?;
            let editor = aem_core::plugin_editor::PluginEditorSession::open(
                self.engine.project(),
                &self.effects.registry,
                object,
                instance,
            )
            .map_err(|e| e.to_string())?;
            let package = self
                .effects
                .registry
                .resolve(
                    &editor.dependency.plugin,
                    &editor.dependency.version,
                    &editor.dependency.hash,
                )
                .map_err(|e| e.to_string())?;
            let definition = package
                .manifest
                .effects
                .iter()
                .find(|d| d.id == editor.effect)
                .ok_or("editor definition missing")?;
            let state = editor
                .state(&self.engine, self.frame.floor() as u32)
                .map_err(|e| e.to_string())?;
            self.editor_token = format!(
                "editor-{}-{}",
                NEXT.fetch_add(1, Ordering::Relaxed),
                self.engine.revision()
            );
            self.editor = Some(editor);
            return Ok(
                json!({"protocol":1,"token":self.editor_token,"definition":definition,"state":state}),
            );
        }
        if self.editor.is_none() || v["token"].as_str() != Some(self.editor_token.as_str()) {
            return Err("plugin editor session is stale or missing".into());
        }
        let editor = self.editor.as_ref().unwrap();
        let package = self
            .effects
            .registry
            .resolve(
                &editor.dependency.plugin,
                &editor.dependency.version,
                &editor.dependency.hash,
            )
            .map_err(|e| e.to_string())?;
        match op {
            "editor_asset" => {
                let path = v["path"].as_str().ok_or("missing editor asset path")?;
                let definition = package
                    .manifest
                    .effects
                    .iter()
                    .find(|d| d.id == editor.effect)
                    .and_then(|d| d.editor.as_ref())
                    .ok_or("editor definition missing")?;
                if !definition.files.iter().any(|p| p == path) {
                    return Err("asset is outside this plugin editor".into());
                }
                let bytes = package.files.get(path).ok_or("editor asset missing")?;
                Ok(json!({"mime":aem_effects::editor_mime(path),"base64":encode_base64(bytes)}))
            }
            "editor_close" => {
                let mut editor = self.editor.take().unwrap();
                editor
                    .close(&mut self.engine, v["commit"].as_bool().unwrap_or(false))
                    .map_err(|e| e.to_string())?;
                self.editor_renderer = None;
                self.editor_target = None;
                self.editor_token.clear();
                self.sample()?;
                Ok(self.snapshot())
            }
            "editor_message" => {
                if v["message"]["op"] == "preview" {
                    return self.editor_preview(&v["message"]);
                }
                let request: aem_core::plugin_editor::EditorRequest =
                    serde_json::from_value(v["message"].clone()).map_err(|e| e.to_string())?;
                let mut state = self
                    .editor
                    .as_mut()
                    .unwrap()
                    .request(&mut self.engine, self.frame.floor() as u32, request)
                    .map_err(|e| e.to_string())?;
                if let Err(error) = self.sample() {
                    state["render_error"] = json!(error);
                }
                Ok(state)
            }
            _ => Err("unknown editor operation".into()),
        }
    }
    fn editor_preview(&mut self, message: &Value) -> Result<Value> {
        self.editor
            .as_ref()
            .ok_or("editor session missing")?
            .state(&self.engine, self.frame.floor() as u32)
            .map_err(|e| e.to_string())?;
        let width = message["width"].as_u64().ok_or("missing preview width")?;
        let height = message["height"].as_u64().ok_or("missing preview height")?;
        if !(1..=512).contains(&width) || !(1..=512).contains(&height) {
            return Err("editor preview must be 1..512 pixels per dimension".into());
        }
        self.sample()?;
        if self.editor_renderer.is_none() {
            let mut renderer =
                pollster::block_on(Renderer::headless()).map_err(|e| e.to_string())?;
            renderer.set_effect_registry(self.effects.registry.clone());
            renderer.configure_assets(self.engine.project(), &self.root)
                .map_err(|e|e.to_string())?;
            self.editor_renderer = Some(renderer);
        }
        let renderer = self.editor_renderer.as_mut().unwrap();
        renderer.prepare_scene_assets(&self.scene, aem_render::image_resources::Resolution::Preview(1024), false)
            .map_err(|e| e.to_string())?;
        renderer
            .preflight_effects(&self.scene, width as u32, height as u32)
            .map_err(|e| e.to_string())?;
        if self
            .editor_target
            .as_ref()
            .is_none_or(|t| t.width != width as u32 || t.height != height as u32)
        {
            self.editor_target = Some(
                renderer
                    .capture_target(width as u32, height as u32)
                    .map_err(|e| e.to_string())?,
            );
        }
        let (pixels, stats) = renderer
            .capture(&self.scene, self.editor_target.as_ref().unwrap())
            .map_err(|e| e.to_string())?;
        let mut png = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut png),
            &pixels,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
        Ok(
            json!({"width":width,"height":height,"png":encode_base64(&png),"revision":self.engine.revision(),"frame":self.frame,"instances":{"alive":stats.particles_alive,"visible":stats.particles_visible,"culled":stats.particles_culled,"upload_bytes":stats.instance_upload_bytes}}),
        )
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
                Err(error) => { failures.push(error.to_string()); continue; }
            };
            match pollster::block_on(Renderer::new_profiled(
                &instance, Some(&surface), wgpu::TextureFormat::Rgba8UnormSrgb, true,
            )) {
                Ok(renderer) => { candidate = Some((instance, surface, renderer)); break; }
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
            .configure_assets(self.engine.project(), &self.root)
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
        g.renderer.set_image_prefetch(aem_render::image_resources::upcoming_assets(self.engine.project(), frame));
        let image_resolution = if self.preview.mode == PreviewMode::High {
            aem_render::image_resources::Resolution::Full
        } else {
            aem_render::image_resources::Resolution::Preview(aem_render::image_resources::MAX_PREVIEW_EDGE)
        };
        let images_ready = g.renderer.prepare_scene_assets(&self.scene, image_resolution, true).map_err(|e| e.to_string())?;
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
        let mut encoder =
            g.renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Motion Studio composition and presentation"),
                });
        let timestamps = slot.map(|i| g.timer.as_ref().unwrap().writes(i, 0));
        let encoded = if self.preview.mode == PreviewMode::High {
            g.renderer.encode(&self.scene, &g.scratch.view, rw, rh, &mut encoder, timestamps)
        } else {
            g.renderer.encode_preview(&self.scene, &g.scratch.view, rw, rh, &mut encoder, timestamps)
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
        g.renderer.prefetch_scene_assets(image_resolution).map_err(|e| e.to_string())?;
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
    fn preview_info(&self) -> Value {
        let tier = self.preview.tier();
        let p = self.engine.project();
        let (sw, sh) = self.graphics.as_ref().map_or((p.width, p.height), |g| (g.config.width, g.config.height));
        let (width, height) = self.preview.render_dimensions(p.width, p.height, sw, sh);
        json!({"mode":self.preview.mode.name(),"tier":tier.name(),"width":width,"height":height,"fps":tier.fps(),
            "effectResolution":if self.preview.mode == PreviewMode::High {"full_layer"} else {"projected_2d"},
            "imageResolution":if self.preview.mode == PreviewMode::High {"original"} else {"proxy_max_2048"},
            "imageDecodes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_decodes),
            "imageProxyCacheHits":self.graphics.as_ref().map_or(0,|g|g.renderer.image_proxy_cache_hits),
            "imageUploadBytes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_upload_bytes),
            "imageMemoryCacheHits":self.graphics.as_ref().map_or(0,|g|g.renderer.image_memory_cache_hits),
            "imageIdleBytes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_idle_bytes()),
            "imageIdleBudgetBytes":aem_render::image_resources::IDLE_TEXTURE_BYTES,
            "imagePrefetches":self.graphics.as_ref().map_or(0,|g|g.renderer.image_prefetches),
            "imagePrefetchSeconds":aem_render::image_resources::PREFETCH_SECONDS,
            "surfaceBounded":self.preview.mode != PreviewMode::High,"gpuTimingActive":self.graphics.as_ref().is_some_and(|g|g.timer.is_some()),
            "profiling":self.recorder.is_some(),"gpuTimestampSupported":self.graphics.as_ref().is_some_and(|g|g.renderer.device.features().contains(wgpu::Features::TIMESTAMP_QUERY)),
            "video":self.video_info()})
    }
    fn video_info(&self) -> Value {
        let mut info = self.video_frames.metrics();
        info["renderAttempts"] = json!(self.render_attempts);
        info["pendingAttempts"] = json!(self.video_pending_attempts);
        info["lastPrepareUs"] = json!(self.video_prepare_us);
        info["lastUploadUs"] = json!(self.video_upload_us);
        if let Some(g) = &self.graphics {
            info["uploadBytes"] = json!(g.renderer.video_upload_bytes);
            info["uploads"] = json!(g.renderer.video_uploads);
            info["gpuConversions"] = json!(g.renderer.video_gpu_conversions);
        }
        info
    }
    fn snapshot(&self) -> Value {
        let original = self.engine.project();
        let p = self.scene.sampled_project(original);
        let f = self.frame;
        let camera = json!({"position":p.camera.position_at(f),"target":p.camera.target.sample(f),
            "fov":p.camera.fov.sample(f).clamp(10.0,120.0),"roll":p.camera.roll.sample(f),"radius":p.camera.radius.sample(f).clamp(1.0,10_000_000.0),
            "azimuth":p.camera.azimuth.sample(f),"elevation":p.camera.elevation.sample(f).clamp(-89.0,89.0)});
        let layers: Vec<_> = p
            .layers
            .iter()
            .map(|l| {
                let local = l.local_frame(f);
                json!({"id":l.id,"position":l.transform.position.sample(local),
            "rotation":l.transform.rotation.sample(local),"scale":l.transform.scale.sample(local),
            "opacity":l.transform.opacity.sample(local).clamp(0.0,1.0),"active":l.active(f,p.frames),"three_d":l.three_d})
            })
            .collect();
        let mut projected: Vec<_> = self
            .scene
            .layers
            .iter()
            .filter_map(|layer| {
                let mvp = layer.view_projection * layer.model;
                let corners = [[-0.5, 0.5], [0.5, 0.5], [0.5, -0.5], [-0.5, -0.5]].map(|[x, y]| {
                    mvp.x_axis * (x * layer.size[0]) + mvp.y_axis * (y * layer.size[1]) + mvp.w_axis
                });
                if corners.iter().any(|c| !c.is_finite() || c.w <= 0.0) {
                    return None;
                }
                let anchor = p
                    .layers
                    .iter()
                    .find(|l| l.id == layer.id)
                    .and_then(|l| self.scene.project_node(l.id));
                Some(
                    json!({"id":layer.id,"anchor":anchor,"corners":corners.map(|c|[
                (c.x/c.w*0.5+0.5)*p.width as f32,
                (0.5-c.y/c.w*0.5)*p.height as f32])}),
                )
            })
            .collect();
        for layer in p
            .layers
            .iter()
            .filter(|l| matches!(l.content, aem_core::Content::Null) && l.active(f, p.frames))
        {
            if let Some(point) = self.scene.project_node(layer.id) {
                let [x, y, _] = point;
                if x.is_finite() && y.is_finite() {
                    projected.push(json!({"id":layer.id,"anchor":point,"null":true,
                    "corners":[[x-12.0,y-12.0],[x+12.0,y-12.0],[x+12.0,y+12.0],[x-12.0,y+12.0]]}));
                }
            }
        }
        let vector_layers: Vec<_> = self.scene.layers.iter().filter_map(|layer| {
            let vector=layer.vector.as_ref()?;
            let stored=p.layers.iter().find(|l|l.id==layer.id).and_then(|l|match &l.content {
                aem_core::Content::Vector{vector}=>Some(&vector.source),_=>None,
            });
            let paths:Vec<_>=vector.paths.iter().enumerate().map(|(i,path)| {
                let original_path=match stored {Some(aem_core::vector::VectorSource::Paths{paths})=>paths.get(i),_=>None};
                let nodes:Vec<_>=path.nodes.iter().enumerate().map(|(j,geometry)|json!({"id":original_path.and_then(|p|p.nodes.get(j)).map_or(j as u64+1,|n|n.id),"geometry":geometry})).collect();
                json!({"id":original_path.map_or(i as u64+1,|p|p.id),"closed":path.closed,"nodes":nodes})
            }).collect();
            let parameters=match stored {
                Some(aem_core::vector::VectorSource::Shape{parameters,..})=>{
                    let offset=p.layers.iter().find(|l|l.id==layer.id).map_or(0,|l|l.clip(p.frames).offset_frame);
                    parameters.iter().map(|(name,track)|(name.clone(),json!(track.sample(f-f64::from(offset))))).collect::<serde_json::Map<_,_>>()
                },_=>serde_json::Map::new(),
            };
            Some(json!({"id":layer.id,"canvas_size":layer.source_size,"source_rect":layer.source_rect,"mvp":(layer.view_projection*layer.model).to_cols_array(),"paths":paths,"parameters":parameters,"fill":vector.fill,"stroke":vector.stroke.map(|s|json!({"color":s.0,"width":s.1,"cap":s.2,"join":s.3,"miter_limit":s.4}))}))
        }).collect();
        let camera_properties: Vec<&str> = if !p.camera.created {
            vec![]
        } else if p.camera.mode == aem_core::CameraMode::Position {
            vec!["position", "target"]
        } else {
            vec!["target"]
        };
        let video_capabilities = json!({
            "container":"MP4","codec":"H.264 baseline/main/high, 8-bit 4:2:0 SDR",
            "containers":["MP4","MOV","3GP","Matroska","WebM"],
            "codecs":["H.264","H.265 Main","VP8","VP9 profile 0"],
            "profile":"8-bit 4:2:0 SDR","device_query":"media_capabilities",
            "max_pixels":aem_core::MAX_VIDEO_PIXELS,"max_dimension":aem_core::MAX_VIDEO_DIMENSION,
            "max_fps":aem_core::MAX_VIDEO_FPS,"max_index_frames":aem_core::MAX_VIDEO_FRAMES,
            "preserves_source_aspect_ratio":true,"arbitrary_aspect_ratio":true,"square_pixels_only":true,
            "input_is_independent_of_composition":true,"max_duration_seconds":3600,
            "async_frames":true,"frame_format":"rgba8","decoder":"Android MediaCodec",
            "max_decoders":4,"default_with_audio":true,"frozen_source_frames":true,"legacy_gles_export_integrated":true
        });
        json!({"project":original,"root":self.root.to_string_lossy(),"frame":f,"revision":self.engine.revision(),"canUndo":self.engine.can_undo(),
            "main_composition":"comp-main","composition":p.composition_id,"compositions":p.composition_list(),"has_audio":p.audio_voices().is_ok_and(|v|!v.is_empty()),
            "composition_context":self.composition_context_snapshot(),
            "vector_layers":vector_layers,
            "capabilities":{"adjustment_layers":{"supported":true,"command":"add_adjustment","composite":"lower_layers","mask":"transformed_rectangle","background":"excluded","three_d":false},"vector_drawing":{"supported":true,"protocol":1,"command":"vector","coordinates":"centered_canvas_pixels_y_down","max_paths":aem_core::vector::MAX_PATHS,"max_nodes":aem_core::vector::MAX_NODES,"fill_rules":["non_zero","even_odd"],"stroke_caps":["butt","round","square"],"stroke_joins":["miter","round","bevel"],"shape_catalog":aem_core::vector::shape_catalog()},"scene_effects":{"sdk_version":aem_effects::SDK_VERSION,"plugin_editor_protocol":1,"max_particles_per_effect":20000,"max_sprites_per_frame":65536,"occlusion":"source_alpha_planes","simulation":"analytic_world_birth_or_legacy_local_space","particle_birth_history":true,"particle_history_expressions":false,"particle_rate_animation":false},"native_plugin_ui":{"supported":true,"protocol":1,"slots":["preview","timeline","parameters","layer_source","image_sprite","seed","transform","note"],"preview":"shared_wgpu_surface","timeline":"shared_composition_clock"},"property_expressions":{"supported":true,"profile":aem_core::EXPRESSION_PROFILE,"engine":"QuickJS-NG","source_max_bytes":8192,"max_expressions":aem_core::MAX_EXPRESSIONS,"cross_property_references":false,"opacity_unit":"percent"},"layer_clips":true,"layer_3d":{"supported":true,"default":false,"activation":"explicit","command":"set_layer_3d"},
                "planar_intersections":{"supported":true,"method":"bsp","geometry_api":"sampleGeometryInto","max_batches":8192,"max_vertices":65536},
                "separate_dimensions":{"supported":true,"activation":"explicit",
                "layer_properties":["position","rotation","scale"],"camera_properties":camera_properties,"axes":["x","y","z"]},
                "composition":{"fps_range":[1,aem_core::MAX_COMPOSITION_FPS],"fps_presets":[24,25,30,50,60,90,120,144,240],"fps_type":"integer"},
                "project_package":audio_runtime::package_limits(),"multiple_compositions":true,"precompose":true,"composition_api":{"version":1,"project_format":8,"max_compositions":32,"max_depth":8,"max_instances":64,"reference_3d":true,"collapse_transformations":false,"precompose_modes":["move_all_attributes"],"precompose_range":["composition"],"precompose_contiguous":true,"precompose_3d":false,"history_scope":"project"},"video_import":true,"audio_import":true,"model_import":false,"prerender":false,
                "video":video_capabilities,
                "audio":{"supported_formats":["M4A/AAC-LC/ALAC","MP3","FLAC","Ogg/Vorbis/Opus","ADTS/AAC","WAV/PCM8/16/24/32/float","AIFF"],"sample_rates":[8000,11025,12000,16000,22050,24000,32000,44100,48000,88200,96000,176400,192000],"sample_rate_range":[8000,192000],"channels":[1,2],"device_query":"media_capabilities",
                "output_rate":48000,"output_channels":2,"pcm":"f32le_interleaved","waveform_bucket_us":10000,
                "source_limit_bytes":aem_core::storage::MAX_MEDIA_ASSET,"source_duration_limit_seconds":3600,
                "pcm_block_limit_frames":aem_media::MAX_BLOCK_FRAMES,"async_import":true,"ui_playback_integrated":true,"mp4_audio_mux_integrated":true}},
            "canRedo":self.engine.can_redo(),"observing":self.observing,"sampledCamera":camera,
            "sampledLayers":layers,"timeline_layers":original.timeline_layers(f),
            "timeline_camera":{"position":original.camera.position.timeline(0),"target":original.camera.target.timeline(0)},
            "projectedLayers":projected,"presented":self.presented,"cpuPrepareUs":self.last_cpu_us,
            "renderError":self.last_error,"effectErrors":self.graphics.as_ref().map(|g|&g.renderer.effect_diagnostics),
            "sampledEffects":self.scene.effects.iter().map(|e|json!({"layer":e.layer,"instance":e.instance,"values":e.param_ids.iter().enumerate().map(|(i,id)|(id.clone(),json!(e.values[i]))).collect::<serde_json::Map<String,Value>>(),"curve_lut":e.lut.map(|i|&self.scene.curve_luts[i][..])})).collect::<Vec<_>>(),"lastPresentedFrame":self.last_presented_frame,
            "lastPresentedRevision":self.last_presented_revision,"viewRevision":self.view_revision,
            "lastPresentedViewRevision":self.last_presented_view_revision,"surfaceEpoch":self.surface_epoch,
            "diagnosticsEnabled":cfg!(feature="diagnostics"),
            "observationView":match self.observer.view {aem_core::ObservationView::Free=>"free",aem_core::ObservationView::Top=>"top",aem_core::ObservationView::Side=>"side"},
            "preview":self.preview_info(),
            "graphics":self.graphics.as_ref().map(|g|json!({"width":g.config.width,"height":g.config.height,"renderWidth":g.scratch.width,"renderHeight":g.scratch.height,"renderTargetBytes":g.scratch.texture_bytes(),"previewImageReadbackBytes":0,"adapter":g.renderer.adapter_info.name,
                "backend":format!("{:?}",g.renderer.adapter_info.backend),"textureBytes":g.renderer.texture_bytes()}))})
    }
}
fn with_session<T>(id: i64, operation: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
    let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
    let session = registry.get_mut(&id).ok_or("native session is closed")?;
    session.check_thread()?;
    operation(session)
}
fn string_result(env: &mut JNIEnv<'_>, operation: impl FnOnce() -> Result<Value>) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    let value = match result {
        Ok(Ok(value)) => json!({"ok":true,"data":value}),
        Ok(Err(error)) => json!({"ok":false,"error":error,"error_detail":error.strip_prefix("composition_error:").and_then(|v|serde_json::from_str::<Value>(v).ok())}),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown native error".into());
            json!({"ok":false,"error":reason.chars().take(2048).collect::<String>()})
        }
    };
    env.new_string(value.to_string())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let a = c[0] as usize;
        let b = c.get(1).copied().unwrap_or(0) as usize;
        let d = c.get(2).copied().unwrap_or(0) as usize;
        out.push(TABLE[a >> 2] as char);
        out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if c.len() > 1 {
            TABLE[((b & 15) << 2) | (d >> 6)] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            TABLE[d & 63] as char
        } else {
            '='
        });
    }
    out
}
fn read_string(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Result<String> {
    env.get_string(text)
        .map(|s| s.into())
        .map_err(|e| e.to_string())
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_plugin(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    request: JString,
) -> jstring {
    let request = read_string(&mut env, &request);
    string_result(&mut env, || {
        with_session(id, |s| {
            let request = request?;
            if request.len() > 256 * 1024 {
                return Err("plugin request exceeds 256 KiB".into());
            }
            let v: Value = serde_json::from_str(&request).map_err(|e| e.to_string())?;
            if v.get("composition").is_some_and(|v|v.as_str()!=Some(s.engine.project().composition_id.as_str())){return Err(s.composition_error("context_mismatch","Open the requested composition before plugin access",json!({})));}
            if v["op"].as_str().is_some_and(|op| op.starts_with("editor_")) {
                return s.editor_operation(&v);
            }
            let text = |name: &str| v[name].as_str().ok_or_else(|| format!("missing {name}"));
            if s.editor.as_ref().is_some_and(|e| e.gesture) && v["op"] != "catalogue" {
                return Err(
                    "finish the plugin editor gesture before other plugin operations".into(),
                );
            }
            let registry = &mut s.effects.registry;
            match text("op")? {
                "catalogue" => {
                    return Ok(
                        json!({"packages":registry.packages.iter().map(|(key,p)|json!({"manifest":p.manifest,"hash":p.hash,"enabled":!registry.disabled.contains(key)})).collect::<Vec<_>>(),"errors":registry.diagnostics}),
                    )
                }
                "install" => {
                    registry
                        .install(&s.plugin_root, &PathBuf::from(text("path")?))
                        .map_err(|e| e.to_string())?;
                }
                "enable" => registry
                    .enable(
                        &s.plugin_root,
                        text("plugin")?,
                        text("version")?,
                        text("hash")?,
                        v["enabled"].as_bool().ok_or("missing enabled")?,
                    )
                    .map_err(|e| e.to_string())?,
                "uninstall" => registry
                    .uninstall(
                        &s.plugin_root,
                        text("plugin")?,
                        text("version")?,
                        text("hash")?,
                    )
                    .map_err(|e| e.to_string())?,
                "add" | "upgrade" => {
                    let p = registry
                        .resolve(text("plugin")?, text("version")?, text("hash")?)
                        .map_err(|e| e.to_string())?;
                    let def = p
                        .manifest
                        .effects
                        .iter()
                        .find(|d| Some(d.id.as_str()) == v["effect"].as_str())
                        .ok_or("unknown effect")?;
                    let object = v["object"].as_u64().ok_or("missing layer")?;
                    let layer = s
                        .engine
                        .project()
                        .layers
                        .iter()
                        .find(|l| l.id == object)
                        .ok_or("layer does not exist")?;
                    let upgrading = text("op")? == "upgrade";
                    if matches!(layer.content,aem_core::Content::Adjustment) && def.renderer!=aem_effects::RendererKind::Image {return Err("adjustment layers support image effects only".into());}
                    let instance = if upgrading {
                        v["instance"].as_u64().ok_or("missing instance")?
                    } else {
                        layer.effects.iter().map(|e| e.id).max().unwrap_or(0) + 1
                    };
                    let index = if upgrading {
                        layer
                            .effects
                            .iter()
                            .position(|e| e.id == instance)
                            .ok_or("instance does not exist")?
                    } else {
                        layer.effects.len()
                    };
                    let mut effect = aem_core::EffectInstance::new(
                        instance,
                        &p.manifest.id,
                        &p.manifest.version,
                        &p.hash,
                        def,
                        if matches!(layer.content,aem_core::Content::Adjustment) {[s.engine.project().width as f32,s.engine.project().height as f32]}else{layer.size},
                    );
                    let preserve = v.get("preserve_parameters")
                        .map(|v|v.as_bool().ok_or("preserve_parameters must be a boolean"))
                        .transpose()?.unwrap_or(false);
                    if upgrading && preserve {
                        let old = &layer.effects[index];
                        let old_package = registry.resolve(&old.plugin,&old.version,&old.hash)
                            .map_err(|e|e.to_string())?;
                        let old_def = old_package.manifest.effects.iter().find(|d|d.id==old.effect)
                            .ok_or("old effect definition is missing")?;
                        if old.plugin != p.manifest.id || old.effect != def.id
                            || old_def.params != def.params || old_def.renderer != def.renderer {
                            return Err("effect parameter contract differs; preserving parameters requires an explicit migration".into());
                        }
                        effect.params = old.params.clone();
                        effect.seed = old.seed;
                        effect.enabled = old.enabled;
                        effect.scene = old.scene.clone();
                    }
                    let mut cmds = Vec::new();
                    if upgrading {
                        cmds.push(Command::Effect {
                            object,
                            action: aem_core::EffectAction::Remove { effect: instance },
                        });
                    }
                    cmds.push(Command::Effect {
                        object,
                        action: aem_core::EffectAction::Insert { instance: effect },
                    });
                    if upgrading {
                        cmds.push(Command::Effect {
                            object,
                            action: aem_core::EffectAction::Move {
                                effect: instance,
                                index,
                            },
                        });
                    }
                    s.engine.apply_batch(cmds).map_err(|e| e.to_string())?;
                    s.sample()?;
                    return Ok(s.snapshot());
                }
                _ => return Err("unknown plugin operation".into()),
            }
            s.editor_renderer = None;
            s.editor_target = None;
            let next = registry.clone();
            s.effects.set_registry(next.clone());
            if let Some(g) = &mut s.graphics {
                g.renderer.set_effect_registry(next);
            }
            s.view_revision += 1;
            s.last_presented_frame = None;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_renderPlanInfo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.observing = false;
            s.sample()?;
            let p = s.engine.project();
            for id in p.reachable_compositions().map_err(|e|e.to_string())?{s.effects.preflight_project(&p.composition(&id).map_err(|e|e.to_string())?)?;}
            s.effects.synchronize_scene_alpha(&s.scene, p, &s.root)?;
            let assets = std::iter::once(0)
                .chain(p.assets.iter().map(|a| a.id))
                .collect::<Vec<_>>();
            for id in p.reachable_compositions().map_err(|e|e.to_string())?{let mut node=p.composition(&id).map_err(|e|e.to_string())?;for e in &mut node.expressions{e.enabled=false;}for c in &mut node.compositions{for e in &mut c.expressions{e.enabled=false;}}let mut scene=Scene::new(&node);scene.sample(&node,0.,None).map_err(|e|e.to_string())?;s.effects.synchronize(&scene)?;}
            s.effects.build(&s.scene, &assets, p.width, p.height, true)?;
            let programs=s.effects.programs.iter().map(|program|json!({"key":program.key,"glsl":program.shader.glsl,"sprite":program.shader.sprite,"additive":program.shader.additive,"resources":program.resources.iter().map(|path|{
   let bytes=&program.package.as_ref().unwrap().files[path];let dimensions=image::load_from_memory(bytes).map(|v|(v.width(),v.height())).unwrap_or((0,0));json!({"path":path,"width":dimensions.0,"height":dimensions.1})
  }).collect::<Vec<_>>() })).collect::<Vec<_>>();
            let count = p
                .layers
                .iter()
                .map(|l| l.effects.iter().filter(|e| e.enabled).count())
                .sum::<usize>();
            let passes = count * 10 + p.layers.len();
            let buffer_bytes = aem_render::effect_plan::HEADER_BYTES
                + p.layers.len() * 128
                + passes * (40 + aem_effects::shader::UNIFORM_BYTES)
                + count * 1024
                + aem_effects::MAX_SPRITES * 48 + 8192 * 12 + 65536 * 20 + aem_core::MAX_LAYERS*28 + 262144*24;
            Ok(
                json!({"version":aem_render::effect_plan::PLAN_VERSION,"composition_bundle_version":1,"composition_bundle_buffer_hint":131072,"has_video":!p.video_assets.is_empty(),"has_audio":p.audio_voices().is_ok_and(|v|!v.is_empty()),"programs":programs,"bufferBytes":buffer_bytes,"uniformBytes":aem_effects::shader::UNIFORM_BYTES,"passBytes":40,"spriteBytes":48,"assetBytes":4,"declaredAssetBytes":4+p.assets.iter().map(|a|u64::from(a.width)*u64::from(a.height)*4).sum::<u64>(),"imageResources":{"version":1,"demandLoading":true,"previewMaxEdge":aem_render::image_resources::MAX_PREVIEW_EDGE,"fullResolutionExport":true,"directBuffer":true}}),
            )
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_sampleRenderPlanInto(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jint,
    buffer: JByteBuffer,
) -> jint {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i32> {
        let capacity = env
            .get_direct_buffer_capacity(&buffer)
            .map_err(|e| e.to_string())?;
        let pointer = env
            .get_direct_buffer_address(&buffer)
            .map_err(|e| e.to_string())?;
        with_session(id, |s| {
            s.frame = f64::from(frame);
            s.sample()?;
            let p = s.engine.project();
            if !s.scene.nested.is_empty(){return Err("nested compositions require CompositionBridge.sampleFrameBundleInto".into());}
            let assets = std::iter::once(0)
                .chain(p.assets.iter().map(|a| a.id))
                .collect::<Vec<_>>();
            s.effects.synchronize_scene_alpha(&s.scene, p, &s.root)?;
            let result = (|| {
                let plan = s
                    .effects
                    .build(&s.scene, &assets, p.width, p.height, true)?;
                let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, capacity) };
                plan.write(&s.scene, bytes).map(|n| n as i32)
            })();
            if let Err(error) = &result {
                s.last_error = Some(error.clone());
            }
            result
        })
    }));
    result.ok().and_then(std::result::Result::ok).unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_pluginPixels(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    program: jint,
    resource: jint,
) -> jbyteArray {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_session(id, |s| {
            let program = s
                .effects
                .programs
                .get(program as usize)
                .ok_or("unknown program")?;
            let path = program
                .resources
                .get(resource as usize)
                .ok_or("unknown resource")?;
            let package = program.package.as_ref().ok_or("program has no resources")?;
            image::load_from_memory(&package.files[path])
                .map(|v| v.into_rgba8().into_raw())
                .map_err(|e| e.to_string())
        })
    }));
    match result {
        Ok(Ok(bytes)) => env
            .byte_array_from_slice(&bytes)
            .map_or(std::ptr::null_mut(), |v| v.into_raw()),
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_create(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
    project: JString,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i64> {
        let root = PathBuf::from(read_string(&mut env, &root)?);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let text = read_string(&mut env, &project)?;
        let project = if text.is_empty() {
            if root.join("project.json").exists() {
                aem_core::storage::load(&root).map_err(|e| e.to_string())?
            } else {
                Project::new(1080, 1920, 30, 180).map_err(|e| e.to_string())?
            }
        } else {
            serde_json::from_str(&text).map_err(|e| e.to_string())?
        };
        let session = Session::new(project, root)?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        sessions()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, session);
        Ok(id)
    }));
    match result {
        Ok(Ok(id)) => {
            CREATION_ERROR.with(|e| e.borrow_mut().clear());
            id
        }
        Ok(Err(error)) => {
            CREATION_ERROR.with(|e| *e.borrow_mut() = error);
            0
        }
        Err(_) => {
            CREATION_ERROR.with(|e| *e.borrow_mut() = "原生工程初始化失败".into());
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_curveGraph(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) -> jstring {
    let parsed = read_string(&mut env, &text);
    string_result(&mut env, || {
        let easing: aem_core::Easing = serde_json::from_str(&parsed?).map_err(|e| e.to_string())?;
        easing.validate().map_err(|e| e.to_string())?;
        let points: Vec<_> = (0..=160).map(|i| easing.sample(i as f64 / 160.0)).collect();
        Ok(
            json!({"points": points, "definitionScale": easing.curve.map_or(1.0, |c| c.definition_scale())}),
        )
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_colorCurveGraph(
    mut env:JNIEnv,_class:JClass,value:JString,
)->jstring {
    let value=read_string(&mut env,&value);
    string_result(&mut env,||{
        let value:aem_core::CurveObject=serde_json::from_str(&value?).map_err(|e|e.to_string())?;
        value.validate().map_err(|e|e.to_string())?;
        Ok(value.graph())
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_projectTemplate(
    mut env: JNIEnv,
    _class: JClass,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        if kind != 0 {
            return Err("unknown project template".into());
        }
        serde_json::to_value(Project::demo()).map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_creationError(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    CREATION_ERROR.with(|e| {
        env.new_string(e.borrow().as_str())
            .map_or(std::ptr::null_mut(), |s| s.into_raw())
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_resourceInfo(
    mut env: JNIEnv,
    _class: JClass,
    directory: JString,
) -> jstring {
    let parsed = read_string(&mut env, &directory);
    string_result(&mut env, || {
        let root = PathBuf::from(parsed?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
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
        Ok(
            json!({"sessions":count,"graphics":graphics,"assetTextureBytes":assets,"renderTargetBytes":targets,
            "scope":"Application-owned sessions and textures in the requested project directory; not driver/system allocations"}),
        )
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_state(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || with_session(id, |s| Ok(s.snapshot())))
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_command(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let parsed = read_string(&mut env, &text);
    string_result(&mut env, || {
        let text = parsed?;
        with_session(id, |s| {
            let commands = aem_core::parse_commands(&text).map_err(|e| e.to_string())?;
            let resources = commands.iter().any(Command::changes_resources);
            if resources {
                let mut check = Engine::new(s.engine.snapshot()).map_err(|e| e.to_string())?;
                check
                    .apply_batch(commands.clone())
                    .map_err(|e| e.to_string())?;
                aem_core::storage::validate_assets(&s.root, check.project())
                    .map_err(|e| e.to_string())?;
            }
            if s.editor.as_ref().is_some_and(|e| e.gesture) {
                return Err("finish the plugin editor gesture before ordinary edits".into());
            }
            let results = s.engine.apply_batch(commands).map_err(|e| e.to_string())?;
            if resources {
                s.effects.alpha_images.clear();
                s.editor_renderer = None;
                s.editor_target = None;
                if let Some(g) = &mut s.graphics {
                    if let Err(error) = g.renderer.configure_assets(s.engine.project(), &s.root) {
                        s.engine.undo().map_err(|e| e.to_string())?;
                        g.renderer.clear_assets();
                        let _ = g.renderer.configure_assets(s.engine.project(), &s.root);
                        return Err(error.to_string());
                    }
                }
            }
            // The edit has committed. A frame-specific expression failure is reported
            // in renderError without pretending the stored edit failed or losing it.
            if let Err(error) = s.sample() {
                s.last_error = Some(error);
            }
            let mut snapshot = s.snapshot();
            if let Some(result) = results.last() {
                snapshot["edit_result"] =
                    serde_json::to_value(result).map_err(|e| e.to_string())?;
                snapshot["edit_results"] =
                    serde_json::to_value(&results).map_err(|e| e.to_string())?;
            }
            Ok(snapshot)
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_drag(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    object: jlong,
    dx: jdouble,
    dy: jdouble,
    width: jint,
    height: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if width <= 0 || height <= 0 || !dx.is_finite() || !dy.is_finite() {
                return Err("invalid preview drag".into());
            }
            s.sample()?;
            let layer = s
                .engine
                .project()
                .layers
                .iter()
                .find(|l| l.id == object as u64)
                .ok_or("drag layer does not exist")?;
            if !layer.active(s.frame, s.engine.project().frames) {
                return Err("drag layer is outside its clip".into());
            }
            let position = layer.transform.position.sample(layer.local_frame(s.frame));
            let separated = layer.transform.position.axes.is_some();
            let world_position = s
                .scene
                .node_position(object as u64)
                .ok_or("object transform is unavailable")?;
            let offset = if layer.three_d {
                s.scene
                    .screen_translation(
                        world_position,
                        [dx as f32, dy as f32],
                        [width as u32, height as u32],
                    )
                    .map_err(|e| e.to_string())?
            } else {
                let p = s.engine.project();
                let scale = (width as f32 / p.width as f32).min(height as f32 / p.height as f32);
                [dx as f32 / scale, dy as f32 / scale, 0.0]
            };
            let offset =
                aem_core::scene_prefix_delta(s.engine.project(), object as u64, s.frame, offset)
                    .map_err(|e| e.to_string())?;
            let frame = s.frame.floor() as u32;
            let commands = if separated {
                [aem_core::Axis::X, aem_core::Axis::Y, aem_core::Axis::Z]
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| offset[*i].abs() > 1e-6)
                    .map(|(i, axis)| Command::SetComponent {
                        object: object as u64,
                        property: aem_core::Property::Position,
                        axis,
                        frame,
                        value: position[i] + offset[i],
                    })
                    .collect()
            } else {
                vec![Command::SetVector {
                    object: object as u64,
                    property: aem_core::Property::Position,
                    frame,
                    value: std::array::from_fn(|i| position[i] + offset[i]),
                }]
            };
            s.engine.apply_batch(commands).map_err(|e| e.to_string())?;
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_history(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    op: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let active=s.engine.project().composition_id.clone();
            match op {
                0 => {
                    s.engine.undo().map_err(|e| e.to_string())?;
                }
                1 => {
                    s.engine.redo().map_err(|e| e.to_string())?;
                }
                2 => s.engine.begin_gesture().map_err(|e| e.to_string())?,
                3 => s.engine.end_gesture(true).map_err(|e| e.to_string())?,
                4 => s.engine.end_gesture(false).map_err(|e| e.to_string())?,
                _ => return Err("invalid history operation".into()),
            }
            if !s.engine.gesture_active(){let target=if s.engine.project().composition_ids().contains(&active){active.as_str()}else{aem_core::MAIN_COMPOSITION};s.engine.activate_composition(target).map_err(|e|e.to_string())?;}
            s.frame=s.frame.min(f64::from(s.engine.project().frames-1));s.audio_mixer=None;s.video_frames.clear();
            if matches!(op,0|1|4) {
                s.effects.alpha_images.clear();
                s.editor_renderer=None;s.editor_target=None;
            }
            if let Some(g) = &mut s.graphics {
                g.renderer
                    .configure_assets(s.engine.project(), &s.root)
                    .map_err(|e| e.to_string())?;
            }
            if let Err(error) = s.sample() {
                s.last_error = Some(error);
            }
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_seek(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
                return Err("invalid frame".into());
            }
            s.frame = frame;
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_observe(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    enabled: jboolean,
    az: jdouble,
    el: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.observing = enabled != 0;
            s.view_revision += 1;
            if s.observer.view == aem_core::ObservationView::Free {
                s.observer
                    .orbit(az as f32, el as f32)
                    .map_err(|e| e.to_string())?;
            } else {
                s.observer
                    .pan(
                        az as f32 * 4.0,
                        el as f32 * 4.0,
                        s.engine.project().width,
                        s.engine.project().height,
                    )
                    .map_err(|e| e.to_string())?;
            }
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_navigate(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    dx: jdouble,
    dy: jdouble,
    zoom: jdouble,
    multi: jboolean,
    width: jint,
    height: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !s.observing || width <= 0 || height <= 0 || !dx.is_finite() || !dy.is_finite() {
                return Err("invalid observation navigation".into());
            }
            s.observer.zoom(zoom as f32).map_err(|e| e.to_string())?;
            if multi != 0 || s.observer.view != aem_core::ObservationView::Free {
                s.sample()?;
                let target = s.observer.camera.target.value;
                let delta = s
                    .scene
                    .screen_translation(
                        target,
                        [dx as f32, dy as f32],
                        [width as u32, height as u32],
                    )
                    .map_err(|e| e.to_string())?;
                s.observer.camera.target.value = std::array::from_fn(|i| target[i] - delta[i]);
            } else {
                s.observer
                    .orbit(dx as f32 * 0.18, dy as f32 * 0.18)
                    .map_err(|e| e.to_string())?;
            }
            s.view_revision += 1;
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_save(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_view(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.observing = kind != 0;
            s.view_revision += 1;
            s.observer.view = match kind {
                0 | 1 => aem_core::ObservationView::Free,
                2 => aem_core::ObservationView::Top,
                3 => aem_core::ObservationView::Side,
                _ => return Err("invalid observation view".into()),
            };
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_surface(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    surface: JObject,
    width: jint,
    height: jint,
) -> jstring {
    let window = if surface.is_null() {
        None
    } else {
        unsafe { NativeWindow::from_surface(env.get_native_interface(), surface.as_raw()) }
    };
    string_result(&mut env, || {
        with_session(id, |s| {
            if let Some(window) = window {
                if width <= 0 || height <= 0 {
                    return Err("surface dimensions must be positive".into());
                }
                s.attach(window, width as u32, height as u32)?;
            } else {
                s.detach();
            }
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_previewInfo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || with_session(id, |s| Ok(s.preview_info())))
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_previewMode(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    mode: jint,
    thermal: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let mode = PreviewMode::from_id(mode).ok_or("invalid preview mode")?;
            let previous_tier = s.preview.tier();
            if s.preview.mode != mode {
                s.preview.set_mode(mode);
            }
            s.preview.set_thermal(thermal);
            if previous_tier != s.preview.tier() {
                s.view_revision += 1;
            }
            Ok(s.preview_info())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_startProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    limit: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !(1..=65_536).contains(&limit) {
                return Err("invalid frame recording limit".into());
            }
            let g = s.graphics.as_mut().ok_or("no preview surface")?;
            g.renderer.device.poll(wgpu::Maintain::Wait);
            g.timer = GpuTimer::new(&g.renderer.device, &g.renderer.queue);
            s.recorder = Some(FrameRecorder::new(s.presented + 1, limit as usize));
            Ok(s.preview_info())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_stopProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if let Some(g) = s.graphics.as_mut() {
                g.renderer.device.poll(wgpu::Maintain::Wait);
                if let Some(timer) = &mut g.timer {
                    for t in timer.collect().into_iter().flatten() {
                        if let Some(r) = &mut s.recorder {
                            r.timing(t);
                        }
                    }
                }
            }
            let r = s.recorder.as_ref().ok_or("frame recording is not active")?;
            let metadata = json!({"preview":s.preview_info(),"projectWidth":s.engine.project().width,"projectHeight":s.engine.project().height,
            "projectFps":s.engine.project().fps,"projectFrames":s.engine.project().frames,"layerCount":s.engine.project().layers.len(),"surfaceEpoch":s.surface_epoch,
            "graphics":s.graphics.as_ref().map(|g|json!({"surfaceWidth":g.config.width,"surfaceHeight":g.config.height,"renderWidth":g.scratch.width,"renderHeight":g.scratch.height,
                "adapter":g.renderer.adapter_info.name,"driver":g.renderer.adapter_info.driver,"driverInfo":g.renderer.adapter_info.driver_info,"backend":format!("{:?}",g.renderer.adapter_info.backend),
                "assetTextureBytes":g.renderer.texture_bytes(),"renderTargetBytes":g.scratch.texture_bytes(),"previewImageReadbackBytes":0,
                "timestampReadbackBytesPerSample":if g.timer.is_some(){32}else{0},"timingSkipped":g.timer.as_ref().map(|t|t.skipped),"timingErrors":g.timer.as_ref().map(|t|t.errors)}))});
            let folder = s.root.join("exports");
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let path = folder.join("preview-performance.json");
            std::fs::write(
                &path,
                serde_json::to_vec(&r.report(metadata)).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            s.recorder = None;
            Ok(json!({"file":path.to_string_lossy()}))
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_injectGraphicsFault(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        #[cfg(not(feature = "diagnostics"))]
        {
            let _ = (id, kind);
            Err("GPU fault injection is not included in this build".into())
        }
        #[cfg(feature = "diagnostics")]
        {
            with_session(id, |s| {
                let g = s.graphics.as_ref().ok_or("no active GPU surface")?;
                match kind {
                    0 => {
                        g.renderer.device.destroy();
                        g.renderer.device.poll(wgpu::Maintain::Wait);
                    }
                    1 => {
                        let _ = g.renderer.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("diagnostic invalid mapped size"),
                            size: 1,
                            usage: wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: true,
                        });
                    }
                    _ => return Err("unknown GPU diagnostic fault".into()),
                }
                g.renderer.device.poll(wgpu::Maintain::Poll);
                Ok(json!({"diagnostics":true,"kind":kind,"gpuError":g.renderer.gpu_error()}))
            })
        }
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_render(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jboolean {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_session(id, |s| {
            // A queued UI frame may precede playback start or belong to the
            // previous composition. Skip it before touching sampled state;
            // invalid timing is not a GPU/device failure.
            if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
                return Ok(false);
            }
            match s.render(frame) {
                Ok(rendered) => Ok(rendered),
                Err(error) => {
                    s.last_error = Some(error);
                    Ok(false)
                }
            }
        })
    }));
    u8::from(
        result
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or(false),
    )
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_capture(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let p = s.engine.project();
            for id in p.reachable_compositions().map_err(|e|e.to_string())?{s.effects.preflight_project(&p.composition(&id).map_err(|e|e.to_string())?)?;}
            s.effects.synchronize_scene_alpha(&s.scene, p, &s.root)?;
            let mut scene = Scene::new(p);
            scene.sample(p, s.frame, None).map_err(|e| e.to_string())?;
            let mut temporary = None;
            if s.graphics.is_none() {
                temporary =
                    Some(pollster::block_on(Renderer::headless()).map_err(|e| e.to_string())?);
            }
            let renderer = if let Some(g) = &mut s.graphics {
                &mut g.renderer
            } else {
                temporary.as_mut().unwrap()
            };
            renderer.set_effect_registry(s.effects.registry.clone());
            renderer
                .configure_assets(p, &s.root)
                .map_err(|e| e.to_string())?;
            renderer.retain_video_instances(&scene);
            renderer.prepare_scene_assets(&scene, aem_render::image_resources::Resolution::Full, false)
                .map_err(|e| e.to_string())?;
            let target = renderer
                .capture_target(p.width, p.height)
                .map_err(|e| e.to_string())?;
            let frames = s
                .video_frames
                .prepare_scene_exact(p, &s.root, &scene)?
                .ok_or("video capture pending; request frames and retry")?;
            renderer.retain_video_instances(&scene);
            for (object, image) in frames {
                let source = scene
                    .video_layers()
                    .into_iter()
                    .find(|l| l.id == object)
                    .unwrap()
                    .video
                    .as_ref()
                    .unwrap()
                    .asset;
                image.upload(renderer, object, source)?;
            }
            let (pixels, _) = renderer
                .capture(&scene, &target)
                .map_err(|e| e.to_string())?;
            let dir = s.root.join("exports");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join(format!("frame-{:06}.png", s.frame.floor() as u32));
            image::save_buffer(&file, &pixels, p.width, p.height, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"path":file.to_string_lossy(),"width":p.width,"height":p.height,"frame":s.frame.floor()}),
            )
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_pack(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let output = s.root.join("exports/project.aem");
            std::fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;
            aem_core::storage::export_package(&s.root, s.engine.project(), &output)
                .map_err(|e| e.to_string())?;
            Ok(json!({"path":output.to_string_lossy()}))
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_newProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || {
        with_session(id, |s| {
            let project: Project = serde_json::from_str(&text?).map_err(|e| e.to_string())?;
            if !project.assets.is_empty()
                || !project.audio_assets.is_empty()
                || !project.video_assets.is_empty()
            {
                return Err("new project must have no external assets".into());
            }
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            let parent = s.root.parent().ok_or("project parent is missing")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let destination = parent.join(format!("project-{stamp}"));
            if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
                aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            }
            std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
            aem_core::storage::save(&destination, engine.project()).map_err(|e| e.to_string())?;
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}
fn project_name_allowed(name: &str, current: &str) -> bool {
    name == current
        || name == "default"
        || ["project-", "import-"].iter().any(|prefix| {
            name.strip_prefix(*prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_openProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    directory: JString,
) -> jstring {
    let name = read_string(&mut env, &directory);
    string_result(&mut env, || {
        with_session(id, |s| {
            let name = name?;
            let current = s
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("invalid active project directory")?;
            if !project_name_allowed(&name, current) || name.contains('/') || name.contains('\\') {
                return Err("invalid project directory".into());
            }
            let parent = s
                .root
                .parent()
                .ok_or("project parent is missing")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let destination = parent
                .join(&name)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if destination.parent() != Some(parent.as_path()) {
                return Err("project resolves outside its library".into());
            }
            let project = aem_core::storage::load(&destination).map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
                aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            }
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_replace(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || {
        with_session(id, |s| {
            let project: Project = serde_json::from_str(&text?).map_err(|e| e.to_string())?;
            aem_core::storage::validate_assets(&s.root, &project).map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            s.replace_project(engine, s.root.clone())?;
            Ok(s.snapshot())
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_importProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    file: JString,
) -> jstring {
    let path = read_string(&mut env, &file);
    string_result(&mut env, || {
        with_session(id, |s| {
            let input = PathBuf::from(path?);
            let parent = s.root.parent().ok_or("project parent is missing")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let destination = parent.join(format!("import-{stamp}"));
            let project = aem_core::storage::import_package(&input, &destination)
                .map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}
/// Pack only small draw parameters into a Java-owned direct buffer for the
/// platform EGL/MediaCodec export adapter. Image bytes are transferred once.
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_sampleInto(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jint,
    buffer: JByteBuffer,
) -> jint {
    let address = env.get_direct_buffer_address(&buffer);
    let capacity = env.get_direct_buffer_capacity(&buffer);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i32> {
        let address = address.map_err(|e| e.to_string())?;
        let capacity = capacity.map_err(|e| e.to_string())?;
        with_session(id, |s| {
            if s.engine
                .project()
                .layers
                .iter()
                .any(|l| l.effects.iter().any(|e| e.enabled))
            {
                return Err("effects require the versioned render plan".into());
            }

            s.frame = f64::from(frame);
            s.scene
                .sample(s.engine.project(), s.frame, None)
                .map_err(|e| e.to_string())?;
            if s.scene.layers.iter().any(|l|l.adjustment||l.vector.is_some()){return Err("vector and adjustment sources require render plan version 4".into());}
            s.geometry.prepare(&s.scene).map_err(|e| e.to_string())?;
            if s.scene.layers.iter().any(|l| l.video.is_some()) {
                return Err("video export requires dynamic frame reads and GeometryBridge".into());
            }
            if s.geometry.batches.iter().any(|b| b.vertices.len() != 6)
                || s.geometry.vertices.len() > s.scene.layers.len() * 6
            {
                return Err("intersecting layers require GeometryBridge.sampleGeometryInto".into());
            }
            let words = s.geometry.batches.len() * 32;
            if capacity < words * 4 {
                return Err("draw parameter buffer is too small".into());
            }
            let output = unsafe { std::slice::from_raw_parts_mut(address, words * 4) };
            for (i, batch) in s.geometry.batches.iter().enumerate() {
                let layer = &s.scene.layers[batch.layer];
                let mut data = [0.0f32; 32];
                data[..16].copy_from_slice(&(layer.view_projection * layer.model).to_cols_array());
                data[16..20].copy_from_slice(&layer.color);
                for c in &mut data[16..19] {
                    *c = if *c <= 0.04045 {
                        *c / 12.92
                    } else {
                        ((*c + 0.055) / 1.055).powf(2.4)
                    };
                }
                data[20] = layer.size[0];
                data[21] = layer.size[1];
                data[22] = layer.opacity;
                data[24] = layer.asset.map_or(0.0, |id| {
                    s.engine
                        .project()
                        .assets
                        .iter()
                        .position(|a| a.id == id)
                        .map_or(0.0, |index| index as f32 + 1.0)
                });
                output[i * 128..(i + 1) * 128].copy_from_slice(bytemuck::cast_slice(&data));
            }
            Ok(s.geometry.batches.len() as i32)
        })
    }));
    result.ok().and_then(std::result::Result::ok).unwrap_or(-1)
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_GeometryBridge_hitCandidates(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    x: jdouble,
    y: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.sample()?;
            let candidates = s
                .scene
                .hit_candidates([x as f32, y as f32])
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"candidates":candidates,"coordinates":"composition_pixels","selection":"geometry_bounds"}),
            )
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_GeometryBridge_sampleGeometryInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
    parameters: JByteBuffer,
    vertices: JByteBuffer,
) -> jstring {
    let parameter_address = env.get_direct_buffer_address(&parameters);
    let parameter_capacity = env.get_direct_buffer_capacity(&parameters);
    let vertex_address = env.get_direct_buffer_address(&vertices);
    let vertex_capacity = env.get_direct_buffer_capacity(&vertices);
    let parameter_readonly = env
        .call_method(&parameters, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z());
    let vertex_readonly = env
        .call_method(&vertices, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z());
    string_result(&mut env, || {
        if parameter_readonly.map_err(|e| e.to_string())?
            || vertex_readonly.map_err(|e| e.to_string())?
        {
            return Err("geometry buffers must be writable".into());
        }
        let pa = parameter_address.map_err(|e| e.to_string())?;
        let pc = parameter_capacity.map_err(|e| e.to_string())?;
        let va = vertex_address.map_err(|e| e.to_string())?;
        let vc = vertex_capacity.map_err(|e| e.to_string())?;
        with_session(id, |s| {
            s.scene
                .sample(s.engine.project(), frame, None)
                .map_err(|e| e.to_string())?;
            if s.scene.layers.iter().any(|l|l.adjustment||l.vector.is_some()){return Err("vector and adjustment sources require render plan version 4".into());}
            s.geometry.prepare(&s.scene).map_err(|e| e.to_string())?;
            let pb = s.geometry.batches.len() * 128;
            let vb = s.geometry.vertices.len() * 20;
            if pc < pb || vc < vb {
                return Err(format!(
                    "geometry buffers too small: need {pb} parameter bytes and {vb} vertex bytes"
                ));
            }
            let pend = (pa as usize)
                .checked_add(pb)
                .ok_or("parameter address overflow")?;
            let vend = (va as usize)
                .checked_add(vb)
                .ok_or("vertex address overflow")?;
            if pb > 0 && vb > 0 && (pa as usize) < vend && (va as usize) < pend {
                return Err("geometry buffers must not overlap".into());
            }
            let parameters = unsafe { std::slice::from_raw_parts_mut(pa, pb) };
            let vertices = unsafe { std::slice::from_raw_parts_mut(va, vb) };
            for (i, batch) in s.geometry.batches.iter().enumerate() {
                let layer = &s.scene.layers[batch.layer];
                let mut data = [0.0f32; 32];
                data[..16].copy_from_slice(&layer.view_projection.to_cols_array());
                data[16..20].copy_from_slice(&layer.color);
                for c in &mut data[16..19] {
                    *c = if *c <= 0.04045 {
                        *c / 12.92
                    } else {
                        ((*c + 0.055) / 1.055).powf(2.4)
                    };
                }
                data[20] = layer.size[0];
                data[21] = layer.size[1];
                data[22] = layer.opacity;
                data[24] = layer.asset.map_or(0.0, |id| {
                    s.engine
                        .project()
                        .assets
                        .iter()
                        .position(|a| a.id == id)
                        .map_or(0.0, |i| i as f32 + 1.0)
                });
                if layer.video.is_some() {
                    data[24] = -(layer.order as f32 + 1.0);
                }
                data[25] = batch.vertices.start as f32;
                data[26] = (batch.vertices.end - batch.vertices.start) as f32;
                data[27] = layer.order as f32;
                data[28] = if layer.three_d { 1.0 } else { 0.0 };
                parameters[i * 128..(i + 1) * 128].copy_from_slice(bytemuck::cast_slice(&data));
            }
            for (i, v) in s.geometry.vertices.iter().enumerate() {
                let data = [
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.uv[0],
                    v.uv[1],
                ];
                vertices[i * 20..(i + 1) * 20].copy_from_slice(bytemuck::cast_slice(&data));
            }
            s.frame = frame;
            Ok(
                json!({"batches":s.geometry.batches.len(),"vertices":s.geometry.vertices.len(),
                "parameterBytes":pb,"vertexBytes":vb,"batchStrideBytes":128,"vertexStrideBytes":20,
                "videoLayers":s.scene.layers.iter().filter_map(|l|l.video.as_ref().map(|v|json!({"object":l.id,"asset":v.asset,
                    "texture_slot":-(l.order as i64+1),"source_time_us":v.source_time_us}))).collect::<Vec<_>>()}),
            )
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_imageInfo(
    mut env: JNIEnv, _class: JClass, path: JString,
) -> jstring {
    let path = read_string(&mut env, &path);
    string_result(&mut env, || {
        let (width,height,format,bytes) = aem_render::image_resources::inspect(std::path::Path::new(&path?))?;
        Ok(json!({"version":1,"width":width,"height":height,"format":format!("{format:?}"),"bytes":bytes,
            "maxEncodedBytes":aem_render::image_resources::MAX_ENCODED_BYTES,"previewMaxEdge":aem_render::image_resources::MAX_PREVIEW_EDGE}))
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_prepareImage(
    mut env: JNIEnv, _class: JClass, root: JString, path: JString,
) -> jstring {
    let root=read_string(&mut env,&root);
    let path=read_string(&mut env,&path);
    string_result(&mut env, || {
        let root=PathBuf::from(root?);let path=path?;
        aem_core::storage::validate_relative_path(&path).map_err(|e|e.to_string())?;
        let (width,height,format,bytes)=aem_render::image_resources::inspect(&root.join(&path))?;
        let asset=aem_core::Asset{id:1,path:path.clone(),width,height};
        let source=aem_render::image_resources::Source::new(&root,&asset)?;
        let proxy=aem_render::image_resources::decode(&source,aem_render::image_resources::Resolution::Preview(aem_render::image_resources::MAX_PREVIEW_EDGE))?;
        Ok(json!({"version":1,"path":path,"width":width,"height":height,"format":format!("{format:?}"),
            "bytes":bytes,"validated":true,"proxyWidth":proxy.width,"proxyHeight":proxy.height,"proxyCached":proxy.cached}))
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_assetPixelsInto(
    mut env: JNIEnv, _class: JClass, id: jlong, asset: jlong, buffer: JByteBuffer,
) -> jstring {
    let destination = (|| -> Result<_> {
        if env.call_method(&buffer,"isReadOnly","()Z",&[]).and_then(|v|v.z()).map_err(|e|e.to_string())? {
            return Err("image output buffer is read-only".into());
        }
        let capacity=env.get_direct_buffer_capacity(&buffer).map_err(|e|e.to_string())?;
        let address=env.get_direct_buffer_address(&buffer).map_err(|e|e.to_string())?;
        Ok((capacity,address))
    })();
    string_result(&mut env, || {
        let (capacity,address)=destination?;
        with_session(id, |s| {
            let a=s.engine.project().assets.iter().find(|a|a.id==asset as u64).ok_or("asset not found")?;
            let bytes=u64::from(a.width)*u64::from(a.height)*4;
            if address.is_null() || bytes>aem_render::image_resources::MAX_DECODED_BYTES || (capacity as u64)<bytes {
                return Err("image direct buffer is too small or exceeds 128 MiB".into());
            }
            let source=aem_render::image_resources::Source::new(&s.root,a)?;
            // JNI owns this direct buffer for the duration of this synchronous call.
            let output=unsafe{std::slice::from_raw_parts_mut(address,bytes as usize)};
            aem_render::image_resources::decode_into(&source,aem_render::image_resources::Resolution::Full,output)?;
            Ok(json!({"version":1,"asset":a.id,"width":a.width,"height":a.height,"bytes":bytes,"resolution":"original"}))
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_assetPixels(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    asset: jlong,
) -> jbyteArray {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_session(id, |s| {
            let a = s
                .engine
                .project()
                .assets
                .iter()
                .find(|a| a.id == asset as u64)
                .ok_or("asset not found")?;
            let reader =
                image::ImageReader::open(s.root.join(&a.path)).map_err(|e| e.to_string())?;
            let dimensions = reader.into_dimensions().map_err(|e| e.to_string())?;
            if dimensions != (a.width, a.height)
                || u64::from(a.width) * u64::from(a.height) * 4 > 128 * 1024 * 1024
            {
                return Err("invalid asset dimensions or memory budget".into());
            }
            let mut pixels = image::ImageReader::open(s.root.join(&a.path))
                .map_err(|e| e.to_string())?
                .decode()
                .map_err(|e| e.to_string())?
                .into_rgba8()
                .into_raw();
            aem_render::premultiply_pixels(&mut pixels);
            Ok(pixels)
        })
    }));
    match result {
        Ok(Ok(pixels)) => env
            .byte_array_from_slice(&pixels)
            .map_or(std::ptr::null_mut(), |b| b.into_raw()),
        _ => std::ptr::null_mut(),
    }
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_destroy(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
        if registry
            .get(&id)
            .is_some_and(|s| s.owner == thread::current().id())
        {
            if let Some(mut s) = registry.remove(&id) {
                s.detach();
            }
        }
    }));
}
