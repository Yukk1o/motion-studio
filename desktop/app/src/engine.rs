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
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
};

/// Maximum JSON payload accepted from the UI. Matches the Android bridge limit
/// so a request that works on one platform works on both.
pub const REQUEST_LIMIT: usize = 256 * 1024;

/// Everything the worker can be asked to do.
///
/// Commands are a closed enum rather than a string channel so a typo in the UI
/// cannot become a runtime error, and so adding a host operation is a
/// compile-time change on both platforms.
pub enum Command {
    State(Sender<Result<Value, String>>),
    Save(Sender<Result<Value, String>>),
    NewProject { text: String, reply: Sender<Result<Value, String>> },
    OpenProject { name: String, reply: Sender<Result<Value, String>> },
    ImportProject { file: PathBuf, reply: Sender<Result<Value, String>> },
    Edit { text: String, reply: Sender<Result<Value, String>> },
    History { op: i32, reply: Sender<Result<Value, String>> },
    Drag { object: i64, dx: f64, dy: f64, width: i32, height: i32, reply: Sender<Result<Value, String>> },
    Seek { frame: f64, reply: Sender<Result<Value, String>> },
    View { kind: i32, reply: Sender<Result<Value, String>> },
    Observe { enabled: bool, azimuth: f64, elevation: f64, reply: Sender<Result<Value, String>> },
    Navigate { dx: f64, dy: f64, zoom: f64, multi: bool, width: i32, height: i32, reply: Sender<Result<Value, String>> },
    Render { frame: f64, reply: Sender<Result<bool, String>> },
    ConfigureMemory { total: i64, guarded: bool, reply: Sender<Result<Value, String>> },
    PreviewMode { mode: i32, thermal: i32, reply: Sender<Result<Value, String>> },
    PreviewInfo(Sender<Result<Value, String>>),
    CurveGraph { text: String, reply: Sender<Result<Value, String>> },
    HitCandidates { x: f64, y: f64, reply: Sender<Result<Value, String>> },
    Plugin { request: String, reply: Sender<Result<Value, String>> },
    ColorCurveGraph { text: String, reply: Sender<Result<Value, String>> },
    Composition { text: String, reply: Sender<Result<Value, String>> },
    Media { text: String, reply: Sender<Result<Value, String>> },
    MediaPackageLimits(Sender<Result<Value, String>>),
    Capture(Sender<Result<Value, String>>),
    Pack(Sender<Result<Value, String>>),
    ImageInfo { path: String, reply: Sender<Result<Value, String>> },
    PrepareImage { root: PathBuf, path: String, reply: Sender<Result<Value, String>> },
    /// Media capabilities do not need a session, so they answer immediately.
    MediaCapabilities { text: String, reply: Sender<Result<Value, String>> },
    ResourceInfo { directory: PathBuf, reply: Sender<Result<Value, String>> },
    /// A decoded video frame requested through the media request pipeline.
    VideoFrame { object: u64, sequence: u64, reply: Sender<Result<ArcFrame, String>> },
    Shutdown,
}

/// A decoded frame plus its timing report, as both hosts return it.
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
    id: std::cell::Cell<i64>,
}

