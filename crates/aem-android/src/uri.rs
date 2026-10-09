//! Resolve `content://` URIs from the system picker into readable files.
//!
//! Desktop hosts pass real paths, so this module is the only place the shared
//! import pipeline learns about Android's storage model.
use aem_host::ops::media::Opener;
use jni::objects::{JObject, JValue};
use jni::JNIEnv;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::{FromRawFd, RawFd},
};

pub const MAX_URI_LENGTH: usize = 8192;

/// Render a pending Java exception as a message and clear it.
pub fn java_error(env: &mut JNIEnv, error: jni::errors::Error) -> String {
    if env.exception_check().unwrap_or(false) {
        let exception = env.exception_occurred();
        let _ = env.exception_clear();
        if let Ok(exception) = exception {
            if let Ok(text) =
                env.call_method(&exception, "toString", "()Ljava/lang/String;", &[])
            {
                if let Ok(text) = text.l() {
                    if let Ok(s) = env.get_string(&jni::objects::JString::from(text)) {
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
) -> aem_host::Result<(Box<dyn Read + Send>, Option<u64>)> {
    if uri.len() > MAX_URI_LENGTH || !(uri.starts_with("content://") || uri.starts_with("file://"))
    {
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

/// Build a deferred opener so provider I/O happens on the import worker thread.
pub fn uri_opener(
    env: &mut JNIEnv,
    context: &JObject,
    uri: String,
) -> aem_host::Result<Opener> {
    let vm = env.get_java_vm().map_err(|e| e.to_string())?;
    let context = env
        .new_global_ref(context)
        .map_err(|e| java_error(env, e))?;
    Ok(Box::new(move || {
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let result = uri_reader(&mut env, context.as_obj(), &uri);
        drop(context); // Release the global reference while the thread is attached.
        result
    }))
}