//! Pixel handoff and independent export readers; no frontend or activity code.
use super::video_decode::DecodedFrame;
use super::video_frames::VideoFrames;
use super::*;
use std::sync::Arc;
struct FrozenVideo {
    project: Project,
    root: PathBuf,
    frames: VideoFrames,
}
static VIDEO: OnceLock<Mutex<HashMap<i64, FrozenVideo>>> = OnceLock::new();
static VIDEO_NEXT: AtomicI64 = AtomicI64::new(1);
fn frozen() -> &'static Mutex<HashMap<i64, FrozenVideo>> {
    VIDEO.get_or_init(|| Mutex::new(HashMap::new()))
}
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jint {
    match jni::JavaVM::from_raw(vm) {
        Ok(vm) => {
            super::video_decode::set_vm(vm);
            jni::sys::JNI_VERSION_1_6
        }
        Err(_) => jni::sys::JNI_ERR,
    }
}
fn copy_frame(
    env: &mut JNIEnv,
    buffer: &JByteBuffer,
    frame: Arc<DecodedFrame>,
    object: i64,
    sequence: i64,
) -> Result<Value> {
    if env
        .call_method(buffer, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z())
        .map_err(|e| e.to_string())?
    {
        return Err("video output buffer must be writable".into());
    }
    let address = env
        .get_direct_buffer_address(buffer)
        .map_err(|e| e.to_string())?;
    let capacity = env
        .get_direct_buffer_capacity(buffer)
        .map_err(|e| e.to_string())?;
    if address.is_null() || capacity < frame.rgba.len() {
        return Err(format!(
            "video requires a direct buffer with {} bytes",
            frame.rgba.len()
        ));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(frame.rgba.as_ptr(), address, frame.rgba.len());
    }
    Ok(
        json!({"object":object,"sequence":sequence,"width":frame.width,"height":frame.height,"bytes":frame.rgba.len(),"format":"rgba8","pts_us":frame.pts,"end_us":frame.end,"decode_us":frame.decode_us,"source_transfer":frame.source_transfer,"decoder":frame.decoder_name}),
    )
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
        let frame = with_session(id, |s| {
            s.video_frames
                .frame(s.engine.project(), object as u64, sequence as u64)
        })?;
        copy_frame(&mut env, &buffer, frame, object, sequence)
    })();
    string_result(&mut env, || result)
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_freezeVideo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let mut readers = frozen()
                .lock()
                .map_err(|_| "frozen video registry poisoned")?;
            if readers.len() >= 4 {
                return Err("at most four frozen video readers; release finished handles".into());
            }
            let handle = VIDEO_NEXT.fetch_add(1, Ordering::Relaxed);
            readers.insert(
                handle,
                FrozenVideo {
                    project: s.engine.snapshot(),
                    root: s.root.clone(),
                    frames: VideoFrames::default(),
                },
            );
            Ok(
                json!({"handle":handle,"revision":s.engine.revision(),"project":s.engine.snapshot()}),
            )
        })
    })
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
        let mut readers = frozen()
            .lock()
            .map_err(|_| "frozen video registry poisoned")?;
        let r = readers
            .get_mut(&handle)
            .ok_or("frozen video reader closed")?;
        r.frames
            .request(&r.project, &r.root, object as u64, frame, sequence as u64)
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
        if object <= 0 || sequence <= 0 {
            return Err("invalid video object/sequence".into());
        }
        let frame = {
            let readers = frozen()
                .lock()
                .map_err(|_| "frozen video registry poisoned")?;
            let r = readers.get(&handle).ok_or("frozen video reader closed")?;
            r.frames.frame(&r.project, object as u64, sequence as u64)?
        };
        copy_frame(&mut env, &buffer, frame, object, sequence)
    })();
    string_result(&mut env, || result)
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_releaseFrozenVideo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    string_result(&mut env, || {
        Ok(
            json!({"released":frozen().lock().map_err(|_|"frozen video registry poisoned")?.remove(&handle).is_some()}),
        )
    })
}
