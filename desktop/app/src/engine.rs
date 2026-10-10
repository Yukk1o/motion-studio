//! Engine bridge: the desktop worker thread that owns the host session.
//!
//! This mirrors the Android arrangement exactly. One dedicated thread owns the
//! session, every host call is queued to it, and the UI thread only ever reads
//! an immutable snapshot. That keeps a session's `ThreadId` guarantee, its
//! undo history and its GPU resources on one core, and it means the desktop UI
//! cannot touch engine state mid-edit.

use aem_host::{
    ops::{composition, editing, effects, export, geometry, images, media, preview, project},
    video_frame::DecodedFrame,
    Platform,
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        mpsc::{channel, Sender},
        Arc,
    },
    thread::JoinHandle,
};

/// Maximum JSON payload accepted from the UI. Matches the Android bridge limit
/// so a request that works on one platform works on both.
pub const REQUEST_LIMIT: usize = 256 * 1024;

/// CPU draw commands only. All GPU objects stay with the session worker.
pub struct UiFrame {
    pub paint: aem_ui::paint::PaintList,
    pub atlas: Option<AtlasUpload>,
    pub logical_size: [f32; 2],
    pub composition: Option<[f32; 4]>,
    pub window: u64,
}
pub struct AtlasUpload {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Everything the worker can be asked to do.
///
/// Commands are a closed enum rather than a string channel so a typo in the UI
/// cannot become a runtime error, and so adding a host operation is a
/// compile-time change on both platforms.
#[allow(dead_code)] // Typed SDK bridge; remaining panels will use the other operations.
pub enum Command {
    EditChecked {
        text: String,
        revision: u64,
        reply: Sender<Result<Value, String>>,
    },
    CaptureUi {
        ui: UiFrame,
        path: PathBuf,
        reply: Sender<Result<Value, String>>,
    },
    Attach {
        target: aem_host::SurfaceTarget,
        reply: Sender<Result<Value, String>>,
    },
    Resize {
        window: u64,
        width: u32,
        height: u32,
        reply: Sender<Result<Value, String>>,
    },
    Present {
        ui: UiFrame,
        reply: Sender<Result<bool, String>>,
    },
    OpenDirectory {
        directory: PathBuf,
        reply: Sender<Result<Value, String>>,
    },
    FloatWindow {
        key: u64,
        target: FloatingSurface,
        reply: Sender<Result<Value, String>>,
    },
    CloseWindow {
        key: u64,
        reply: Sender<Result<Value, String>>,
    },
    State(Sender<Result<Value, String>>),
    Save(Sender<Result<Value, String>>),
    NewProject {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    OpenProject {
        name: String,
        reply: Sender<Result<Value, String>>,
    },
    ImportProject {
        file: PathBuf,
        reply: Sender<Result<Value, String>>,
    },
    Edit {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    History {
        op: i32,
        reply: Sender<Result<Value, String>>,
    },
    Drag {
        object: i64,
        dx: f64,
        dy: f64,
        width: i32,
        height: i32,
        reply: Sender<Result<Value, String>>,
    },
    Seek {
        frame: f64,
        reply: Sender<Result<Value, String>>,
    },
    View {
        kind: i32,
        reply: Sender<Result<Value, String>>,
    },
    Observe {
        enabled: bool,
        azimuth: f64,
        elevation: f64,
        reply: Sender<Result<Value, String>>,
    },
    Navigate {
        dx: f64,
        dy: f64,
        zoom: f64,
        multi: bool,
        width: i32,
        height: i32,
        reply: Sender<Result<Value, String>>,
    },
    Render {
        frame: f64,
        reply: Sender<Result<bool, String>>,
    },
    ConfigureMemory {
        total: i64,
        guarded: bool,
        reply: Sender<Result<Value, String>>,
    },
    PreviewMode {
        mode: i32,
        thermal: i32,
        reply: Sender<Result<Value, String>>,
    },
    PreviewInfo(Sender<Result<Value, String>>),
    CurveGraph {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    HitCandidates {
        x: f64,
        y: f64,
        reply: Sender<Result<Value, String>>,
    },
    Plugin {
        request: String,
        reply: Sender<Result<Value, String>>,
    },
    ColorCurveGraph {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    Composition {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    Media {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    MediaPackageLimits(Sender<Result<Value, String>>),
    Capture(Sender<Result<Value, String>>),
    Pack(Sender<Result<Value, String>>),
    ImageInfo {
        path: String,
        reply: Sender<Result<Value, String>>,
    },
    PrepareImage {
        root: PathBuf,
        path: String,
        reply: Sender<Result<Value, String>>,
    },
    /// Media capabilities do not need a session, so they answer immediately.
    MediaCapabilities {
        text: String,
        reply: Sender<Result<Value, String>>,
    },
    ResourceInfo {
        directory: PathBuf,
        reply: Sender<Result<Value, String>>,
    },
    /// A decoded video frame requested through the media request pipeline.
    VideoFrame {
        object: u64,
        sequence: u64,
        reply: Sender<Result<ArcFrame, String>>,
    },
    Shutdown,
}

/// A decoded frame plus its timing report, as both hosts return it.
#[allow(dead_code)] // Video frame reports are part of the desktop SDK bridge.
pub struct ArcFrame {
    pub frame: std::sync::Arc<DecodedFrame>,
    pub report: Value,
}

impl std::ops::Deref for ArcFrame {
    type Target = DecodedFrame;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}

/// Handle used by the UI thread to talk to the engine.
pub struct Engine {
    sender: Sender<Command>,
    worker: Option<JoinHandle<()>>,
    platform: Arc<dyn Platform>,
    factory: std::sync::Mutex<Option<SurfaceFactory>>,
}

#[allow(dead_code)] // SDK bridge methods are intentionally available to future panels.
impl Engine {
    /// Start the worker thread and open a project at `root`.
    pub fn start(
        root: PathBuf,
        platform: std::sync::Arc<dyn Platform>,
        total_memory: u64,
    ) -> Result<Self, String> {
        let desktop_platform = platform.clone();
        let (sender, receiver) = channel::<Command>();
        let (ready, opened) = channel::<Result<i64, String>>();
        let worker = std::thread::Builder::new()
            .name("motion-engine".into())
            .spawn(move || {
                let mut guard = match ProjectGuard::acquire(&root) {
                    Ok(guard) => guard,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                let initial = if root.join("project.json").exists() {
                    String::new()
                } else {
                    let mut project = aem_core::Project::new(1920, 1080, 30, 300)
                        .expect("valid default composition");
                    project.name = "Untitled Project".into();
                    serde_json::to_string(&project).expect("serializable default composition")
                };
                let id = match project::create(root, &initial, platform.clone()) {
                    Ok(id) => id,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                // The scratch budget is a device property, so apply it before
                // the first render rather than after a visible hitch.
                if let Err(error) = preview::configure_memory(id, total_memory as i64, false) {
                    aem_host::session::close(id);
                    let _ = ready.send(Err(error));
                    return;
                }
                if ready.send(Ok(id)).is_err() {
                    aem_host::session::close(id);
                    return;
                }
                let mut painter = None;
                let mut windows = std::collections::HashMap::new();
                while let Ok(command) = receiver.recv() {
                    if handle(
                        id,
                        &platform,
                        &mut painter,
                        &mut windows,
                        &mut guard,
                        command,
                    ) {
                        break;
                    }
                }
                windows.clear();
                aem_host::session::close(id);
            })
            .map_err(|e| e.to_string())?;
        let _id = opened
            .recv()
            .map_err(|_| "engine thread stopped during startup".to_string())??;
        Ok(Self {
            sender,
            worker: Some(worker),
            platform: desktop_platform,
            factory: std::sync::Mutex::new(None),
        })
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .send(command)
            .map_err(|_| "engine thread has stopped".into())
    }

    /// Blocking call for operations the UI thread can afford to wait on.
    pub fn request<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, String>>) -> Command,
    ) -> Result<std::sync::mpsc::Receiver<Result<T, String>>, String> {
        let (reply, receive) = channel();
        self.send(make(reply))?;
        Ok(receive)
    }
    pub fn call<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (reply, receive) = channel();
        self.send(make(reply))?;
        receive
            .recv()
            .map_err(|_| "engine thread has stopped".to_string())?
    }

    pub fn state(&self) -> Result<Value, String> {
        self.call(Command::State)
    }
    pub fn save(&self) -> Result<Value, String> {
        self.call(Command::Save)
    }
    pub fn new_project(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::NewProject { text, reply })
    }
    pub fn open_project(&self, name: String) -> Result<Value, String> {
        self.call(|reply| Command::OpenProject { name, reply })
    }
    pub fn import_project(&self, file: PathBuf) -> Result<Value, String> {
        self.call(|reply| Command::ImportProject { file, reply })
    }
    pub fn edit(&self, text: String) -> Result<Value, String> {
        if text.len() > REQUEST_LIMIT {
            return Err("edit request exceeds 256 KiB".into());
        }
        self.call(|reply| Command::Edit { text, reply })
    }
    pub fn edit_at_revision(&self, text: String, revision: u64) -> Result<Value, String> {
        if text.len() > REQUEST_LIMIT {
            return Err("edit request exceeds 256 KiB".into());
        }
        self.call(|reply| Command::EditChecked {
            text,
            revision,
            reply,
        })
    }
    pub fn capture_ui(&self, ui: UiFrame, path: PathBuf) -> Result<Value, String> {
        self.call(|reply| Command::CaptureUi { ui, path, reply })
    }
    pub fn history(&self, op: i32) -> Result<Value, String> {
        self.call(|reply| Command::History { op, reply })
    }
    pub fn drag(
        &self,
        object: i64,
        dx: f64,
        dy: f64,
        width: i32,
        height: i32,
    ) -> Result<Value, String> {
        self.call(|reply| Command::Drag {
            object,
            dx,
            dy,
            width,
            height,
            reply,
        })
    }
    pub fn seek(&self, frame: f64) -> Result<Value, String> {
        self.call(|reply| Command::Seek { frame, reply })
    }
    pub fn view(&self, kind: i32) -> Result<Value, String> {
        self.call(|reply| Command::View { kind, reply })
    }
    pub fn observe(&self, enabled: bool, azimuth: f64, elevation: f64) -> Result<Value, String> {
        self.call(|reply| Command::Observe {
            enabled,
            azimuth,
            elevation,
            reply,
        })
    }
    pub fn navigate(
        &self,
        dx: f64,
        dy: f64,
        zoom: f64,
        multi: bool,
        width: i32,
        height: i32,
    ) -> Result<Value, String> {
        self.call(|reply| Command::Navigate {
            dx,
            dy,
            zoom,
            multi,
            width,
            height,
            reply,
        })
    }
    pub fn render(&self, frame: f64) -> Result<bool, String> {
        self.call(|reply| Command::Render { frame, reply })
    }
    pub fn configure_memory(&self, total: i64, guarded: bool) -> Result<Value, String> {
        self.call(|reply| Command::ConfigureMemory {
            total,
            guarded,
            reply,
        })
    }
    pub fn preview_mode(&self, mode: i32, thermal: i32) -> Result<Value, String> {
        self.call(|reply| Command::PreviewMode {
            mode,
            thermal,
            reply,
        })
    }
    pub fn preview_info(&self) -> Result<Value, String> {
        self.call(Command::PreviewInfo)
    }
    pub fn curve_graph(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::CurveGraph { text, reply })
    }
    pub fn hit_candidates(&self, x: f64, y: f64) -> Result<Value, String> {
        self.call(|reply| Command::HitCandidates { x, y, reply })
    }
    pub fn plugin(&self, request: String) -> Result<Value, String> {
        self.call(|reply| Command::Plugin { request, reply })
    }
    pub fn color_curve_graph(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::ColorCurveGraph { text, reply })
    }
    pub fn composition(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::Composition { text, reply })
    }
    pub fn media(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::Media { text, reply })
    }
    pub fn media_package_limits(&self) -> Result<Value, String> {
        self.call(Command::MediaPackageLimits)
    }
    pub fn media_capabilities(&self, text: String) -> Result<Value, String> {
        self.call(|reply| Command::MediaCapabilities { text, reply })
    }
    pub fn capture(&self) -> Result<Value, String> {
        self.call(Command::Capture)
    }
    pub fn pack(&self) -> Result<Value, String> {
        self.call(Command::Pack)
    }
    pub fn image_info(&self, path: String) -> Result<Value, String> {
        self.call(|reply| Command::ImageInfo { path, reply })
    }
    pub fn prepare_image(&self, root: PathBuf, path: String) -> Result<Value, String> {
        self.call(|reply| Command::PrepareImage { root, path, reply })
    }
    pub fn resource_info(&self, directory: PathBuf) -> Result<Value, String> {
        self.call(|reply| Command::ResourceInfo { directory, reply })
    }
    pub fn video_frame(&self, object: u64, sequence: u64) -> Result<ArcFrame, String> {
        self.call(|reply| Command::VideoFrame {
            object,
            sequence,
            reply,
        })
    }

