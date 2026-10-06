//! MediaBridge is a backend boundary. No activity, UI or file picker required.
use super::*;
use aem_media::{AudioMixer, ImportOptions, MAX_BLOCK_FRAMES};
use jni::objects::JValue;
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::{FromRawFd, RawFd},
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    MediaCapabilities,
    ImportMedia {
        request_id: String,
        uri: String,
        kind: String,
        #[serde(default)]
        at_frame: u32,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        track: Option<u32>,
        #[serde(default = "with_audio")]
        with_audio: bool,
        #[serde(default)]
        audio_track: Option<u32>,
    },
    ProbeMedia {
        request_id: String,
        uri: String,
        kind: String,
        #[serde(default)]
        track: Option<u32>,
        #[serde(default = "with_audio")]
        with_audio: bool,
        #[serde(default)]
        audio_track: Option<u32>,
    },
    MediaStatus {
        request_id: String,
    },
    FinishMediaImport {
        request_id: String,
    },
    CancelMediaImport {
        request_id: String,
    },
    ReleaseMediaTask {
        request_id: String,
    },
    PrepareAudio {
        request_id: String,
        asset: u64,
    },
    AudioWaveform {
        asset: u64,
        #[serde(default)]
        first_bucket: u64,
        count: usize,
    },
    PrepareVideo {
        request_id: String,
        asset: u64,
    },
    VideoThumbnail {
        asset: u64,
    },
    RequestVideoFrame {
        object: u64,
        frame: f64,
        sequence: u64,
    },
    ReleaseVideoFrames,
}
fn with_audio() -> bool {
    true
}
fn audio_name() -> String {
    "音频".into()
}

