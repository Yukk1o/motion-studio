//! Motion Studio desktop application.
//!
//! The shell is a native winit window. The composition preview is presented by
//! `aem-host` straight into the swapchain, and the panel chrome is drawn by
//! `aem-ui` over the same target, so there is no readback and no copy between
//! the engine and the screen.
//!
//! Layout follows After Effects: Project on the left, Effect Controls and
//! Composition on the right, Timeline across the bottom, with a menu bar and a
//! context toolbar above.

mod engine;
mod panels;

use aem_host::Platform;
use aem_ui::{
    dock::{self, Node},
    input::{Event, Input, Key, MouseButton, Rect},
    paint::PaintList,
    text::TextAtlas,
    theme::metrics,
    Painter,
};
use serde_json::Value;
use std::{path::PathBuf, rc::Rc, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key as WinitKey, NamedKey},
    window::{Window, WindowId},
};

use engine::Engine;

/// How long a press must be held before it becomes a long press.
const LONG_PRESS_MS: u128 = 400;
/// Redraw cadence while idle, so the preview keeps its VSync pacing.
const IDLE_FRAME_MS: u128 = 8;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(
        Instant::now() + std::time::Duration::from_millis(IDLE_FRAME_MS),
    ));
    let mut app = Shell::new(project_root(), aem_desktop_media::platform())?;
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Default project directory, overridable so a build can start on a scratch file.
fn project_root() -> PathBuf {
    match std::env::var_os("MOTION_PROJECT") {
        Some(path) => PathBuf::from(path),
        None => directories().join("default"),
    }
}

fn directories() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(base).join("MotionStudio");
    }
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Library/Application Support/MotionStudio");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(data).join("motion-studio");
    } else if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/motion-studio");
    }
    std::env::temp_dir().join("motion-studio")
}

/// Total physical memory in bytes.
///
/// Desktop GPUs do not expose device memory through wgpu, so machine memory is
/// the honest upper bound. It keeps the scratch budget policy identical to the
/// Android host's `configureMemory` call.
pub fn total_memory() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("MemTotal:"))
                .and_then(|line| {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|value| value.parse::<u64>().ok())
                })
                .map(|kib| kib * 1024)
        })
        .unwrap_or(8 * 1024 * 1024 * 1024)
}

struct Shell {
    window: Option<Rc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    painter: Option<Painter>,
    atlas: TextAtlas,
    engine: Option<Engine>,
    platform: Arc<dyn Platform>,
    layout: Node,
    panels: panels::Panels,
    paint: PaintList,
    input: Input,
    state: Option<Value>,
    error: Option<String>,
    surface_format: wgpu::TextureFormat,
    /// Force a chrome redraw even when the preview is idle.
    dirty: bool,
    frame: f64,
    playing: bool,
    press: Option<Press>,
    scale: f32,
}

struct Press {
    button: MouseButton,
    started: Instant,
    position: [f32; 2],
    promoted: bool,
}

