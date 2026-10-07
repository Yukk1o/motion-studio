//! Pixel handoff and independent export readers; no frontend or activity code.
use crate::video_frame::DecodedFrame;
use super::video_frames::VideoFrames;
use super::*;
use std::sync::Arc;
struct FrozenVideo {
    project: Project,
    root: PathBuf,
    frames: VideoFrames,
    prepared:HashMap<u64,Arc<DecodedFrame>>,
    sequence:u64,
    time:f64,
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
    let required = frame.width as usize * frame.height as usize * 4;
    if address.is_null() || capacity < required {
        return Err(format!(
            "video requires a direct buffer with {} bytes",
            required
        ));
    }
    let pixels = frame.rgba()?;
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), address, pixels.len());
    }
    Ok(
        json!({"object":object,"sequence":sequence,"width":frame.width,"height":frame.height,"bytes":pixels.len(),"format":"rgba8","pts_us":frame.pts,"end_us":frame.end,"decode_us":frame.decode_us,"codec_us":frame.codec_us,"transfer_us":frame.transfer_us,"pack_us":frame.pack_us,"source_transfer":frame.source_transfer,"decoder":frame.decoder_name}),
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
                    prepared:Default::default(),sequence:0,time:-1.,
                },
            );
            Ok(
                json!({"handle":handle,"revision":s.engine.revision(),"project":s.engine.snapshot()}),
            )
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_requestFrozenCompositionFrame(mut env:JNIEnv,_class:JClass,handle:jlong,frame:jdouble,sequence:jlong)->jstring {
    string_result(&mut env,||{
        if sequence<=0{return Err("invalid frozen video sequence".into());}
        let mut readers=frozen().lock().map_err(|_|"frozen video registry poisoned")?;
        let r=readers.get_mut(&handle).ok_or("frozen video reader closed")?;
        if (sequence as u64)<r.sequence || (sequence as u64==r.sequence&&frame!=r.time) {return Err("frozen composition request superseded".into());}
        let mut scene=Scene::new(&r.project);scene.sample(&r.project,frame,None).map_err(|e|e.to_string())?;
        if sequence as u64!=r.sequence{r.prepared.clear();r.sequence=sequence as u64;r.time=frame;}
        match r.frames.prepare_scene_exact(&r.project,&r.root,&scene)?{
            None=>Ok(json!({"state":"pending","sequence":sequence})),
            Some(frames)=>{r.prepared=frames.into_iter().collect();Ok(json!({"state":"ready","sequence":sequence,"instances":r.prepared.keys().collect::<Vec<_>>()}))}
        }
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_readFrozenCompositionVideoInto(mut env:JNIEnv,_class:JClass,handle:jlong,object:jlong,sequence:jlong,buffer:JByteBuffer)->jstring {
    let result=(||{let readers=frozen().lock().map_err(|_|"frozen video registry poisoned")?;let r=readers.get(&handle).ok_or("frozen video reader closed")?;
        if sequence<=0||r.sequence!=sequence as u64{return Err("frozen composition video superseded".into());}
        let frame=r.prepared.get(&(object as u64)).cloned().ok_or("frozen composition video is pending or absent")?;
        copy_frame(&mut env,&buffer,frame,object,sequence)
    })();string_result(&mut env,||result)
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