impl Engine {
    /// Start the worker thread and open a project at `root`.
    pub fn start(
        root: PathBuf,
        platform: std::sync::Arc<dyn Platform>,
        total_memory: u64,
    ) -> Result<Self, String> {
        let (sender, receiver) = channel::<Command>();
        let (ready, opened) = channel::<Result<i64, String>>();
        let worker = std::thread::Builder::new()
            .name("motion-engine".into())
            .spawn(move || {
                let id = match project::create(root, "", platform.clone()) {
                    Ok(id) => id,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                // The scratch budget is a device property, so apply it before
                // the first render rather than after a visible hitch.
                if let Err(error) = preview::configure_memory(id, total_memory as i64, false) {
                    let _ = ready.send(Err(error));
                    return;
                }
                if ready.send(Ok(id)).is_err() {
                    return;
                }
                while let Ok(command) = receiver.recv() {
                    if handle(id, &platform, command) {
                        break;
                    }
                }
                aem_host::session::close(id);
            })
            .map_err(|e| e.to_string())?;
        let id = opened
            .recv()
            .map_err(|_| "engine thread stopped during startup".to_string())??;
        Ok(Self {
            sender,
            worker: Some(worker),
            id: std::cell::Cell::new(id),
        })
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .send(command)
            .map_err(|_| "engine thread has stopped".into())
    }

    /// Blocking call for operations the UI thread can afford to wait on.
    pub fn call<T>(&self, make: impl FnOnce(Sender<Result<T, String>>) -> Command) -> Result<T, String> {
        let (reply, receive) = channel();
        self.send(make(reply))?;
        receive.recv().map_err(|_| "engine thread has stopped".to_string())?
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
        self.call(|reply| Command::Edit { text, reply })
    }
    pub fn history(&self, op: i32) -> Result<Value, String> {
        self.call(|reply| Command::History { op, reply })
    }
    pub fn drag(&self, object: i64, dx: f64, dy: f64, width: i32, height: i32) -> Result<Value, String> {
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

    /// Bind the engine surface to a window and re-present on the next frame.
    ///
    /// Called from the UI thread once the window exists, then once per resize.
    pub fn attach(
        &self,
        platform: &dyn Platform,
        window: Box<dyn raw_window_handle::HasWindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<Value, String> {
        let target = platform.attach_surface(window, width, height)?;
        let id = self.id.get();
        aem_host::session::with_session(id, |s| {
            s.attach(target)?;
            Ok(s.snapshot())
        })
    }

    pub fn detach(&self) {
        let id = self.id.get();
        let _ = aem_host::session::with_session(id, |s| {
            s.detach();
            Ok(())
        });
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


/// Execute one command against the session.
///
/// Returns true when the worker should stop. Reply channels are best effort:
/// a UI that has already gone away must not keep the engine blocked.
fn handle(id: i64, platform: &Arc<dyn Platform>, command: Command) -> bool {
    match command {
        Command::State(reply) => reply.send(project::state(id)),
        Command::Save(reply) => reply.send(project::save(id)),
        Command::NewProject { text, reply } => reply.send(project::new_project(id, &text)),
        Command::OpenProject { name, reply } => reply.send(project::open_project(id, &name)),
        Command::ImportProject { file, reply } => reply.send(project::import_project(id, &file)),
        Command::Edit { text, reply } => reply.send(editing::command(id, &text)),
        Command::History { op, reply } => reply.send(editing::history(id, op)),
        Command::Drag {
            object,
            dx,
            dy,
            width,
            height,
            reply,
        } => reply.send(editing::drag(id, object, dx, dy, width, height)),
        Command::Seek { frame, reply } => reply.send(preview::seek(id, frame)),
        Command::View { kind, reply } => reply.send(preview::view(id, kind)),
        Command::Observe {
            enabled,
            azimuth,
            elevation,
            reply,
        } => reply.send(preview::observe(id, enabled, azimuth, elevation)),
        Command::Navigate {
            dx,
            dy,
            zoom,
            multi,
            width,
            height,
            reply,
        } => reply.send(preview::navigate(id, dx, dy, zoom, multi, width, height)),
        Command::Render { frame, reply } => reply.send(preview::render(id, frame)),
        Command::ConfigureMemory {
            total,
            guarded,
            reply,
        } => reply.send(preview::configure_memory(id, total, guarded)),
        Command::PreviewMode {
            mode,
            thermal,
            reply,
        } => reply.send(preview::preview_mode(id, mode, thermal)),
        Command::PreviewInfo(reply) => reply.send(preview::preview_info(id)),
        Command::CurveGraph { text, reply } => reply.send(editing::curve_graph(&text)),
        Command::HitCandidates { x, y, reply } => reply.send(geometry::hit_candidates(id, x, y)),
        Command::Plugin { request, reply } => reply.send(effects::plugin(id, &request)),
        Command::ColorCurveGraph { text, reply } => reply.send(effects::color_curve_graph(&text)),
        Command::Composition { text, reply } => reply.send(composition::request(id, &text)),
        Command::Media { text, reply } => reply.send(media_request(id, &text)),
        Command::MediaPackageLimits(reply) => reply.send(Ok(media::package_limits())),
        Command::Capture(reply) => reply.send(export::capture(id)),
        Command::Pack(reply) => reply.send(export::pack(id)),
        Command::ImageInfo { path, reply } => reply.send(images::image_info(&path)),
        Command::PrepareImage { root, path, reply } => reply.send(images::prepare_image(root, path)),
        Command::MediaCapabilities { text, reply } => {
            reply.send(capabilities_request(platform, &text))
        }
        Command::ResourceInfo { directory, reply } => {
            reply.send(aem_host::session::resource_info(&directory))
        }
        Command::VideoFrame {
            object,
            sequence,
            reply,
        } => reply.send(media::read_video_frame_into(id, object, sequence).map(|frame| ArcFrame {
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
            serde_json::from_value::<media::Request>(value).ok().and_then(|request| match request
            {
                media::Request::MediaCapabilities { video_query } => video_query,
                _ => None,
            })
        });
    platform.media_capabilities(query.as_ref())
}