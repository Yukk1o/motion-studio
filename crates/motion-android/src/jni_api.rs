//! JNI entry points.
//!
//! Every function here does the same three things: decode JNI arguments, call
//! the matching [`motion_host`] operation, and encode the shared JSON envelope.
//! No editing logic lives on this side of the boundary.
use crate::bridge::{
    bytes_result, integer_result, is_read_only, read_string, string_result, BufferAccess,
};
use crate::platform::{install, AndroidPlatform};
use crate::{uri, video_decode};
use motion_host::{
    ops::{composition, editing, effects, export, geometry, images, media, preview, project},
    session, Session,
};
use jni::{
    objects::{JByteBuffer, JClass, JObject, JString},
    sys::{jboolean, jbyteArray, jdouble, jint, jlong, jstring},
    JNIEnv,
};
use std::path::PathBuf;
use std::sync::Arc;

type Error = String;

fn with<T>(
    id: jlong,
    operation: impl FnOnce(&mut Session) -> Result<T, Error>,
) -> Result<T, Error> {
    session::with_session(id, operation)
}

/// Platform handle used by endpoints that receive no `Context`.
fn host() -> Arc<AndroidPlatform> {
    install()
}

// ---------------------------------------------------------------------------
// NativeBridge: project lifecycle
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_resourceInfo(
    mut env: JNIEnv,
    _class: JClass,
    directory: JString,
) -> jstring {
    let parsed = read_string(&mut env, &directory);
    string_result(&mut env, || session::resource_info(&PathBuf::from(parsed?)))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_create(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
    project: JString,
) -> jlong {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<jlong, Error> {
            let root = PathBuf::from(read_string(&mut env, &root)?);
            let text = read_string(&mut env, &project)?;
            project::create(root, &text, host())
        }));
    match result {
        Ok(Ok(id)) => {
            session::set_creation_error(String::new());
            id
        }
        Ok(Err(error)) => {
            session::set_creation_error(error);
            0
        }
        Err(_) => {
            session::set_creation_error("原生工程初始化失败".into());
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_projectTemplate(
    mut env: JNIEnv,
    _class: JClass,
    kind: jint,
) -> jstring {
    string_result(&mut env, || project::template(kind))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_creationError(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let error = session::creation_error();
    env.new_string(error.as_str())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_state(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || project::state(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_save(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || project::save(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_newProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || project::new_project(id, &text?))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_openProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    directory: JString,
) -> jstring {
    let name = read_string(&mut env, &directory);
    string_result(&mut env, || project::open_project(id, &name?))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_replace(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || project::replace(id, &text?))
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
        project::import_project(id, &PathBuf::from(path?))
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_destroy(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session::close(id)));
}

// ---------------------------------------------------------------------------
// NativeBridge: editing
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_command(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || editing::command(id, &text?))
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
        editing::drag(id, object, dx, dy, width, height)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_history(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    op: jint,
) -> jstring {
    string_result(&mut env, || editing::history(id, op))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_curveGraph(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || editing::curve_graph(&text?))
}

// ---------------------------------------------------------------------------
// NativeBridge: preview and Surface binding
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_configureMemory(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    total_mem: jlong,
    guarded: jboolean,
) -> jstring {
    string_result(&mut env, || {
        preview::configure_memory(id, total_mem, guarded != 0)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_seek(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jstring {
    string_result(&mut env, || preview::seek(id, frame))
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
    string_result(&mut env, || preview::observe(id, enabled != 0, az, el))
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
        preview::navigate(id, dx, dy, zoom, multi != 0, width, height)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_view(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    kind: jint,
) -> jstring {
    string_result(&mut env, || preview::view(id, kind))
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
        unsafe {
            ndk::native_window::NativeWindow::from_surface(
                env.get_native_interface(),
                surface.as_raw(),
            )
        }
    };
    string_result(&mut env, || {
        with(id, |s| {
            if let Some(window) = window {
                if width <= 0 || height <= 0 {
                    return Err("surface dimensions must be positive".into());
                }
                // Android allows one producer per native window. Disconnect the
                // old swapchain before creating its replacement on this window.
                s.detach();
                let target = host().attach_native_window(window, width as u32, height as u32)?;
                s.attach(target)?;
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
    string_result(&mut env, || preview::preview_info(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_previewMode(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    mode: jint,
    thermal: jint,
) -> jstring {
    string_result(&mut env, || preview::preview_mode(id, mode, thermal))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_startProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    limit: jint,
) -> jstring {
    string_result(&mut env, || preview::start_profiling(id, limit))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_stopProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || preview::stop_profiling(id))
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
            with(id, |s| {
                use serde_json::json;
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
        // A queued UI frame may precede playback start or belong to the previous
        // composition. Skip it before touching sampled state; invalid timing is
        // not a GPU or device failure.
        with(id, |s| preview::render_inner(s, frame))
    }));
    u8::from(
        result
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or(false),
    )
}

// ---------------------------------------------------------------------------
// NativeBridge: export, images and effects
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_renderPlanInfo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || export::render_plan_info(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_sampleRenderPlanInto(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jint,
    buffer: JByteBuffer,
) -> jint {
    integer_result(|| {
        let (pointer, capacity) = BufferAccess::capacity_first(&env, &buffer)?;
        with(id, |s| {
            // JNI owns this direct buffer for the duration of the call.
            let out = unsafe { crate::bridge::output_bytes(pointer, capacity) };
            export::sample_render_plan_into_inner(s, frame, out).map(|n| n as i32)
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_capture(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || export::capture(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_pack(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || export::pack(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_sampleInto(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jint,
    buffer: JByteBuffer,
) -> jint {
    integer_result(|| {
        let (pointer, capacity) = BufferAccess::address_first(&env, &buffer)?;
        with(id, |s| {
            let out = unsafe { crate::bridge::output_bytes(pointer, capacity) };
            export::sample_into_inner(s, frame, out).map(|batches| batches as i32)
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
    bytes_result(&env, || export::asset_pixels(id, asset))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_imageInfo(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jstring {
    let path = read_string(&mut env, &path);
    string_result(&mut env, || images::image_info(&path?))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_prepareImage(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
    path: JString,
) -> jstring {
    let root = read_string(&mut env, &root);
    let path = read_string(&mut env, &path);
    string_result(&mut env, || {
        images::prepare_image(PathBuf::from(root?), path?)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_assetPixelsInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    asset: jlong,
    buffer: JByteBuffer,
) -> jstring {
    let writable = is_read_only(&mut env, &buffer);
    let access = BufferAccess::capture(&env, &buffer);
    string_result(&mut env, || {
        if writable? {
            return Err("image output buffer is read-only".into());
        }
        let (address, capacity) = access.resolve()?;
        with(id, |s| {
            if address.is_null() {
                return Err("invalid direct buffer".into());
            }
            let out = unsafe { crate::bridge::output_bytes(address, capacity) };
            images::asset_pixels_into_inner(s, asset, out)
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_plugin(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    request: JString,
) -> jstring {
    let request = read_string(&mut env, &request);
    string_result(&mut env, || effects::plugin(id, &request?))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_pluginPixels(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    program: jint,
    resource: jint,
) -> jbyteArray {
    bytes_result(&env, || effects::plugin_pixels(id, program, resource))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_colorCurveGraph(
    mut env: JNIEnv,
    _class: JClass,
    value: JString,
) -> jstring {
    let value = read_string(&mut env, &value);
    string_result(&mut env, || effects::color_curve_graph(&value?))
}

// ---------------------------------------------------------------------------
// GeometryBridge
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_GeometryBridge_hitCandidates(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    x: jdouble,
    y: jdouble,
) -> jstring {
    string_result(&mut env, || geometry::hit_candidates(id, x, y))
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
    // Capture buffer lookup before the writability check: this endpoint has
    // always reported read-only buffers before lookup failures.
    let parameter_access = BufferAccess::capture(&env, &parameters);
    let vertex_access = BufferAccess::capture(&env, &vertices);
    let parameter_readonly = is_read_only(&mut env, &parameters);
    let vertex_readonly = is_read_only(&mut env, &vertices);
    string_result(&mut env, || {
        if parameter_readonly? || vertex_readonly? {
            return Err("geometry buffers must be writable".into());
        }
        let (pa, pc) = parameter_access.resolve()?;
        let (va, vc) = vertex_access.resolve()?;
        with(id, |s| {
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
            let parameter_bytes = unsafe { crate::bridge::output_bytes(pa, pb) };
            let vertex_bytes = unsafe { crate::bridge::output_bytes(va, vb) };
            geometry::sample_geometry_into_inner(s, frame, parameter_bytes, vertex_bytes)
        })
    })
}

// ---------------------------------------------------------------------------
// CompositionBridge
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_CompositionBridge_request(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || composition::request(id, &text?))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_CompositionBridge_sampleFrameBundleInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    composition_id: JString,
    frame: jdouble,
    buffer: JByteBuffer,
) -> jint {
    let composition_id = read_string(&mut env, &composition_id);
    integer_result(|| {
        if is_read_only(&mut env, &buffer)? {
            return Err("frame bundle buffer is read-only".into());
        }
        let (address, capacity) = BufferAccess::capacity_first(&env, &buffer)?;
        let mut bundle = Vec::new();
        let len = composition::frame_bundle(id, &composition_id?, frame, &mut bundle)?;
        // A negative result reports the required size so Java can grow the buffer.
        if capacity < len {
            return Ok(-(len as i32));
        }
        if address.is_null() {
            return Err("invalid direct buffer".into());
        }
        unsafe { crate::bridge::copy_bytes(address, &bundle) };
        Ok(len as i32)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_CompositionBridge_render(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    composition_id: JString,
    frame: jdouble,
) -> jboolean {
    let composition_id = read_string(&mut env, &composition_id);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        composition::render(id, &composition_id?, frame)
    }));
    u8::from(
        result
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or(false),
    )
}

// ---------------------------------------------------------------------------
// MediaBridge
// ---------------------------------------------------------------------------

/// Copy a decoded frame into a Java-owned direct buffer and describe it.
fn copy_frame(
    env: &mut JNIEnv,
    buffer: &JByteBuffer,
    frame: &motion_host::video_frame::DecodedFrame,
    object: u64,
    sequence: u64,
) -> Result<serde_json::Value, Error> {
    let (address, capacity) = BufferAccess::address_first(env, buffer)?;
    let required = frame.width as usize * frame.height as usize * 4;
    if address.is_null() || capacity < required {
        return Err(format!(
            "video requires a direct buffer with {required} bytes"
        ));
    }
    let pixels = frame.rgba()?;
    if pixels.len() != required {
        return Err("decoded video RGBA dimensions do not match its pixel buffer".into());
    }
    unsafe { crate::bridge::copy_bytes(address, &pixels) };
    Ok(media::frame_report(frame, object, sequence))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_packageLimits(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    string_result(&mut env, || Ok(media::package_limits()))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_request(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    context: JObject,
    text: JString,
) -> jstring {
    let result = (|| -> Result<serde_json::Value, Error> {
        let text = read_string(&mut env, &text)?;
        let value = motion_host::ops::parse_request(&text, 16 * 1024, "media request")?;
        // Only import and probe open the selected document, and they must do it
        // on the import worker thread, so the opener is built lazily.
        let op = value["op"].as_str().unwrap_or("");
        let opener = if matches!(op, "import_media" | "probe_media") {
            let uri = value["path"]
                .as_str()
                .or_else(|| value["uri"].as_str())
                .ok_or("media request requires a path or URI")?;
            Some(uri::uri_opener(&mut env, &context, uri.to_owned())?)
        } else {
            None
        };
        media::request(id, value, opener)
    })();
    string_result(&mut env, || result)
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readVideoFrameInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    object: jlong,
    sequence: jlong,
    buffer: JByteBuffer,
) -> jstring {
    let result = (|| {
        if object <= 0 || sequence <= 0 {
            return Err("invalid video object/sequence".into());
        }
        let frame = media::read_video_frame_into(id, object as u64, sequence as u64)?;
        copy_frame(&mut env, &buffer, &frame, object as u64, sequence as u64)
    })();
    string_result(&mut env, || result)
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_freezeAudio(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || media::freeze_audio(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_releaseFrozenAudio(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    string_result(&mut env, || media::release_frozen_audio(handle))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readPcmInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    start: jlong,
    frames: jint,
    buffer: JByteBuffer,
) -> jstring {
    let writable = is_read_only(&mut env, &buffer);
    let access = BufferAccess::address_first(&mut env, &buffer);
    string_result(&mut env, || {
        if writable? {
            return Err("PCM output buffer must be writable".into());
        }
        if frames <= 0 || start < 0 {
            return Err("invalid PCM block range".into());
        }
        let (address, capacity) = access?;
        if address.is_null() {
            return Err("PCM output requires a sufficient direct ByteBuffer".into());
        }
        // JNI owns this direct buffer for the duration of the call.
        let out = unsafe { crate::bridge::output_bytes(address, capacity) };
        media::read_pcm_into(id, start as u64, frames as usize, out)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readFrozenPcmInto(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    start: jlong,
    frames: jint,
    buffer: JByteBuffer,
) -> jstring {
    let writable = is_read_only(&mut env, &buffer);
    let access = BufferAccess::address_first(&mut env, &buffer);
    string_result(&mut env, || {
        if writable? {
            return Err("PCM output buffer must be writable".into());
        }
        if frames <= 0 || start < 0 {
            return Err("invalid PCM block range".into());
        }
        let (address, capacity) = access?;
        if address.is_null() {
            return Err("PCM output requires a sufficient direct ByteBuffer".into());
        }
        let out = unsafe { crate::bridge::output_bytes(address, capacity) };
        media::read_frozen_pcm_into(handle, start as u64, frames as usize, out)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_freezeVideo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || media::freeze_video(id))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_releaseFrozenVideo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    string_result(&mut env, || media::release_frozen_video(handle))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_requestFrozenCompositionFrame(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    frame: jdouble,
    sequence: jlong,
) -> jstring {
    string_result(&mut env, || {
        media::request_frozen_composition_frame(handle, frame, sequence as u64)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readFrozenCompositionVideoInto(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    object: jlong,
    sequence: jlong,
    buffer: JByteBuffer,
) -> jstring {
    let result = (|| {
        let frame =
            media::read_frozen_composition_video_into(handle, object as u64, sequence as u64)?;
        copy_frame(&mut env, &buffer, &frame, object as u64, sequence as u64)
    })();
    string_result(&mut env, || result)
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_requestFrozenVideoFrame(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    object: jlong,
    frame: jdouble,
    sequence: jlong,
) -> jstring {
    string_result(&mut env, || {
        if object <= 0 || sequence <= 0 {
            return Err("invalid video object/sequence".into());
        }
        media::request_frozen_video_frame(handle, object as u64, frame, sequence as u64)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readFrozenVideoFrameInto(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    object: jlong,
    sequence: jlong,
    buffer: JByteBuffer,
) -> jstring {
    let result = (|| {
        let frame = media::read_frozen_video_frame_into(handle, object as u64, sequence as u64)?;
        copy_frame(&mut env, &buffer, &frame, object as u64, sequence as u64)
    })();
    string_result(&mut env, || result)
}

// ---------------------------------------------------------------------------
// JNI entry points
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jint {
    match jni::JavaVM::from_raw(vm) {
        Ok(vm) => {
            video_decode::set_vm(vm);
            install();
            jni::sys::JNI_VERSION_1_6
        }
        Err(_) => jni::sys::JNI_ERR,
    }
}
