//! Device decoder inventory is a capability hint, never a substitute for probing.
use aem_host::platform::VideoQuery;
use aem_media::Result;
use jni::{
    objects::{JObject, JObjectArray, JString, JValue},
    JNIEnv,
};
use serde_json::{json, Value};

impl VideoQuery {
    fn validate(&self) -> Result<()> {
        let mime = self.mime.as_deref().unwrap_or_default();
        if !mime.starts_with("video/")
            || !mime.is_ascii()
            || mime.len() > 128
            || !(1..=16384).contains(&self.width.unwrap_or(1))
            || !(1..=16384).contains(&self.height.unwrap_or(1))
        {
            return Err(
                "video_query requires MIME, dimensions 1..16384 and frame_rate >0..1000".into(),
            );
        }
        if let Some(rate) = self.frame_rate {
            if !matches!(rate, 1..=1000) {
                return Err(
                    "video_query requires MIME, dimensions 1..16384 and frame_rate >0..1000"
                        .into(),
                );
            }
        }
        Ok(())
    }
    fn backend_eligible(&self) -> bool {
        let Some(mime) = self.mime.as_deref() else {
            return false;
        };
        let width = self.width.unwrap_or(0);
        let height = self.height.unwrap_or(0);
        aem_media::VIDEO_MIMES.contains(&mime)
            && width <= aem_core::MAX_VIDEO_DIMENSION
            && height <= aem_core::MAX_VIDEO_DIMENSION
            && u64::from(width) * u64::from(height) <= aem_core::MAX_VIDEO_PIXELS
            && self.frame_rate.unwrap_or(0) <= aem_core::MAX_VIDEO_FPS
    }
    fn to_json(&self) -> Value {
        json!({"mime":self.mime,"width":self.width,"height":self.height,"frame_rate":self.frame_rate})
    }
}

fn range(
    env: &mut JNIEnv,
    caps: &JObject,
    method: &str,
    args: &[JValue],
) -> jni::errors::Result<Value> {
    let signature = if args.is_empty() {
        "()Landroid/util/Range;"
    } else {
        "(II)Landroid/util/Range;"
    };
    let range = env.call_method(caps, method, signature, args)?.l()?;
    let mut values = Vec::new();
    for bound in ["getLower", "getUpper"] {
        let number = env
            .call_method(&range, bound, "()Ljava/lang/Comparable;", &[])?
            .l()?;
        values.push(env.call_method(&number, "doubleValue", "()D", &[])?.d()?);
    }
    Ok(json!(values))
}

fn video_capabilities(
    env: &mut JNIEnv,
    caps: &JObject,
    target: Option<&VideoQuery>,
) -> jni::errors::Result<Value> {
    env.with_local_frame(32, |env| {
        let video = env.call_method(caps, "getVideoCapabilities", "()Landroid/media/MediaCodecInfo$VideoCapabilities;", &[])?.l()?;
        let mut value = json!({
            "width_range":range(env,&video,"getSupportedWidths",&[])?,
            "height_range":range(env,&video,"getSupportedHeights",&[])?,
            "frame_rate_range":range(env,&video,"getSupportedFrameRates",&[])?,
            "width_alignment":env.call_method(&video,"getWidthAlignment","()I",&[])?.i()?,
            "height_alignment":env.call_method(&video,"getHeightAlignment","()I",&[])?.i()?,
            "ranges_are_independent":true,
            "real_time_guaranteed":false
        });
        if let Some(target) = target {
            let dimensions = [
                JValue::Int(target.width.unwrap_or(0) as i32),
                JValue::Int(target.height.unwrap_or(0) as i32),
            ];
            let size = env.call_method(&video,"isSizeSupported","(II)Z",&dimensions)?.z()?;
            let size_rate = env.call_method(&video,"areSizeAndRateSupported","(IID)Z",&[
                dimensions[0],dimensions[1],JValue::Double(target.frame_rate.unwrap_or(0) as f64)])?.z()?;
            let rates = if size {range(env,&video,"getSupportedFrameRatesFor",&dimensions)?} else {Value::Null};
            value["query"] = json!({"size_supported":size,"size_and_rate_supported":size_rate,"frame_rates_for_size":rates});
        }
        Ok(value)
    })
}

