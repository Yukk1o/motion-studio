//! Native desktop editor. One worker owns the session, GPU and swapchain.
mod engine;
mod i18n;
mod mcp;
mod panels;
mod workspace;
use i18n::{Locale, Text as T};

use aem_core::{Command, Layer, Project};
use aem_ui::{
    dock,
    input::{Event, Input, Key, Modifiers, MouseButton},
    paint::PaintList,
    text::TextAtlas,
};
use engine::{AtlasUpload, Engine, UiFrame};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key as WinitKey, NamedKey},
    window::{Window, WindowId},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Motion Studio\n  --project <directory>  open an existing project or create one\n  --mcp                  serve editor tools over MCP stdio (no window)\n  --mcp-ui               serve MCP and open the shared editor window\n  --locale <zh|en>        UI language override\n  --smoke <report.json>  hidden-window GPU acceptance with screenshot");
        return Ok(());
    }
    let argument = |flag: &str| -> Result<Option<PathBuf>, String> {
        if let Some(i) = args.iter().position(|a| a == flag) {
            args.get(i + 1)
                .filter(|a| !a.starts_with("--"))
                .map(|a| Some(PathBuf::from(a)))
                .ok_or_else(|| format!("{flag} requires a path"))
        } else {
            Ok(None)
        }
    };
    let smoke = argument("--smoke")?;
    let locale = argument("--locale")?
        .map(|p| Locale::parse(&p.to_string_lossy()).ok_or("locale must be zh or en"))
        .transpose()?;
    let root = argument("--project")?
        .or_else(|| std::env::var_os("MOTION_PROJECT").map(PathBuf::from))
        .unwrap_or_else(|| {
            if smoke.is_some() {
                std::env::temp_dir()
                    .join(format!("motion-desktop-smoke-{}", std::process::id()))
                    .join("default")
            } else {
                directories().join("default")
            }
        });
    if args.iter().any(|a| a == "--mcp") {
        return mcp::serve(
            Arc::new(Engine::start(
                root,
                aem_desktop_media::platform(),
                total_memory(),
            )?),
            None,
        )
        .map_err(Into::into);
    }
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = Shell::new(root, smoke, locale)?;
    if args.iter().any(|a| a == "--mcp-ui") {
        let engine = app.engine.clone();
        std::thread::Builder::new()
            .name("motion-mcp".into())
            .spawn(move || {
                if let Err(e) = mcp::serve(
                    engine,
                    Some(Box::new(move || {
                        let _ = proxy.send_event(UserEvent::EngineChanged);
                    })),
                ) {
                    eprintln!("MCP: {e}");
                }
            })?;
    }
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.fatal {
        return Err(error.into());
    }
    Ok(())
}
fn directories() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("MotionStudio");
    }
    #[cfg(target_os = "macos")]
    if let Some(base) = std::env::var_os("HOME") {
        return PathBuf::from(base).join("Library/Application Support/MotionStudio");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(base).join("motion-studio");
        }
        if let Some(base) = std::env::var_os("HOME") {
            return PathBuf::from(base).join(".local/share/motion-studio");
        }
    }
    std::env::temp_dir().join("motion-studio")
}
fn total_memory() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
                .map(|v| v * 1024)
        })
        .unwrap_or(8 * 1024 * 1024 * 1024)
}
enum UserEvent {
    EngineChanged,
}
struct Shell {
    window: Option<Arc<Window>>,
    engine: Arc<Engine>,
    atlas: TextAtlas,
    workspace: workspace::Workspace,
    layout_path: PathBuf,
    floating: std::collections::HashMap<WindowId, FloatWindow>,
    next_window: u64,
    panels: panels::Panels,
    input: Input,
    state: Value,
    scale: f32,
    dirty: bool,
    pending: bool,
    frame: f64,
    playback: Option<(Instant, f64)>,
    attaching: Option<std::sync::mpsc::Receiver<Result<Value, String>>>,
    attached: bool,
    rendering: std::collections::HashMap<u64, std::sync::mpsc::Receiver<Result<bool, String>>>,
    resizing: Option<std::sync::mpsc::Receiver<Result<Value, String>>>,
    resize_queue: std::collections::HashMap<u64, [u32; 2]>,
    actions: std::collections::VecDeque<panels::Action>,
    closing: bool,
    next_redraw: Instant,
    fatal: Option<String>,
    smoke: Option<PathBuf>,
    smoke_frames: u32,
    smoke_stage: u8,
    smoke_float_frames: std::collections::HashMap<u32, u32>,
    started: Instant,
}
struct FloatWindow {
    window: Arc<Window>,
    key: u64,
    panel: dock::Panel,
    input: Input,
    scale: f32,
}