impl Shell {
    fn new(root: PathBuf, platform: Arc<dyn Platform>) -> Result<Self, String> {
        let atlas = TextAtlas::from_system(13.0)?;
        let engine = Engine::start(root, platform.clone(), total_memory()).ok();
        Ok(Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            painter: None,
            atlas,
            engine,
            platform,
            layout: dock::editor_layout(),
            panels: panels::Panels::default(),
            paint: PaintList::default(),
            input: Input::default(),
            state: None,
            error: None,
            surface_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            dirty: true,
            frame: 0.0,
            playing: false,
            press: None,
            scale: 1.0,
        })
    }

    fn refresh_state(&mut self) {
        let Some(engine) = &self.engine else {
            return;
        };
        match engine.state() {
            Ok(state) => {
                self.state = Some(state);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        self.dirty = true;
    }

    /// Draw the panel chrome for one frame.
    fn draw_chrome(&mut self) {
        let (Some(size), Some(device), Some(queue)) = (
            self.window.as_ref().map(|w| w.inner_size()),
            self.device.as_ref(),
            self.queue.as_ref(),
        ) else {
            return;
        };
        let Some(painter) = self.painter.as_mut() else {
            return;
        };
        self.paint = PaintList::default();
        self.input.begin_frame();
        panels::chrome(
            &mut self.paint,
            &mut self.atlas,
            self.layout_area(size),
            &self.layout,
            &self.panels,
            self.state.as_ref(),
            &self.input,
            self.scale,
        );
        if self.atlas.take_dirty() {
            let config = self.atlas.config();
            painter.upload_atlas(
                device,
                queue,
                self.atlas.pixel_data(),
                config.width,
                config.height,
            );
        }
        self.dirty = false;
    }

    fn layout_area(&self, size: winit::dpi::PhysicalSize<u32>) -> Rect {
        let chrome = panels::CHROME_HEIGHT * self.scale;
        Rect::new(
            0.0,
            chrome,
            size.width as f32 / self.scale,
            (size.height as f32 / self.scale - chrome).max(0.0),
        )
    }

    /// Present the composition frame, then draw the chrome over it.
    fn present(&mut self) {
        if self.dirty {
            self.draw_chrome();
        }
        let (Some(window), Some(device), Some(queue)) = (
            self.window.clone(),
            self.device.as_ref(),
            self.queue.as_ref(),
        ) else {
            return;
        };
        let size = window.inner_size();
        let scale = self.scale;
        // The engine owns its own surface for the composition. It presents into
        // the same window; the chrome is composited by the OS compositor layer,
        // so this pass only draws when the window is exposed.
        let Ok(frame) = self
            .surface
            .as_ref()
            .expect("surface is created in resumed")
            .get_current_texture()
        else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Motion Studio panel chrome"),
        });
        if let Some(painter) = self.painter.as_mut() {
            painter.render(
                device,
                queue,
                &mut encoder,
                &view,
                &self.paint,
                [size.width as f32 / scale, size.height as f32 / scale],
            );
        }
        queue.submit(Some(encoder.finish()));
        frame.present();
    }

    fn translate(&mut self, event: Event) {
        aem_ui::ui::accumulate(&mut self.input, event);
        self.dirty = true;
    }

    fn bind_surface(&mut self) {
        let (Some(window), Some(engine)) = (self.window.clone(), self.engine.as_ref()) else {
            return;
        };
        let size = window.inner_size();
        let (Ok(width), Ok(height)) = (u32::try_from(size.width), u32::try_from(size.height))
        else {
            return;
        };
        if width == 0 || height == 0 {
            return;
        }
        engine.detach();
        if let Ok(state) = engine.attach(
            self.platform.as_ref(),
            Box::new(window),
            width,
            height,
        ) {
            self.state = Some(state);
        }
        self.dirty = true;
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Motion Studio")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                self.error = Some(format!("cannot open a window: {error}"));
                event_loop.exit();
                return;
            }
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                self.error = Some(format!("cannot create a window surface: {error}"));
                event_loop.exit();
                return;
            }
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }));
        let Some(adapter) = adapter else {
            self.error = Some("no compatible GPU adapter was found".into());
            event_loop.exit();
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Motion Studio desktop device"),
            required_features: adapter.features(),
            required_limits: wgpu::Limits::downlevel_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("device request failed");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        self.surface_format = format;
        self.painter = Some(Painter::new(&device, format));
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface = Some(surface);
        self.window = Some(window);
        self.refresh_state();
        self.bind_surface();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.bind_surface(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = aem_ui::Scale::clamped(scale_factor as f32).0;
                self.dirty = true;
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
                match state {
                    ElementState::Pressed => {
                        self.press = Some(Press {
                            button,
                            started: Instant::now(),
                            position,
                            promoted: false,
                        });
                        self.translate(Event::MousePressed { position, button });
                    }
                    ElementState::Released => {
                        self.press = None;
                        self.translate(Event::MouseReleased { position, button });
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self.scale;
                self.translate(Event::MouseWheel {
                    delta: match delta {
                        MouseScrollDelta::LineDelta(_, y) => [0.0, -y * 40.0 * scale],
                        MouseScrollDelta::PixelDelta(position) => {
                            [position.x as f32, position.y as f32]
                        }
                    },
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(key) = translate_key(&event.logical_key) {
                    self.translate(Event::KeyPressed {
                        key,
                        modifiers: aem_ui::input::Modifiers {
                            shift: event.modifiers.shift(),
                            control: event.modifiers.control(),
                            alt: event.modifiers.alt(),
                        },
                    });
                }
                if let Some(text) = event
                    .text
                    .as_ref()
                    .filter(|text| !text.chars().all(char::is_control))
                {
                    self.translate(Event::TextInput(text.clone()));
                }
            }
            WindowEvent::Focused(false) => self.translate(Event::FocusLost),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Promote a held press into a long press once the threshold passes. The
        // timeline uses this to tell a scrub from an intended clip move, which is
        // the same rule the touch build applies.
        if let Some(press) = self.press.as_mut() {
            if !press.promoted && press.started.elapsed().as_millis() as u128 >= LONG_PRESS_MS {
                press.promoted = true;
                let position = press.position;
                self.translate(Event::LongPress { position });
            }
        }
        self.present();
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + std::time::Duration::from_millis(IDLE_FRAME_MS),
        ));
    }
}

fn translate_key(key: &WinitKey) -> Option<Key> {
    Some(match key {
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Space) => Key::Space,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::Left,
        WinitKey::Named(NamedKey::ArrowRight) => Key::Right,
        WinitKey::Named(NamedKey::ArrowUp) => Key::Up,
        WinitKey::Named(NamedKey::ArrowDown) => Key::Down,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::Comma) => Key::Comma,
        WinitKey::Named(NamedKey::Period) => Key::Period,
        WinitKey::Named(NamedKey::ShiftLeft | NamedKey::ShiftRight) => Key::Shift,
        WinitKey::Named(NamedKey::ControlLeft | NamedKey::ControlRight) => Key::Control,
        WinitKey::Named(NamedKey::AltLeft | NamedKey::AltRight) => Key::Alt,
        _ => return None,
    })
}