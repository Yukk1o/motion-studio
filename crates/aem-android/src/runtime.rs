use aem_core::{Command, Engine, Observer, Project, Scene};
use aem_render::{CaptureTarget, Presenter, Renderer};
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
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    thread::{self, ThreadId},
};

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
    scratch: CaptureTarget,
    presenter: Presenter,
    config: wgpu::SurfaceConfiguration,
    _instance: wgpu::Instance,
}
struct Session {
    engine: Engine,
    scene: Scene,
    observer: Observer,
    observing: bool,
    frame: f64,
    root: PathBuf,
    graphics: Option<Graphics>,
    owner: ThreadId,
    presented: u64,
    last_cpu_us: u64,
    last_error: Option<String>,
}
impl Session {
    fn new(project: Project, root: PathBuf) -> Result<Self> {
        let mut scene = Scene::new(&project);
        scene
            .sample(&project, 0.0, None)
            .map_err(|e| e.to_string())?;
        let observer = Observer::new(project.width, project.height);
        Ok(Self {
            engine: Engine::new(project).map_err(|e| e.to_string())?,
            scene,
            observer,
            observing: false,
            frame: 0.0,
            root,
            graphics: None,
            owner: thread::current().id(),
            presented: 0,
            last_cpu_us: 0,
            last_error: None,
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
        if let Some(g) = self.graphics.take() {
            g.renderer.device.poll(wgpu::Maintain::Wait);
            drop(g);
        }
    }
    fn attach(&mut self, window: NativeWindow, width: u32, height: u32) -> Result<()> {
        self.detach();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        // Surface owns a NativeWindow clone via its safe raw-window-handle implementation.
        let surface = instance
            .create_surface(AndroidWindow(window))
            .map_err(|e| e.to_string())?;
        let mut renderer = pollster::block_on(Renderer::new(
            &instance,
            Some(&surface),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ))
        .map_err(|e| e.to_string())?;
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
        let scratch = renderer
            .capture_target(width, height)
            .map_err(|e| e.to_string())?;
        let presenter = Presenter::new(&renderer, &scratch.view, format);
        self.graphics = Some(Graphics {
            surface,
            renderer,
            scratch,
            presenter,
            config,
            _instance: instance,
        });
        self.last_error = None;
        Ok(())
    }
    fn render(&mut self, frame: f64) -> Result<bool> {
        self.frame = frame;
        self.sample()?;
        let Some(g) = &mut self.graphics else {
            return Ok(false);
        };
        let output = match g.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                g.surface.configure(&g.renderer.device, &g.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(error) => return Err(format!("surface rendering failed: {error}")),
        };
        let stats = g
            .renderer
            .draw(
                &self.scene,
                &g.scratch.view,
                g.config.width,
                g.config.height,
            )
            .map_err(|e| e.to_string())?;
        let view = output.texture.create_view(&Default::default());
        g.presenter.draw(&g.renderer, &view);
        output.present();
        self.presented += 1;
        self.last_cpu_us = stats.cpu_prepare_us;
        Ok(true)
    }
    fn sample(&mut self) -> Result<()> {
        self.scene
            .sample(
                self.engine.project(),
                self.frame,
                if self.observing {
                    Some(&self.observer)
                } else {
                    None
                },
            )
            .map_err(|e| e.to_string())
    }
    fn snapshot(&self) -> Value {
        let p = self.engine.project();
        let f = self.frame;
        let camera = json!({"position":p.camera.position_at(f),"target":p.camera.target.sample(f),
            "fov":p.camera.fov.sample(f),"roll":p.camera.roll.sample(f),"radius":p.camera.radius.sample(f),
            "azimuth":p.camera.azimuth.sample(f),"elevation":p.camera.elevation.sample(f)});
        let layers: Vec<_> = p
            .layers
            .iter()
            .map(|l| {
                json!({"id":l.id,"position":l.transform.position.sample(f),
            "rotation":l.transform.rotation.sample(f),"scale":l.transform.scale.sample(f),
            "opacity":l.transform.opacity.sample(f)})
            })
            .collect();
        let projected: Vec<_> = self
            .scene
            .layers
            .iter()
            .filter_map(|layer| {
                let mvp = self.scene.camera.view_projection * layer.model;
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
                    .map(|l| self.scene.project_point(l.transform.position.sample(f)));
                Some(
                    json!({"id":layer.id,"anchor":anchor,"corners":corners.map(|c|[
                (c.x/c.w*0.5+0.5)*p.width as f32,
                (0.5-c.y/c.w*0.5)*p.height as f32])}),
                )
            })
            .collect();
        json!({"project":p,"root":self.root.to_string_lossy(),"frame":f,"revision":self.engine.revision(),"canUndo":self.engine.can_undo(),
            "canRedo":self.engine.can_redo(),"observing":self.observing,"sampledCamera":camera,
            "sampledLayers":layers,"projectedLayers":projected,"presented":self.presented,"cpuPrepareUs":self.last_cpu_us,
            "renderError":self.last_error})
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
        Ok(Err(error)) => json!({"ok":false,"error":error}),
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
fn read_string(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Result<String> {
    env.get_string(text)
        .map(|s| s.into())
        .map_err(|e| e.to_string())
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
                Project::demo()
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
    result.ok().and_then(std::result::Result::ok).unwrap_or(0)
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
            let commands: Vec<Command> = if text.trim_start().starts_with('[') {
                serde_json::from_str(&text).map_err(|e| e.to_string())?
            } else {
                vec![serde_json::from_str(&text).map_err(|e| e.to_string())?]
            };
            let resources = commands.iter().any(|c| {
                matches!(
                    c,
                    Command::RegisterAsset { .. } | Command::Content { .. } | Command::Add { .. }
                )
            });
            if resources {
                let mut check = Engine::new(s.engine.snapshot()).map_err(|e| e.to_string())?;
                check
                    .apply_batch(commands.clone())
                    .map_err(|e| e.to_string())?;
                aem_core::storage::validate_assets(&s.root, check.project())
                    .map_err(|e| e.to_string())?;
            }
            s.engine.apply_batch(commands).map_err(|e| e.to_string())?;
            if resources {
                if let Some(g) = &mut s.graphics {
                    if let Err(error) = g.renderer.synchronize_assets(s.engine.project(), &s.root) {
                        s.engine.undo().map_err(|e| e.to_string())?;
                        g.renderer.clear_assets();
                        let _ = g.renderer.synchronize_assets(s.engine.project(), &s.root);
                        return Err(error.to_string());
                    }
                }
            }
            s.sample()?;
            Ok(s.snapshot())
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
            let position = layer.transform.position.sample(s.frame);
            let offset = s
                .scene
                .screen_translation(
                    position,
                    [dx as f32, dy as f32],
                    [width as u32, height as u32],
                )
                .map_err(|e| e.to_string())?;
            s.engine
                .apply(Command::SetVector {
                    object: object as u64,
                    property: aem_core::Property::Position,
                    frame: s.frame.floor() as u32,
                    value: std::array::from_fn(|i| position[i] + offset[i]),
                })
                .map_err(|e| e.to_string())?;
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
            if let Some(g) = &mut s.graphics {
                g.renderer
                    .synchronize_assets(s.engine.project(), &s.root)
                    .map_err(|e| e.to_string())?;
            }
            s.sample()?;
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
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_render(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jboolean {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_session(id, |s| match s.render(frame) {
            Ok(rendered) => Ok(rendered),
            Err(error) => {
                s.last_error = Some(error);
                Ok(false)
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
            renderer
                .synchronize_assets(p, &s.root)
                .map_err(|e| e.to_string())?;
            let target = renderer
                .capture_target(p.width, p.height)
                .map_err(|e| e.to_string())?;
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
            if let Some(g) = &mut s.graphics {
                g.renderer
                    .replace_assets(engine.project(), &s.root)
                    .map_err(|e| e.to_string())?;
            }
            s.scene = Scene::new(engine.project());
            s.observer = Observer::new(engine.project().width, engine.project().height);
            s.engine = engine;
            s.frame = 0.0;
            s.observing = false;
            s.sample()?;
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
            if let Some(g) = &mut s.graphics {
                g.renderer
                    .replace_assets(engine.project(), &destination)
                    .map_err(|e| e.to_string())?;
            }
            s.scene = Scene::new(engine.project());
            s.observer = Observer::new(engine.project().width, engine.project().height);
            s.engine = engine;
            s.root = destination;
            s.frame = 0.0;
            s.observing = false;
            s.sample()?;
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
            s.frame = f64::from(frame);
            s.scene
                .sample(s.engine.project(), s.frame, None)
                .map_err(|e| e.to_string())?;
            let words = s.scene.layers.len() * 32;
            if capacity < words * 4 {
                return Err("draw parameter buffer is too small".into());
            }
            let output = unsafe { std::slice::from_raw_parts_mut(address, words * 4) };
            for (i, layer) in s.scene.layers.iter().enumerate() {
                let mut data = [0.0f32; 32];
                data[..16].copy_from_slice(
                    &(s.scene.camera.view_projection * layer.model).to_cols_array(),
                );
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
            Ok(s.scene.layers.len() as i32)
        })
    }));
    result.ok().and_then(std::result::Result::ok).unwrap_or(-1)
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
