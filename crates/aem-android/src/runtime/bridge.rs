//! JNI value conversion and the stable ok/data/error envelope.
use super::{Result, Value};
pub(super) use jni::{
    objects::{JByteBuffer, JClass, JObject, JString},
    sys::{jboolean, jbyteArray, jdouble, jint, jlong, jstring},
    JNIEnv,
};
use serde_json::json;

pub(super) fn string_result(
    env: &mut JNIEnv<'_>,
    operation: impl FnOnce() -> Result<Value>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    let value = match result {
        Ok(Ok(value)) => json!({"ok":true,"data":value}),
        Ok(Err(error)) => {
            json!({"ok":false,"error":error,"error_detail":error.strip_prefix("composition_error:").and_then(|v|serde_json::from_str::<Value>(v).ok())})
        }
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
pub(super) fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let a = c[0] as usize;
        let b = c.get(1).copied().unwrap_or(0) as usize;
        let d = c.get(2).copied().unwrap_or(0) as usize;
        out.push(TABLE[a >> 2] as char);
        out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if c.len() > 1 {
            TABLE[((b & 15) << 2) | (d >> 6)] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            TABLE[d & 63] as char
        } else {
            '='
        });
    }
    out
}
pub(super) fn read_string(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Result<String> {
    env.get_string(text)
        .map(|s| s.into())
        .map_err(|e| e.to_string())
}

/// Legacy packed-buffer endpoints report every native failure as -1.
pub(super) fn integer_result(operation: impl FnOnce() -> Result<i32>) -> jint {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .ok()
        .and_then(std::result::Result::ok)
        .unwrap_or(-1)
}

/// Pixel endpoints use a null Java array for both failures and panics.
pub(super) fn bytes_result(
    env: &JNIEnv<'_>,
    operation: impl FnOnce() -> Result<Vec<u8>>,
) -> jbyteArray {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(bytes)) => env
            .byte_array_from_slice(&bytes)
            .map_or(std::ptr::null_mut(), |array| array.into_raw()),
        _ => std::ptr::null_mut(),
    }
}
