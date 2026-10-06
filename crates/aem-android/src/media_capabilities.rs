//! Device decoder inventory is a capability hint, never a substitute for probing.
use aem_media::Result;
use jni::{
    objects::{JObjectArray, JString, JValue},
    JNIEnv,
};
use serde_json::{json, Value};

pub fn query(env: &mut JNIEnv) -> Result<Value> {
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
                    let profiles=JObjectArray::from(env.get_field(&caps,"profileLevels","[Landroid/media/MediaCodecInfo$CodecProfileLevel;")?.l()?);
                    let mut levels=Vec::new();
                    for k in 0..env.get_array_length(&profiles)?.min(64) {
                        let p=env.get_object_array_element(&profiles,k)?;
                        levels.push(json!({"profile":env.get_field(&p,"profile","I")?.i()?,"level":env.get_field(&p,"level","I")?.i()?}));
                        env.delete_local_ref(p)?;
                    }
                    let backend_enabled=aem_media::VIDEO_MIMES.contains(&mime_text.as_str())||aem_media::NATIVE_AUDIO_MIMES.contains(&mime_text.as_str());
                    supported.push(json!({"mime":mime_text,"backend_enabled":backend_enabled,"profile_levels":levels,"profiles_truncated":env.get_array_length(&profiles)?>64}));
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
                "max_pixels":2073600,"max_dimension":1920,"max_fps":120,"hdr":false,"bit_depth":8},
            "audio":{"portable_formats":["M4A/AAC-LC","M4A/ALAC","MP3","FLAC","Ogg/Vorbis","WAV/PCM8/16/24/32","WAV/float32/64","AIFF/PCM"],
                "platform_formats":["Opus","ADTS/AAC","HE-AAC","AMR-NB","AMR-WB"],"native_mime_types":aem_media::NATIVE_AUDIO_MIMES,
                "min_sample_rate":8000,"max_sample_rate":192000,"channels":[1,2],"output_rate":48000,"output_channels":2},
            "requires_probe":true,"probe_operation":"probe_media","decoder_presence_guarantees_file_support":false,
            "max_source_bytes":aem_core::storage::MAX_MEDIA_ASSET,"max_duration_seconds":3600,"max_pcm_cache_bytes":aem_media::Limits::default().cache_bytes}))
    })();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result.map_err(|e| format!("media capability query failed: {e}"))
}