fn java_error(env: &mut JNIEnv, error: jni::errors::Error) -> String {
    if env.exception_check().unwrap_or(false) {
        let exception = env.exception_occurred();
        let _ = env.exception_clear();
        if let Ok(exception) = exception {
            if let Ok(text) = env.call_method(exception, "toString", "()Ljava/lang/String;", &[]) {
                if let Ok(text) = text.l() {
                    if let Ok(s) = env.get_string(&JString::from(text)) {
                        return s.into();
                    }
                }
            }
            let _ = env.exception_clear();
        }
    }
    error.to_string()
}
fn uri_reader(
    env: &mut JNIEnv,
    context: &JObject,
    uri: &str,
) -> Result<(Box<dyn Read + Send>, Option<u64>)> {
    if uri.len() > 8192 || !(uri.starts_with("content://") || uri.starts_with("file://")) {
        return Err("media URI must be content:// or file://".into());
    }
    let text = env.new_string(uri).map_err(|e| java_error(env, e))?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&text)],
        )
        .map_err(|e| java_error(env, e))?
        .l()
        .map_err(|e| e.to_string())?;
    let resolver = env
        .call_method(
            context,
            "getContentResolver",
            "()Landroid/content/ContentResolver;",
            &[],
        )
        .map_err(|e| java_error(env, e))?
        .l()
        .map_err(|e| e.to_string())?;
    let mode = env.new_string("r").map_err(|e| java_error(env, e))?;
    let afd = env
        .call_method(
            resolver,
            "openAssetFileDescriptor",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/res/AssetFileDescriptor;",
            &[JValue::Object(&uri), JValue::Object(&mode)],
        )
        .map_err(|e| java_error(env, e))?
        .l()
        .map_err(|e| e.to_string())?;
    if afd.is_null() {
        return Err("media provider returned no readable descriptor".into());
    }
    let result = (|| {
        let offset = env
            .call_method(&afd, "getStartOffset", "()J", &[])
            .map_err(|e| java_error(env, e))?
            .j()
            .map_err(|e| e.to_string())?;
        let length = env
            .call_method(&afd, "getDeclaredLength", "()J", &[])
            .map_err(|e| java_error(env, e))?
            .j()
            .map_err(|e| e.to_string())?;
        if offset < 0 || length < -1 {
            return Err("invalid provider descriptor range".into());
        }
        let pfd = env
            .call_method(
                &afd,
                "getParcelFileDescriptor",
                "()Landroid/os/ParcelFileDescriptor;",
                &[],
            )
            .map_err(|e| java_error(env, e))?
            .l()
            .map_err(|e| e.to_string())?;
        let fd = env
            .call_method(pfd, "getFd", "()I", &[])
            .map_err(|e| java_error(env, e))?
            .i()
            .map_err(|e| e.to_string())?;
        let duplicate: RawFd = unsafe { libc::dup(fd) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut file = unsafe { File::from_raw_fd(duplicate) };
        if offset > 0 {
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|e| e.to_string())?;
        }
        let bytes = if length >= 0 {
            Some(length as u64)
        } else {
            file.metadata()
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len().saturating_sub(offset as u64))
        };
        let reader: Box<dyn Read + Send> = if let Some(bytes) = bytes {
            Box::new(file.take(bytes))
        } else {
            Box::new(file)
        };
        Ok((reader, bytes))
    })();
    if let Err(e) = env.call_method(&afd, "close", "()V", &[]) {
        let _ = java_error(env, e);
    }
    result
}
fn uri_opener(
    env: &mut JNIEnv,
    context: &JObject,
    uri: String,
) -> Result<impl FnOnce() -> Result<(Box<dyn Read + Send>, Option<u64>)> + Send + 'static> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let context = env
        .new_global_ref(context)
        .map_err(|e| java_error(env, e))?;
    Ok(move || {
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let result = uri_reader(&mut env, context.as_obj(), &uri);
        drop(context); // Release the global reference while the thread is attached.
        result
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_request(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    context: JObject,
    text: JString,
) -> jstring {
    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let text=read_string(&mut env,&text)?;if text.len()>16*1024{return Err("media request too large".into());}
        let request:Request=serde_json::from_str(&text).map_err(|e|e.to_string())?;
        match request {
            Request::MediaCapabilities=>super::media_capabilities::query(&mut env),
            Request::ImportMedia{request_id,uri,kind,at_frame,name,track,with_audio,audio_track} => {
                if kind!="audio"&&kind!="video"{return Err("media kind must be audio or video".into());}
                let name=name.unwrap_or_else(||if kind=="video"{"视频".into()}else{audio_name()});
                with_session(id,|s|{if s.audio_jobs.contains(&request_id)||s.video_jobs.contains(&request_id){return Err("media request id already exists".into());}Ok(())})?;
                with_session(id,|s| {if at_frame>=s.engine.project().frames{return Err("audio insertion outside composition".into());}Ok(())})?;
                let open=uri_opener(&mut env,&context,uri)?;
                if kind=="video" {return with_session(id,|s|Ok(json!(s.video_jobs.start(open,aem_media::VideoImportOptions{request_id,at_frame,name,track,audio_track,with_audio},false,std::sync::Arc::new(super::video_decode::probe))?)));}
                with_session(id,|s|Ok(json!(s.audio_jobs.start_with_source(open,ImportOptions{request_id,at_frame,name,track},false)?)))
            }
            Request::ProbeMedia{request_id,uri,kind,track,with_audio,audio_track} => {
                if kind!="audio"&&kind!="video"{return Err("media kind must be audio or video".into());}
                with_session(id,|s|{if s.audio_jobs.contains(&request_id)||s.video_jobs.contains(&request_id){return Err("media request id already exists".into());}Ok(())})?;
                let open=uri_opener(&mut env,&context,uri)?;
                if kind=="video" {return with_session(id,|s|Ok(json!(s.video_jobs.start(open,aem_media::VideoImportOptions{request_id,at_frame:0,name:"视频".into(),track,audio_track,with_audio},true,std::sync::Arc::new(super::video_decode::probe))?)));}
                with_session(id,|s|Ok(json!(s.audio_jobs.start_with_source(open,ImportOptions{request_id,at_frame:0,name:audio_name(),track},true)?)))
            }
            Request::MediaStatus{request_id}=>with_session(id,|s|if s.video_jobs.contains(&request_id){Ok(json!(s.video_jobs.status(&request_id)?))}else{Ok(json!(s.audio_jobs.status(&request_id)?))}),
            Request::FinishMediaImport{request_id}=>with_session(id,|s| {
                let task=if s.video_jobs.contains(&request_id){json!(s.video_jobs.commit(&request_id,&mut s.engine)?)}else{json!(s.audio_jobs.commit(&request_id,&mut s.engine)?)};s.audio_mixer=None;
                s.sample()?;Ok(json!({"task":task,"state":s.snapshot()}))
            }),
            Request::CancelMediaImport{request_id}=>with_session(id,|s|if s.video_jobs.contains(&request_id){Ok(json!(s.video_jobs.cancel(&request_id)?))}else{Ok(json!(s.audio_jobs.cancel(&request_id)?))}),
            Request::ReleaseMediaTask{request_id}=>with_session(id,|s|{if s.video_jobs.contains(&request_id){s.video_jobs.release(&request_id)?;}else{s.audio_jobs.release(&request_id)?;}Ok(json!({"released":request_id}))}),
            Request::PrepareAudio{request_id,asset}=>with_session(id,|s|{
                if s.video_jobs.contains(&request_id){return Err("media request id already exists".into());}
                let asset=s.engine.project().audio_assets.iter().find(|a|a.id==asset).cloned().ok_or("audio asset missing")?;
                Ok(json!(s.audio_jobs.prepare_cache(&request_id,asset)?))
            }),
            Request::AudioWaveform{asset,first_bucket,count}=>with_session(id,|s|{
                let a=s.engine.project().audio_assets.iter().find(|a|a.id==asset).ok_or("audio asset missing")?;
                Ok(json!({"asset":asset,"first_bucket":first_bucket,"bucket_duration_us":10000,"source_duration_us":a.duration_us,
                    "buckets":aem_media::read_waveform(&s.root,a,first_bucket,count)?}))
            }),
            Request::PrepareVideo{request_id,asset}=>with_session(id,|s|{
                if s.audio_jobs.contains(&request_id){return Err("media request id already exists".into());}
                let asset=s.engine.project().video_assets.iter().find(|a|a.id==asset).cloned().ok_or("video asset missing")?;
                s.video_frames.clear();Ok(json!(s.video_jobs.prepare_cache(&request_id,asset,std::sync::Arc::new(super::video_decode::probe))?))
            }),
            Request::VideoThumbnail{asset}=>with_session(id,|s|{
                let a=s.engine.project().video_assets.iter().find(|a|a.id==asset).ok_or("video asset missing")?;
                let path=aem_media::video_cache_path(&s.root,a)?.with_extension("png");let path=path.canonicalize().map_err(|_|"thumbnail missing; call prepare_video")?;
                if !path.starts_with(s.root.canonicalize().map_err(|e|e.to_string())?){return Err("thumbnail outside project".into());}
                Ok(json!({"asset":asset,"path":path,"source_time_us":a.video_start_us,"width":a.display_width,"height":a.display_height}))
            }),
            Request::RequestVideoFrame{object,frame,sequence}=>with_session(id,|s|s.video_frames.request(s.engine.project(),&s.root,object,frame,sequence)),
            Request::ReleaseVideoFrames=>with_session(id,|s|{s.video_frames.clear();Ok(json!({"released":true}))}),
        }
    })).unwrap_or_else(|_|Err("media request failed".into()));
    string_result(&mut env, || result)
}
struct Frozen {
    mixer: AudioMixer,
    pcm: Vec<f32>,
}
static AUDIO_NEXT: AtomicI64 = AtomicI64::new(1);
static FROZEN: OnceLock<Mutex<HashMap<i64, Frozen>>> = OnceLock::new();
fn frozen() -> &'static Mutex<HashMap<i64, Frozen>> {
    FROZEN.get_or_init(|| Mutex::new(HashMap::new()))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_freezeAudio(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let mut streams = frozen()
                .lock()
                .map_err(|_| "audio stream registry poisoned")?;
            if streams.len() >= 8 {
                return Err("release frozen audio streams before creating more".into());
            }
            let mixer = AudioMixer::new(s.engine.snapshot(), &s.root)?;
            let total = mixer.total_frames();
            let handle = AUDIO_NEXT.fetch_add(1, Ordering::Relaxed);
            streams.insert(
                handle,
                Frozen {
                    mixer,
                    pcm: Vec::new(),
                },
            );
            Ok(
                json!({"handle":handle,"total_frames":total,"sample_rate":48000,"channels":2,"revision":s.engine.revision()}),
            )
        })
    })
}
#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_MediaBridge_releaseFrozenAudio(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    string_result(&mut env, || {
        let removed = frozen()
            .lock()
            .map_err(|_| "audio stream registry poisoned")?
            .remove(&handle)
            .is_some();
        Ok(json!({"released":removed}))
    })
}
fn output_buffer(
    env: &mut JNIEnv,
    buffer: &JByteBuffer,
    frames: jint,
    start: jlong,
) -> Result<(*mut u8, usize)> {
    if frames <= 0 || frames as usize > MAX_BLOCK_FRAMES || start < 0 {
        return Err("invalid PCM block range".into());
    }
    if env
        .call_method(buffer, "isReadOnly", "()Z", &[])
        .map_err(|e| java_error(env, e))?
        .z()
        .map_err(|e| e.to_string())?
    {
        return Err("PCM output buffer must be writable".into());
    }
    let len = frames as usize * 8;
    let address = env
        .get_direct_buffer_address(buffer)
        .map_err(|e| e.to_string())?;
    let capacity = env
        .get_direct_buffer_capacity(buffer)
        .map_err(|e| e.to_string())?;
    if capacity < len || address.is_null() {
        return Err("PCM output requires a sufficient direct ByteBuffer".into());
    }
    Ok((address, len))
}
fn copy_pcm(address: *mut u8, pcm: &[f32], count: usize, start: u64, total: u64) -> Value {
    for (i, value) in pcm[..count * 2].iter().enumerate() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), address.add(i * 4), 4);
        }
    }
    json!({"frames":count,"bytes":count*8,"sample_rate":48000,"channels":2,"format":"f32le_interleaved",
        "start_sample":start,"pts_us":start*1_000_000/48000,"end_of_stream":start+count as u64==total})
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
    let output = output_buffer(&mut env, &buffer, frames, start);
    string_result(&mut env, || {
        let (address, _) = output?;
        with_session(id, |s| {
            if s.audio_mixer
                .as_ref()
                .is_none_or(|(rev, _)| *rev != s.engine.revision())
            {
                s.audio_mixer = Some((
                    s.engine.revision(),
                    AudioMixer::new(s.engine.snapshot(), &s.root)?,
                ));
            }
            s.audio_pcm.resize(frames as usize * 2, 0.0);
            let count = s
                .audio_mixer
                .as_mut()
                .unwrap()
                .1
                .mix(start as u64, &mut s.audio_pcm)?;
            Ok(copy_pcm(
                address,
                &s.audio_pcm,
                count,
                start as u64,
                s.audio_mixer.as_ref().unwrap().1.total_frames(),
            ))
        })
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
    let output = output_buffer(&mut env, &buffer, frames, start);
    string_result(&mut env, || {
        let (address, _) = output?;
        let mut streams = frozen()
            .lock()
            .map_err(|_| "audio stream registry poisoned")?;
        let stream = streams
            .get_mut(&handle)
            .ok_or("frozen audio stream is closed")?;
        stream.pcm.resize(frames as usize * 2, 0.0);
        let count = stream.mixer.mix(start as u64, &mut stream.pcm)?;
        Ok(copy_pcm(
            address,
            &stream.pcm,
            count,
            start as u64,
            stream.mixer.total_frames(),
        ))
    })
}