    pub fn open_directory(&self, directory: PathBuf) -> Result<Value, String> {
        self.call(|reply| Command::OpenDirectory { directory, reply })
    }
    pub fn attach_async(
        &self,
        window: Arc<dyn wgpu::WindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<std::sync::mpsc::Receiver<Result<Value, String>>, String> {
        // Winit exposes Win32 handles only on the GUI thread. Build a surface
        // here, then transfer rendering ownership to the session worker.
        let target = self.platform.attach_surface(window, width, height)?;
        *self
            .factory
            .lock()
            .map_err(|_| "surface factory lock poisoned")? = Some(SurfaceFactory {
            instance: target.instance.clone(),
            adapter: target.renderer.adapter.clone(),
            device: target.renderer.device.clone(),
            config: target.config.clone(),
        });
        self.request(|reply| Command::Attach { target, reply })
    }
    pub fn resize_async(
        &self,
        window: u64,
        width: u32,
        height: u32,
    ) -> Result<std::sync::mpsc::Receiver<Result<Value, String>>, String> {
        self.request(|reply| Command::Resize {
            window,
            width,
            height,
            reply,
        })
    }
    pub fn float_window(
        &self,
        key: u64,
        window: Arc<dyn wgpu::WindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<Value, String> {
        let factory = self
            .factory
            .lock()
            .map_err(|_| "surface factory lock poisoned")?;
        let factory = factory.as_ref().ok_or("desktop surface is not attached")?;
        let surface = factory
            .instance
            .create_surface(window)
            .map_err(|e| e.to_string())?;
        if !surface
            .get_capabilities(&factory.adapter)
            .formats
            .contains(&factory.config.format)
        {
            return Err("floating surface format is incompatible".into());
        }
        let mut config = factory.config.clone();
        config.width = width;
        config.height = height;
        surface.configure(&factory.device, &config);
        let target = FloatingSurface { surface, config };
        self.call(|reply| Command::FloatWindow { key, target, reply })
    }
    pub fn close_window(&self, key: u64) -> Result<Value, String> {
        self.call(|reply| Command::CloseWindow { key, reply })
    }
    pub fn present_async(
        &self,
        ui: UiFrame,
    ) -> Result<std::sync::mpsc::Receiver<Result<bool, String>>, String> {
        self.request(|reply| Command::Present { ui, reply })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn clear_target(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("floating panel background"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.01,
                    g: 0.01,
                    b: 0.012,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

fn respond<T>(reply: Sender<Result<T, String>>, value: Result<T, String>) {
    let _ = reply.send(value);
}

/// Execute one command against the session.
///
/// Returns true when the worker should stop. Reply channels are best effort:
/// a UI that has already gone away must not keep the engine blocked.
struct ProjectGuard {
    root: PathBuf,
    _file: std::fs::File,
}
impl ProjectGuard {
    fn acquire(root: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".motion-editor.lock"))
            .map_err(|e| e.to_string())?;
        file.try_lock()
            .map_err(|e| format!("project is already open or cannot be locked: {e}"))?;
        Ok(Self { root, _file: file })
    }
    fn changed(&mut self, result: Result<Value, String>) -> Result<Value, String> {
        let state = result?;
        let root = PathBuf::from(state["root"].as_str().ok_or("missing project root")?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if root != self.root {
            *self = Self::acquire(&root)?;
        }
        Ok(state)
    }
}

struct SurfaceFactory {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
}

pub struct FloatingSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

fn handle(
    id: i64,
    platform: &Arc<dyn Platform>,
    painter: &mut Option<aem_ui::Painter>,
    windows: &mut std::collections::HashMap<u64, FloatingSurface>,
    guard: &mut ProjectGuard,
    command: Command,
) -> bool {
    match command {
        Command::EditChecked { text, revision, reply } => respond(reply,aem_host::session::with_session(id, |s| {
            if s.engine.gesture_active(){return Err("finish the active editor gesture before an agent edit".into());}
            if s.engine.revision() != revision { return Err("project revision changed; fetch motion_state before editing".into()); }
            s.apply_commands(&text)
        })),
        Command::CaptureUi { ui, path, reply } => respond(reply,aem_host::session::with_session(id, |s| {
            let g = s.graphics.as_ref().ok_or("desktop surface is not attached")?;
            let width = g.config.width; let height = g.config.height;
            let target = g.renderer.capture_target(width, height).map_err(|e| e.to_string())?;
            let mut chrome = aem_ui::Painter::new(&g.renderer.device, wgpu::TextureFormat::Rgba8UnormSrgb);
            let upload = ui.atlas.ok_or("screenshot requires the complete glyph atlas")?;
            chrome.upload_atlas(&g.renderer.device, &g.renderer.queue, &upload.pixels, upload.width, upload.height);
            let presenter = aem_render::Presenter::new(&g.renderer, &g.scratch.view, wgpu::TextureFormat::Rgba8UnormSrgb);
            let mut encoder = g.renderer.device.create_command_encoder(&Default::default());
            let [x,y,w,h] = ui.composition.ok_or("screenshot needs a composition viewport")?;
            let scale = (w / s.scene.width as f32).min(h / s.scene.height as f32);
            let vw = s.scene.width as f32 * scale; let vh = s.scene.height as f32 * scale;
            presenter.encode(&mut encoder, &target.view, None, Some([x+(w-vw)*0.5,y+(h-vh)*0.5,vw,vh]), s.scene.background);
            chrome.render(&g.renderer.device, &g.renderer.queue, &mut encoder, &target.view, &ui.paint, ui.logical_size);
            g.renderer.queue.submit(Some(encoder.finish()));
            let pixels = g.renderer.read_target(&target).map_err(|e| e.to_string())?;
            image::save_buffer(&path, &pixels, width, height, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({"path":path,"width":width,"height":height}))
        })),
        Command::Attach { target, reply } => respond(reply,(|| {
            aem_host::session::with_session(id, |s| {
                let next = aem_ui::Painter::new(&target.renderer.device, target.config.format);
                s.attach(target)?;
                *painter = Some(next);
                Ok(s.snapshot())
            })
        })()),
        Command::FloatWindow {key,target,reply} => respond(reply,aem_host::session::with_session(id,|s|{
            if key==0 || windows.contains_key(&key){return Err("invalid floating window id".into());}
            windows.insert(key,target);Ok(s.snapshot())
        })),
        Command::CloseWindow {key,reply} => {windows.remove(&key);respond(reply,project::state(id));},
        Command::Resize { window, width, height, reply } => respond(reply,aem_host::session::with_session(id, |s| {
            if width == 0 || height == 0 { return Ok(s.snapshot()); }
            if let Some(g) = &mut s.graphics {
                if window==0 {g.config.width=width;g.config.height=height;g.surface.configure(&g.renderer.device,&g.config);}
                else if let Some(w)=windows.get_mut(&window){w.config.width=width;w.config.height=height;w.surface.configure(&g.renderer.device,&w.config);}
                else{return Err("floating window is closed".into());}
                s.view_revision += 1;
            }
            Ok(s.snapshot())
        })),
        Command::Present { ui, reply } => respond(reply,aem_host::session::with_session(id, |s| {
            let painter=painter.as_mut().ok_or("desktop surface is not attached")?;
            let floating=if ui.window==0 {None}else{Some(windows.get_mut(&ui.window).ok_or("floating window is closed")?)};
            let mut floating=floating;
            if let (Some(w),Some(g))=(floating.as_mut(),s.graphics.as_mut()) {
                std::mem::swap(&mut w.surface,&mut g.surface);std::mem::swap(&mut w.config,&mut g.config);
            }
            let result=(||{
                let g=s.graphics.as_ref().ok_or("desktop surface is not attached")?;
                if let Some(upload)=&ui.atlas {painter.upload_atlas(&g.renderer.device,&g.renderer.queue,&upload.pixels,upload.width,upload.height);}
                if let Some(viewport)=ui.composition {
                    let frame=s.frame; // Shared clock, never a stale UI frame.
                    s.render_with_overlay(frame,Some(viewport),|r,encoder,target|painter.render(&r.device,&r.queue,encoder,target,&ui.paint,ui.logical_size))
                }else{
                    let output=match g.surface.get_current_texture(){
                        Ok(output)=>output,
                        Err(wgpu::SurfaceError::Lost|wgpu::SurfaceError::Outdated)=>{g.surface.configure(&g.renderer.device,&g.config);return Ok(false)},
                        Err(wgpu::SurfaceError::Timeout)=>return Ok(false),Err(e)=>return Err(e.to_string()),
                    };
                    let view=output.texture.create_view(&Default::default());
                    let mut encoder=g.renderer.device.create_command_encoder(&Default::default());
                    clear_target(&mut encoder,&view);
                    painter.render(&g.renderer.device,&g.renderer.queue,&mut encoder,&view,&ui.paint,ui.logical_size);
                    g.renderer.queue.submit(Some(encoder.finish()));output.present();Ok(true)
                }
            })();
            if let (Some(w),Some(g))=(floating.as_mut(),s.graphics.as_mut()) {
                std::mem::swap(&mut w.surface,&mut g.surface);std::mem::swap(&mut w.config,&mut g.config);
            }
            result
        })),
        Command::OpenDirectory { directory, reply } => respond(reply,(||{
            aem_core::storage::load(&directory).map_err(|e|e.to_string())?;
            let directory=directory.canonicalize().map_err(|e|e.to_string())?;
            if directory==guard.root{return project::save(id);}
            let next=ProjectGuard::acquire(&directory)?;
            let result=project::open_directory(id,&directory)?;*guard=next;Ok(result)
        })()),
        Command::State(reply) => respond(reply,project::state(id).map(|mut state|{
            state["desktop"]=serde_json::json!({"headless":painter.is_none(),"panelWindows":if painter.is_some(){windows.len()+1}else{0},"sharedGpuDevice":painter.is_some(),"agentToolProtocol":1});state
        })),
        Command::Save(reply) => respond(reply,project::save(id)),
        Command::NewProject { text, reply } => respond(reply,guard.changed(project::new_project(id, &text))),
        Command::OpenProject { name, reply } => respond(reply,(||{
            let directory=guard.root.parent().ok_or("project parent is missing")?.join(&name).canonicalize().map_err(|e|e.to_string())?;
            if directory==guard.root{return project::save(id);}
            let next=ProjectGuard::acquire(&directory)?;
            let result=project::open_project(id,&name)?;*guard=next;Ok(result)
        })()),
        Command::ImportProject { file, reply } => respond(reply,guard.changed(project::import_project(id, &file))),
        Command::Edit { text, reply } => respond(reply,editing::command(id, &text)),
        Command::History { op, reply } => respond(reply,editing::history(id, op)),
        Command::Drag {
            object,
            dx,
            dy,
            width,
            height,
            reply,
        } => respond(reply,editing::drag(id, object, dx, dy, width, height)),
        Command::Seek { frame, reply } => respond(reply,preview::seek(id, frame)),
        Command::View { kind, reply } => respond(reply,preview::view(id, kind)),
        Command::Observe {
            enabled,
            azimuth,
            elevation,
            reply,
        } => respond(reply,preview::observe(id, enabled, azimuth, elevation)),
        Command::Navigate {
            dx,
            dy,
            zoom,
            multi,
            width,
            height,
            reply,
        } => respond(reply,preview::navigate(id, dx, dy, zoom, multi, width, height)),
        Command::Render { frame, reply } => respond(reply,preview::render(id, frame)),
        Command::ConfigureMemory {
            total,
            guarded,
            reply,
        } => respond(reply,preview::configure_memory(id, total, guarded)),
        Command::PreviewMode {
            mode,
            thermal,
            reply,
        } => respond(reply,preview::preview_mode(id, mode, thermal)),
        Command::PreviewInfo(reply) => respond(reply,preview::preview_info(id)),
        Command::CurveGraph { text, reply } => respond(reply,editing::curve_graph(&text)),
        Command::HitCandidates { x, y, reply } => respond(reply,geometry::hit_candidates(id, x, y)),
        Command::Plugin { request, reply } => respond(reply,effects::plugin(id, &request)),
        Command::ColorCurveGraph { text, reply } => respond(reply,effects::color_curve_graph(&text)),
        Command::Composition { text, reply } => respond(reply,composition::request(id, &text)),
        Command::Media { text, reply } => respond(reply,media_request(id, &text)),
        Command::MediaPackageLimits(reply) => respond(reply,Ok(media::package_limits())),
        Command::Capture(reply) => respond(reply,export::capture(id)),
        Command::Pack(reply) => respond(reply,export::pack(id)),
        Command::ImageInfo { path, reply } => respond(reply,images::image_info(&path)),
        Command::PrepareImage { root, path, reply } => respond(reply,images::prepare_image(root, path)),
        Command::MediaCapabilities { text, reply } => {
            respond(reply,capabilities_request(platform, &text))
        }
        Command::ResourceInfo { directory, reply } => {
            respond(reply,aem_host::session::resource_info(&directory))
        }
        Command::VideoFrame {
            object,
            sequence,
            reply,
        } => respond(reply,media::read_video_frame_into(id, object, sequence).map(|frame| ArcFrame {
            report: media::frame_report(&frame, object, sequence),
            frame,
        })),
        Command::Shutdown => return true,
    };
    false
}

/// Desktop media requests carry real paths, so the shared opener applies.
fn media_request(id: i64, text: &str) -> Result<Value, String> {
    let value = aem_host::ops::parse_request(text, 16 * 1024, "media request")?;
    let opener = value["path"]
        .as_str()
        .map(|path| media::file_opener(PathBuf::from(path)));
    media::request(id, value, opener)
}

/// Capability queries need no session, so they answer from the platform alone.
fn capabilities_request(platform: &Arc<dyn Platform>, text: &str) -> Result<Value, String> {
    let query = aem_host::ops::parse_request(text, 16 * 1024, "media request")
        .ok()
        .and_then(|value| {
            serde_json::from_value::<media::Request>(value)
                .ok()
                .and_then(|request| match request {
                    media::Request::MediaCapabilities { video_query } => video_query,
                    _ => None,
                })
        });
    platform.media_capabilities(query.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_lock_releases_on_worker_shutdown() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("default");
        let engine = Engine::start(root.clone(), aem_desktop_media::platform(), 8 << 30).unwrap();
        assert!(Engine::start(root.clone(), aem_desktop_media::platform(), 8 << 30).is_err());
        engine.save().unwrap();
        drop(engine);
        let reopened = Engine::start(root, aem_desktop_media::platform(), 8 << 30).unwrap();
        assert!(reopened.state().is_ok());
    }
}