fn query_with_env(env: &mut JNIEnv, target: Option<&VideoQuery>) -> Result<Value> {
    let result = (|| -> jni::errors::Result<Value> {
        let list = env.new_object("android/media/MediaCodecList", "(I)V", &[JValue::Int(1)])?;
        let infos = JObjectArray::from(
            env.call_method(
                &list,
                "getCodecInfos",
                "()[Landroid/media/MediaCodecInfo;",
                &[],
            )?
            .l()?,
        );
        let count = env.get_array_length(&infos)?;
        if count > 512 {
            return Err(jni::errors::Error::NullPtr(
                "codec inventory exceeds budget",
            ));
        }
        let mut decoders = Vec::new();
        let mut matches = Vec::new();
        for i in 0..count {
            let decoder=env.with_local_frame(96,|env|->jni::errors::Result<Option<Value>>{
                let info=env.get_object_array_element(&infos,i)?;
                if env.call_method(&info,"isEncoder","()Z",&[])?.z()? {return Ok(None);}
                let name=JString::from(env.call_method(&info,"getName","()Ljava/lang/String;",&[])?.l()?);
                let name:String=env.get_string(&name)?.into();
                let hardware=env.call_method(&info,"isHardwareAccelerated","()Z",&[])?.z()?;
                let software=env.call_method(&info,"isSoftwareOnly","()Z",&[])?.z()?;
                let types=JObjectArray::from(env.call_method(&info,"getSupportedTypes","()[Ljava/lang/String;",&[])?.l()?);
                let mut supported=Vec::new();
                for j in 0..env.get_array_length(&types)?.min(16) {
                    let mime=JString::from(env.get_object_array_element(&types,j)?);
                    let mime_text:String=env.get_string(&mime)?.into();
                    if !(mime_text.starts_with("audio/")||mime_text.starts_with("video/")){continue;}
                    let caps=env.call_method(&info,"getCapabilitiesForType","(Ljava/lang/String;)Landroid/media/MediaCodecInfo$CodecCapabilities;",&[JValue::Object(&mime)])?.l()?;
                    let target_for_type=target.as_ref().filter(|t| t.mime.as_deref()==Some(mime_text.as_str()));
                    let video=if mime_text.starts_with("video/") {video_capabilities(env,&caps,target_for_type)?}else{Value::Null};
                    if target_for_type.is_some() {
                        matches.push(json!({"name":name,"hardware_accelerated":hardware,"software_only":software,"capabilities":video["query"]}));
                    }
                    let profiles=JObjectArray::from(env.get_field(&caps,"profileLevels","[Landroid/media/MediaCodecInfo$CodecProfileLevel;")?.l()?);
                    let mut levels=Vec::new();
                    for k in 0..env.get_array_length(&profiles)?.min(64) {
                        let p=env.get_object_array_element(&profiles,k)?;
                        levels.push(json!({"profile":env.get_field(&p,"profile","I")?.i()?,"level":env.get_field(&p,"level","I")?.i()?}));
                        env.delete_local_ref(p)?;
                    }
                    let backend_enabled=aem_media::VIDEO_MIMES.contains(&mime_text.as_str())||aem_media::NATIVE_AUDIO_MIMES.contains(&mime_text.as_str());
                    supported.push(json!({"mime":mime_text,"backend_enabled":backend_enabled,"profile_levels":levels,"profiles_truncated":env.get_array_length(&profiles)?>64,"video_capabilities":video}));
                    env.delete_local_ref(profiles)?;env.delete_local_ref(caps)?;env.delete_local_ref(mime)?;
                }
                Ok(Some(json!({"name":name,"hardware_accelerated":hardware,"software_only":software,"types":supported})))
            })?;
            if let Some(d) = decoder {
                decoders.push(d);
            }
        }
        Ok(json!({"schema_version":1,"decoders":decoders,
            "video":{"containers":["MP4","MOV","3GP","Matroska","WebM"],"mime_types":aem_media::VIDEO_MIMES,
                "profiles":{"video/avc":["Baseline","Main","High"],"video/hevc":["Main"],"video/x-vnd.on2.vp8":["8-bit"],"video/x-vnd.on2.vp9":["0"]},
                "max_pixels":aem_core::MAX_VIDEO_PIXELS,"max_dimension":aem_core::MAX_VIDEO_DIMENSION,"max_fps":aem_core::MAX_VIDEO_FPS,"max_index_frames":aem_core::MAX_VIDEO_FRAMES,"hdr":false,"bit_depth":8,
                "preserves_source_timestamps":true,"preserves_source_aspect_ratio":true,"arbitrary_aspect_ratio":true,"square_pixels_only":true,"input_is_independent_of_composition":true,"max_stream_cache_bytes":aem_host::video_cache::MAX_CACHE_BYTES},
            "composition":{"fps_range":[1,aem_core::MAX_COMPOSITION_FPS],"fps_presets":[24,25,30,50,60,90,120,144,240],"fps_type":"integer","default_fps":30,"max_dimension":8192},
            "video_query":target.map(VideoQuery::to_json),
            "query_result":target.as_ref().map(|t|json!({"backend_eligible":t.backend_eligible(),"decoders":matches,"real_time_guaranteed":false})),
            "audio":{"portable_formats":["M4A/AAC-LC","M4A/ALAC","MP3","FLAC","Ogg/Vorbis","WAV/PCM8/16/24/32","WAV/float32/64","AIFF/PCM"],
                "platform_formats":["Opus","ADTS/AAC","HE-AAC","AMR-NB","AMR-WB"],"native_mime_types":aem_media::NATIVE_AUDIO_MIMES,
                "min_sample_rate":8000,"max_sample_rate":192000,"channels":[1,2],"output_rate":48000,"output_channels":2},
            "requires_probe":true,"probe_operation":"probe_media","decoder_presence_guarantees_file_support":false,
            "project_package":aem_host::ops::media::package_limits(),"max_source_bytes":aem_core::storage::MAX_MEDIA_ASSET,"max_duration_seconds":3600,"max_pcm_cache_bytes":aem_media::Limits::default().cache_bytes}))
    })();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result.map_err(|e| format!("media capability query failed: {e}"))
}

/// Attach the calling thread to the JVM and enumerate the decoder inventory.
///
/// The shared host calls this without a `JNIEnv`, so the thread is attached here
/// and released when the guard drops.
pub fn query(target: Option<&VideoQuery>) -> Result<Value> {
    if let Some(t) = &target {
        t.validate()?;
    }
    let _guard = crate::video_decode::attach().map_err(|e| e.to_string())?;
    let mut env = _guard.env();
    query_with_env(&mut env, target)
}