impl Shell {
    fn new(root: PathBuf, smoke: Option<PathBuf>, locale: Option<Locale>) -> Result<Self, String> {
        let engine = Arc::new(Engine::start(
            root,
            aem_desktop_media::platform(),
            total_memory(),
        )?);
        let state = engine.state()?;
        let mut workspace = workspace::Workspace::load(&directories().join("desktop-layout.json"));
        if let Some(locale) = locale {
            workspace.locale = locale;
        }
        let chosen = workspace.locale;
        let mut shell = Self {
            window: None,
            engine,
            atlas: TextAtlas::from_system_language(18.0, chosen == Locale::Zh)?,
            workspace,
            layout_path: directories().join("desktop-layout.json"),
            floating: Default::default(),
            next_window: 1,
            panels: Default::default(),
            input: Default::default(),
            state,
            scale: 1.0,
            dirty: true,
            pending: false,
            frame: 0.0,
            playback: None,
            attaching: None,
            attached: false,
            rendering: Default::default(),
            resizing: None,
            resize_queue: Default::default(),
            actions: Default::default(),
            closing: false,
            next_redraw: Instant::now(),
            fatal: None,
            smoke,
            smoke_frames: 0,
            smoke_stage: 0,
            smoke_float_frames: Default::default(),
            started: Instant::now(),
        };
        shell.panels.locale = chosen;
        if shell.smoke.is_some() {
            shell.workspace = workspace::Workspace::default();
            shell.workspace.locale = chosen;
            shell.prepare_smoke()?;
        }
        Ok(shell)
    }
    fn prepare_smoke(&mut self) -> Result<(), String> {
        self.add_solid()?;
        let object = self.panels.selected.unwrap();
        self.state = self.engine.edit(
            serde_json::to_string(&vec![
                Command::Animate {
                    object,
                    property: aem_core::Property::Position,
                    axis: None,
                    frame: 0,
                    enabled: true,
                },
                Command::SetVector {
                    object,
                    property: aem_core::Property::Position,
                    frame: 90,
                    value: [1280.0, 540.0, 0.0],
                },
            ])
            .map_err(|e| e.to_string())?,
        )?;
        self.state = self.engine.seek(45.0)?;
        self.frame = 45.0;
        self.state = self.engine.save()?;
        let root = PathBuf::from(self.state["root"].as_str().ok_or("missing project root")?);
        let mut other = Project::new(1920, 1080, 30, 300).map_err(|e| e.to_string())?;
        other.name = "Reopen check".into();
        self.state = self
            .engine
            .new_project(serde_json::to_string(&other).map_err(|e| e.to_string())?)?;
        self.state = self.engine.open_directory(root)?;
        self.state = self.engine.seek(45.0)?;
        Ok(())
    }
    fn add_solid(&mut self) -> Result<(), String> {
        let layers = self.state["project"]["layers"]
            .as_array()
            .ok_or("missing layers")?;
        let id = layers
            .iter()
            .filter_map(|l| l["id"].as_u64())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("layer id overflow")?;
        let w = self.state["project"]["width"].as_f64().unwrap_or(1920.0) as f32;
        let h = self.state["project"]["height"].as_f64().unwrap_or(1080.0) as f32;
        let layer = Layer::solid(
            id,
            &format!("{} {id}", self.workspace.locale.t(T::Solid)),
            [w * 0.3, h * 0.3],
            [w * 0.5, h * 0.5, 0.0],
            [0.22, 0.6, 0.96, 1.0],
        );
        self.state = self
            .engine
            .edit(serde_json::to_string(&Command::Add { layer }).map_err(|e| e.to_string())?)?;
        self.panels.selected = Some(id);
        Ok(())
    }
    fn action(
        &mut self,
        action: panels::Action,
        event_loop: Option<&ActiveEventLoop>,
    ) -> Result<(), String> {
        use panels::Action;
        match action {
            Action::RefreshState => {
                self.stop();
                self.state = self.engine.state()?;
                self.frame = self.state["frame"].as_f64().unwrap_or(0.0);
            }
            Action::Language(locale) => {
                self.atlas = TextAtlas::from_system_language(18.0, locale == Locale::Zh)?;
                self.workspace.locale = locale;
                self.panels.locale = locale;
                for f in self.floating.values() {
                    f.window
                        .set_title(&format!("{} — Motion Studio", locale.panel(f.panel.dock())));
                }
                if self.smoke.is_none() {
                    self.workspace.save(&self.layout_path)?;
                }
                self.panels.message = Some(locale.t(T::Ready).into());
            }
            Action::ResetWorkspace => {
                let ids: Vec<_> = self.floating.values().map(|f| f.panel.dock()).collect();
                for id in ids {
                    self.redock_panel(id)?;
                }
                self.workspace.root = dock::editor_layout();
                self.workspace.floating.clear();
                self.workspace.cancel();
                if self.smoke.is_none() {
                    self.workspace.save(&self.layout_path)?;
                }
            }
            Action::ShowPanel(id) => {
                if let Some(f) = self.floating.values().find(|f| f.panel.dock() == id) {
                    f.window.focus_window();
                } else if let Some(dock::Node::Dock { id: dock, .. }) = self.workspace.root.find(id)
                {
                    let dock = *dock;
                    self.workspace.root.activate(dock, id);
                }
            }
            Action::About => {
                self.panels.message = Some(self.workspace.locale.t(T::AboutDescription).into())
            }
            Action::PreviewMode(mode) => {
                self.state["preview"] = self.engine.preview_mode(mode, 0)?
            }
            Action::SelectAt(point) => {
                self.stop();
                let result = self.engine.hit_candidates(point[0], point[1])?;
                self.panels.selected = result["candidates"]
                    .as_array()
                    .and_then(|c| c.first())
                    .and_then(|c| c["id"].as_u64());
            }
            Action::Float(panel) => {
                self.float_panel(panel, event_loop.ok_or("window event loop is unavailable")?)?;
            }
            Action::Redock(panel) => {
                self.redock_panel(panel)?;
            }
            Action::Activate { dock, tab } => {
                self.workspace.root.activate(dock, tab);
                if self.smoke.is_none() {
                    self.workspace.save(&self.layout_path)?;
                }
            }
            Action::OpenPath(path) => {
                self.stop();
                self.state = if path.is_dir() {
                    self.engine.open_directory(path)?
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("aem"))
                {
                    self.engine.import_project(path)?
                } else {
                    self.engine.open_directory(
                        path.parent()
                            .ok_or("project file has no parent")?
                            .to_path_buf(),
                    )?
                };
                self.frame = 0.0;
                self.panels.selected = None;
            }
            Action::Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(self.workspace.locale.t(T::OpenProject))
                    .add_filter("Motion Studio Project", &["json", "aem"])
                    .pick_file()
                {
                    self.stop();
                    self.state = if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("aem"))
                    {
                        self.engine.import_project(path)?
                    } else {
                        self.engine.open_directory(
                            path.parent()
                                .ok_or("project file has no parent")?
                                .to_path_buf(),
                        )?
                    };
                    self.frame = self.state["frame"].as_f64().unwrap_or(0.0);
                    self.panels.selected = None;
                    self.panels.message = Some(self.workspace.locale.t(T::ProjectOpened).into());
                }
            }
            Action::New => {
                self.stop();
                let mut project = Project::new(1920, 1080, 30, 300).map_err(|e| e.to_string())?;
                project.name = self.workspace.locale.t(T::Untitled).into();
                self.state = self
                    .engine
                    .new_project(serde_json::to_string(&project).map_err(|e| e.to_string())?)?;
                self.frame = 0.0;
                self.panels.selected = None;
                self.panels.message = Some(self.workspace.locale.t(T::NewComposition).into());
            }
            Action::Save => {
                self.state = self.engine.save()?;
                self.panels.message = Some(format!(
                    "{}: {}",
                    self.workspace.locale.t(T::Saved),
                    self.state["root"].as_str().unwrap_or("")
                ));
            }
            Action::Pack => {
                let result = self.engine.pack()?;
                self.panels.message = Some(format!(
                    "{}: {}",
                    self.workspace.locale.t(T::ProjectPackage),
                    result["path"].as_str().unwrap_or("")
                ));
            }
            Action::History(op) => {
                self.stop();
                self.state = self.engine.history(op)?;
            }
            Action::Seek(frame) => {
                self.stop();
                self.frame = frame;
                self.state = self.engine.seek(frame)?;
            }
            Action::TogglePlay => {
                if self.playback.is_some() {
                    self.stop();
                } else {
                    self.playback = Some((Instant::now(), self.frame));
                    self.panels.playing = true;
                }
            }
            Action::AddSolid => {
                self.stop();
                self.add_solid()?;
            }
            Action::Edit(command) => {
                self.stop();
                self.state = self
                    .engine
                    .edit(serde_json::to_string(&command).map_err(|e| e.to_string())?)?;
            }
        }
        self.dirty = true;
        Ok(())
    }
    fn stop(&mut self) {
        self.playback = None;
        self.panels.playing = false;
    }
    fn translate(&mut self, event: Event) {
        aem_ui::ui::accumulate(&mut self.input, event);
        self.dirty = true;
        self.redraw();
    }
    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        for f in self.floating.values() {
            f.window.request_redraw();
        }
    }
    fn size(&self) -> Option<[f32; 2]> {
        self.window.as_ref().map(|w| {
            let s = w.inner_size();
            [s.width as f32 / self.scale, s.height as f32 / self.scale]
        })
    }
    fn draw_commands(&mut self, size: [f32; 2]) -> (PaintList, Vec<panels::Action>) {
        let mut paint = PaintList::default();
        let actions = panels::chrome(
            &mut paint,
            &mut self.atlas,
            size,
            &self.workspace.root,
            &mut self.panels,
            &self.state,
            &self.input,
        );
        self.input.begin_frame(); // Clear transient input only after it has been consumed.
        (paint, actions)
    }
    fn ui_frame(&mut self, paint: PaintList, size: [f32; 2], force_atlas: bool) -> UiFrame {
        let config = self.atlas.config();
        let atlas = if self.atlas.take_dirty() || force_atlas {
            Some(AtlasUpload {
                pixels: self.atlas.pixel_data().to_vec(),
                width: config.width,
                height: config.height,
            })
        } else {
            None
        };
        let view = panels::composition_view(&self.workspace.root, size);
        UiFrame {
            paint,
            atlas,
            logical_size: size,
            window: 0,
            composition: if view.width() > 0.0 && view.height() > 0.0 {
                Some([
                    view.min[0] * self.scale,
                    view.min[1] * self.scale,
                    view.width() * self.scale,
                    view.height() * self.scale,
                ])
            } else {
                None
            },
        }
    }
    fn present(&mut self, _event_loop: &ActiveEventLoop) -> Result<(), String> {
        if !self.actions.is_empty()
            || !self.attached
            || self.rendering.contains_key(&0)
            || self.resizing.is_some()
            || !self.resize_queue.is_empty()
        {
            return Ok(());
        }
        let Some(size) = self.size() else {
            return Ok(());
        };
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Ok(());
        }
        if self.rendering.is_empty() {
            if let Some((start, origin)) = self.playback {
                self.frame = playback_frame(
                    origin,
                    start.elapsed(),
                    self.state["project"]["fps"].as_u64().unwrap_or(30) as u32,
                    self.state["project"]["frames"].as_u64().unwrap_or(1) as u32,
                );
                self.state = self.engine.seek(self.frame)?;
            }
        }
        if let Some(change) = self.workspace.interact(&self.input, size) {
            match change {
                workspace::Change::Float(panel) => {
                    self.actions.push_back(panels::Action::Float(panel))
                }
                workspace::Change::Changed => {
                    if self.smoke.is_none() && self.input.released.is_some() {
                        self.workspace.save(&self.layout_path)?;
                    }
                    self.dirty = true;
                }
            }
        }
        let (mut paint, actions) = self.draw_commands(size);
        self.actions.extend(actions);
        if !self.actions.is_empty() {
            return Ok(());
        }
        // Actions may change project, layer selection or playhead during this draw.
        if self.dirty {
            let neutral = Input::default();
            paint = PaintList::default();
            let _ = panels::chrome(
                &mut paint,
                &mut self.atlas,
                size,
                &self.workspace.root,
                &mut self.panels,
                &self.state,
                &neutral,
            );
        }
        let ui = self.ui_frame(paint, size, false);
        self.rendering.insert(0, self.engine.present_async(ui)?);
        self.dirty = false;
        Ok(())
    }
    fn float_panel(
        &mut self,
        id: dock::DockId,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        if self.floating.values().any(|f| f.panel.dock() == id) {
            return Ok(());
        }
        let panel = panels::panel(id).ok_or("invalid panel")?;
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!(
                            "{} — Motion Studio",
                            self.workspace.locale.panel(panel.dock())
                        ))
                        .with_inner_size(winit::dpi::LogicalSize::new(720.0, 520.0))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(360.0, 320.0))
                        .with_visible(self.smoke.is_none()),
                )
                .map_err(|e| e.to_string())?,
        );
        let size = window.inner_size();
        let key = self.next_window;
        self.next_window += 1;
        self.engine
            .float_window(key, window.clone(), size.width, size.height)?;
        let scale = window.scale_factor() as f32;
        self.workspace.float(id);
        self.floating.insert(
            window.id(),
            FloatWindow {
                window,
                key,
                panel,
                input: Default::default(),
                scale,
            },
        );
        if self.smoke.is_none() {
            self.workspace.save(&self.layout_path)?;
        }
        self.dirty = true;
        self.redraw();
        Ok(())
    }
    fn redock_panel(&mut self, panel: dock::DockId) -> Result<(), String> {
        if let Some(id) = self
            .floating
            .iter()
            .find(|(_, f)| f.panel.dock() == panel)
            .map(|(id, _)| *id)
        {
            if let Some(f) = self.floating.remove(&id) {
                self.engine.close_window(f.key)?;
            }
        }
        self.workspace.redock(panel);
        if self.smoke.is_none() {
            self.workspace.save(&self.layout_path)?;
        }
        self.dirty = true;
        self.redraw();
        Ok(())
    }
    fn present_float(&mut self, id: WindowId, _event_loop: &ActiveEventLoop) -> Result<(), String> {
        if !self.actions.is_empty()
            || !self.attached
            || self.resizing.is_some()
            || !self.resize_queue.is_empty()
        {
            return Ok(());
        }
        let f = self.floating.get_mut(&id).ok_or("floating window closed")?;
        if self.rendering.contains_key(&f.key) {
            return Ok(());
        }
        let physical = f.window.inner_size();
        if physical.width == 0 || physical.height == 0 {
            return Ok(());
        }
        let size = [
            physical.width as f32 / f.scale,
            physical.height as f32 / f.scale,
        ];
        let panel = f.panel;
        let key = f.key;
        let scale = f.scale;
        let mut paint = PaintList::default();
        let actions = panels::floating_chrome(
            &mut paint,
            &mut self.atlas,
            panel,
            size,
            &mut self.panels,
            &self.state,
            &f.input,
        );
        f.input.begin_frame();
        self.actions.extend(actions);
        if !self.actions.is_empty() {
            return Ok(());
        }
        if !self.floating.contains_key(&id) {
            return Ok(());
        }
        let config = self.atlas.config();
        let atlas = if self.atlas.take_dirty() {
            Some(AtlasUpload {
                pixels: self.atlas.pixel_data().to_vec(),
                width: config.width,
                height: config.height,
            })
        } else {
            None
        };
        let composition = panels::floating_view(panel, size).map(|v| {
            [
                v.min[0] * scale,
                v.min[1] * scale,
                v.width() * scale,
                v.height() * scale,
            ]
        });
        self.rendering.insert(
            key,
            self.engine.present_async(UiFrame {
                paint,
                atlas,
                logical_size: size,
                composition,
                window: key,
            })?,
        );
        Ok(())
    }
    fn floating_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(f) = self.floating.get_mut(&id) else {
            return;
        };
        let input_event = match event {
            WindowEvent::CloseRequested => {
                self.actions
                    .push_back(panels::Action::Redock(f.panel.dock()));
                return;
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.present_float(id, event_loop) {
                    self.panels.message = Some(self.workspace.locale.error(&e));
                }
                return;
            }
            WindowEvent::Resized(size) => {
                let key = f.key;
                if size.width > 0 && size.height > 0 {
                    self.resize_queue.insert(key, [size.width, size.height]);
                }
                self.dirty = true;
                self.redraw();
                return;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                f.scale = scale_factor as f32;
                self.dirty = true;
                self.redraw();
                return;
            }
            WindowEvent::ModifiersChanged(m) => {
                let m = m.state();
                f.input.modifiers = Modifiers {
                    control: m.control_key() || m.super_key(),
                    shift: m.shift_key(),
                    alt: m.alt_key(),
                };
                return;
            }
            WindowEvent::CursorMoved { position, .. } => Some(Event::MouseMoved {
                position: [position.x as f32 / f.scale, position.y as f32 / f.scale],
            }),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Right => MouseButton::Right,
                    _ => MouseButton::Middle,
                };
                Some(if state == ElementState::Pressed {
                    Event::MousePressed {
                        position: f.input.mouse,
                        button,
                    }
                } else {
                    Event::MouseReleased {
                        position: f.input.mouse,
                        button,
                    }
                })
            }
            WindowEvent::MouseWheel { delta, .. } => Some(Event::MouseWheel {
                delta: match delta {
                    MouseScrollDelta::LineDelta(x, y) => [-x * 32.0, -y * 32.0],
                    MouseScrollDelta::PixelDelta(p) => [p.x as f32 / f.scale, p.y as f32 / f.scale],
                },
            }),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(key) = translate_key(&event.logical_key) {
                    let modifiers = f.input.modifiers;
                    aem_ui::ui::accumulate(&mut f.input, Event::KeyPressed { key, modifiers });
                }
                event
                    .text
                    .filter(|t| !t.chars().all(char::is_control) && !f.input.modifiers.control)
                    .map(|t| Event::TextInput(t.to_string()))
            }
            WindowEvent::Focused(false) => {
                self.actions
                    .extend(panels::cancel_interaction(&mut self.panels));
                Some(Event::FocusLost)
            }
            _ => None,
        };
        if let Some(event) = input_event {
            aem_ui::ui::accumulate(&mut f.input, event);
            self.dirty = true;
            self.redraw();
        }
    }
    fn finish_smoke(&mut self) -> Result<(), String> {
        let path = self.smoke.as_ref().ok_or("no smoke report path")?.clone();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let size = self.size().ok_or("window has no size")?;
        let (paint, _) = self.draw_commands(size);
        let ui = self.ui_frame(paint, size, true);
        let screenshot = path.with_extension("png");
        self.engine.capture_ui(ui, screenshot.clone())?;
        let state = self.engine.state()?;
        let result = serde_json::json!({"ok":true,"framesPresented":state["presented"],"frame":state["frame"],"preview":state["preview"],
            "root":state["root"],"screenshot":screenshot,"floatingFrames":self.smoke_float_frames,"locale":self.workspace.locale,"desktop":state["desktop"],"checks":["create_layer","animate","seek","save","reopen","single_surface_preview","chrome","floating_properties","floating_composition","resize","redock"]});
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl ApplicationHandler<UserEvent> for Shell {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        self.actions.push_back(panels::Action::RefreshState);
        self.dirty = true;
        self.redraw();
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Motion Studio")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0))
            .with_visible(self.smoke.is_none());
        let result = (|| {
            let window = Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|e| e.to_string())?,
            );
            self.scale = window.scale_factor() as f32;
            let size = window.inner_size();
            self.attaching = Some(self.engine.attach_async(
                window.clone(),
                size.width,
                size.height,
            )?);
            self.window = Some(window);
            self.dirty = true;
            self.redraw();
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.fatal = Some(error);
            event_loop.exit();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.floating.contains_key(&id) {
            self.floating_event(event_loop, id, event);
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.actions
                    .extend(panels::cancel_interaction(&mut self.panels));
                self.stop();
                self.closing = true;
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.resize_queue.insert(0, [size.width, size.height]);
                    self.dirty = true;
                    self.redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor as f32;
                self.dirty = true;
                self.redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let m = modifiers.state();
                self.input.modifiers = Modifiers {
                    control: m.control_key() || m.super_key(),
                    shift: m.shift_key(),
                    alt: m.alt_key(),
                };
            }
            WindowEvent::CursorMoved { position, .. } => self.translate(Event::MouseMoved {
                position: [
                    position.x as f32 / self.scale,
                    position.y as f32 / self.scale,
                ],
            }),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Right => MouseButton::Right,
                    _ => MouseButton::Middle,
                };
                let position = self.input.mouse;
                self.translate(if state == ElementState::Pressed {
                    Event::MousePressed { position, button }
                } else {
                    Event::MouseReleased { position, button }
                });
            }
            WindowEvent::MouseWheel { delta, .. } => self.translate(Event::MouseWheel {
                delta: match delta {
                    MouseScrollDelta::LineDelta(x, y) => [-x * 32.0, -y * 32.0],
                    MouseScrollDelta::PixelDelta(p) => {
                        [p.x as f32 / self.scale, p.y as f32 / self.scale]
                    }
                },
            }),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(key) = translate_key(&event.logical_key) {
                    self.translate(Event::KeyPressed {
                        key,
                        modifiers: self.input.modifiers,
                    });
                }
                if !self.input.modifiers.control && !self.input.modifiers.alt {
                    if let Some(text) = event.text.filter(|t| !t.chars().all(char::is_control)) {
                        self.translate(Event::TextInput(text.to_string()));
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.workspace.cancel();
                self.stop();
                self.actions
                    .extend(panels::cancel_interaction(&mut self.panels));
                self.translate(Event::FocusLost);
            }
            WindowEvent::DroppedFile(path) => {
                self.actions.push_back(panels::Action::OpenPath(path));
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.present(event_loop) {
                    self.pending = false;
                    self.stop();
                    self.panels.message = Some(self.workspace.locale.error(&error));
                    if self.smoke.is_some() {
                        self.fatal = Some(error);
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.smoke.is_some() && self.started.elapsed() > Duration::from_secs(30) {
            self.fatal = Some("desktop smoke timed out".into());
            event_loop.exit();
            return;
        }
        if let Some(receive) = &self.attaching {
            if let Ok(result) = receive.try_recv() {
                self.attaching = None;
                match result {
                    Ok(state) => {
                        self.state = state;
                        self.attached = true;
                        self.dirty = true;
                        let restore = self.workspace.floating.clone();
                        self.workspace.floating.clear();
                        for panel in restore {
                            self.workspace.redock(panel);
                            if let Err(e) = self.float_panel(panel, event_loop) {
                                self.panels.message = Some(self.workspace.locale.error(&e));
                            }
                        }
                    }
                    Err(error) => {
                        self.fatal = Some(error);
                        event_loop.exit();
                        return;
                    }
                }
            }
        }
        let mut completed = vec![];
        for (key, receive) in &self.rendering {
            if let Ok(result) = receive.try_recv() {
                completed.push((*key, result));
            }
        }
        for (key, result) in completed {
            self.rendering.remove(&key);
            match result {
                Ok(rendered) => {
                    self.pending = !rendered;
                    if rendered && self.smoke.is_some() {
                        if key == 0 {
                            self.smoke_frames += 1;
                            self.pending = true;
                        } else if let Some(f) = self.floating.values().find(|f| f.key == key) {
                            *self.smoke_float_frames.entry(f.panel.dock().0).or_default() += 1;
                        }
                    }
                }
                Err(e) => {
                    self.stop();
                    self.panels.message = Some(self.workspace.locale.error(&e));
                    if self.smoke.is_some() {
                        self.fatal = Some(e);
                        event_loop.exit();
                        return;
                    }
                }
            }
        }
        if let Some(receive) = &self.resizing {
            if let Ok(result) = receive.try_recv() {
                self.resizing = None;
                match result {
                    Ok(state) => self.state = state,
                    Err(e) => self.panels.message = Some(e),
                }
                self.dirty = true;
            }
        }
        if self.attached && self.rendering.is_empty() && self.resizing.is_none() {
            while let Some(action) = self.actions.pop_front() {
                if let Err(e) = self.action(action, Some(event_loop)) {
                    self.panels.message = Some(self.workspace.locale.error(&e));
                    self.stop();
                }
                self.redraw();
            }
            if let Some((&key, &size)) = self.resize_queue.iter().next() {
                self.resize_queue.remove(&key);
                match self.engine.resize_async(key, size[0], size[1]) {
                    Ok(receive) => self.resizing = Some(receive),
                    Err(e) => self.panels.message = Some(e),
                }
            }
            if self.closing {
                match self.engine.save() {
                    Ok(_) => {
                        event_loop.exit();
                        return;
                    }
                    Err(e) => {
                        self.closing = false;
                        self.panels.message = Some(self.workspace.locale.error(&e));
                        self.dirty = true;
                    }
                }
            }
            if self.smoke.is_some() {
                let step = (|| -> Result<(), String> {
                    if self.smoke_stage == 0 && self.smoke_frames >= 2 {
                        self.float_panel(dock::DockId(2), event_loop)?;
                        if let Some(f) = self
                            .floating
                            .values()
                            .find(|f| f.panel == dock::Panel::EffectControls)
                        {
                            let _ = f
                                .window
                                .request_inner_size(winit::dpi::LogicalSize::new(640.0, 560.0));
                        }
                        self.smoke_stage = 1;
                    } else if self.smoke_stage == 1
                        && self.smoke_float_frames.get(&2).copied().unwrap_or(0) >= 2
                    {
                        self.float_panel(dock::DockId(3), event_loop)?;
                        self.smoke_stage = 2;
                    } else if self.smoke_stage == 2
                        && self.smoke_float_frames.get(&3).copied().unwrap_or(0) >= 2
                    {
                        self.redock_panel(dock::DockId(2))?;
                        self.redock_panel(dock::DockId(3))?;
                        let locale = self.workspace.locale;
                        self.workspace = workspace::Workspace::default();
                        self.workspace.locale = locale;
                        self.smoke_stage = 3;
                        self.dirty = true;
                    } else if self.smoke_stage == 3 && self.smoke_frames >= 6 {
                        self.finish_smoke()?;
                        self.smoke_stage = 4;
                    }
                    Ok(())
                })();
                if let Err(e) = step {
                    self.fatal = Some(e);
                    event_loop.exit();
                    return;
                }
                if self.smoke_stage == 4 {
                    event_loop.exit();
                    return;
                }
            }
        }
        if self.smoke.is_some()
            && self.attached
            && self.rendering.is_empty()
            && self.resizing.is_none()
            && self.resize_queue.is_empty()
        {
            // Windows may suppress RedrawRequested for a hidden acceptance window.
            if let Err(e) = self.present(event_loop) {
                self.fatal = Some(e);
                event_loop.exit();
                return;
            }
            let floating: Vec<_> = self.floating.keys().copied().collect();
            for id in floating {
                if let Err(e) = self.present_float(id, event_loop) {
                    self.fatal = Some(e);
                    event_loop.exit();
                    return;
                }
            }
        }
        let busy = self.attaching.is_some()
            || !self.rendering.is_empty()
            || self.resizing.is_some()
            || !self.resize_queue.is_empty()
            || !self.actions.is_empty();
        if busy || self.pending || self.playback.is_some() || self.dirty {
            if self.attached
                && self.resizing.is_none()
                && self.resize_queue.is_empty()
                && (self.dirty || Instant::now() >= self.next_redraw)
            {
                self.redraw();
                let rate = self.state["project"]["fps"]
                    .as_f64()
                    .unwrap_or(30.0)
                    .min(self.state["preview"]["fps"].as_f64().unwrap_or(60.0))
                    .clamp(1.0, 120.0);
                self.next_redraw = Instant::now() + Duration::from_secs_f64(1.0 / rate);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(if busy { 4 } else { 16 }),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}
fn playback_frame(origin: f64, elapsed: Duration, fps: u32, frames: u32) -> f64 {
    (origin + elapsed.as_secs_f64() * f64::from(fps.max(1))).rem_euclid(f64::from(frames.max(1)))
}
fn translate_key(key: &WinitKey) -> Option<Key> {
    Some(match key {
        WinitKey::Character(s) => match s.to_ascii_lowercase().as_str() {
            "n" => Key::N,
            "o" => Key::O,
            "s" => Key::S,
            "z" => Key::Z,
            "y" => Key::Y,
            "d" => Key::D,
            "," => Key::Comma,
            "." => Key::Period,
            _ => return None,
        },
        WinitKey::Named(k) => match k {
            NamedKey::Escape => Key::Escape,
            NamedKey::Enter => Key::Enter,
            NamedKey::Tab => Key::Tab,
            NamedKey::Space => Key::Space,
            NamedKey::Delete => Key::Delete,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            _ => return None,
        },
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_wraps_without_sampling_the_invalid_end_frame() {
        assert_eq!(playback_frame(29.0, Duration::from_secs(1), 30, 30), 29.0);
        for ms in [0, 33, 1000, 100000] {
            let f = playback_frame(179.0, Duration::from_millis(ms), 60, 180);
            assert!((0.0..180.0).contains(&f));
        }
    }
    #[test]
    fn keyboard_chords_use_character_keys() {
        assert_eq!(
            translate_key(&WinitKey::Character("S".into())),
            Some(Key::S)
        );
    }
}
