//! JNI value conversion and the stable ok/data/error envelope.
use aem_host::ops::dispatch;
use aem_host::Result;
use jni::{
    objects::{JByteBuffer, JString},
    sys::{jbyteArray, jint, jstring},
    JNIEnv,
};
use serde_json::Value;

/// Run a host operation and return the shared JSON envelope as a Java string.
pub fn string_result(env: &mut JNIEnv<'_>, operation: impl FnOnce() -> Result<Value>) -> jstring {
    let value = dispatch(operation);
    env.new_string(value.to_string())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

pub fn read_string(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Result<String> {
    env.get_string(text)
        .map(|s| s.into())
        .map_err(|e| e.to_string())
}

/// Legacy packed-buffer endpoints report every native failure as -1.
pub fn integer_result(operation: impl FnOnce() -> Result<i32>) -> jint {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .ok()
        .and_then(std::result::Result::ok)
        .unwrap_or(-1)
}

/// Pixel endpoints use a null Java array for both failures and panics.
pub fn bytes_result(env: &JNIEnv<'_>, operation: impl FnOnce() -> Result<Vec<u8>>) -> jbyteArray {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(bytes)) => env
            .byte_array_from_slice(&bytes)
            .map_or(std::ptr::null_mut(), |array| array.into_raw()),
        _ => std::ptr::null_mut(),
    }
}

/// Direct-buffer metadata captured without changing an endpoint's validation
/// order. GeometryBridge historically reports read-only buffers before lookup
/// failures, so both results are captured up front.
pub struct BufferAccess {
    pub address: jni::errors::Result<*mut u8>,
    pub capacity: jni::errors::Result<usize>,
}

impl BufferAccess {
    pub fn capture(env: &JNIEnv<'_>, buffer: &JByteBuffer<'_>) -> Self {
        let address = env.get_direct_buffer_address(buffer);
        let capacity = env.get_direct_buffer_capacity(buffer);
        Self { address, capacity }
    }

    /// Render-plan/bundle adapters historically validate capacity first.
    pub fn capacity_first(env: &JNIEnv<'_>, buffer: &JByteBuffer<'_>) -> Result<(*mut u8, usize)> {
        let capacity = env
            .get_direct_buffer_capacity(buffer)
            .map_err(|e| e.to_string())?;
        let address = env
            .get_direct_buffer_address(buffer)
            .map_err(|e| e.to_string())?;
        Ok((address, capacity))
    }

    pub fn address_first(env: &JNIEnv<'_>, buffer: &JByteBuffer<'_>) -> Result<(*mut u8, usize)> {
        let address = env
            .get_direct_buffer_address(buffer)
            .map_err(|e| e.to_string())?;
        let capacity = env
            .get_direct_buffer_capacity(buffer)
            .map_err(|e| e.to_string())?;
        Ok((address, capacity))
    }

    pub fn resolve(&self) -> Result<(*mut u8, usize)> {
        let address = *self.address.as_ref().map_err(|e| e.to_string())?;
        let capacity = *self.capacity.as_ref().map_err(|e| e.to_string())?;
        Ok((address, capacity))
    }
}

pub fn is_read_only(env: &mut JNIEnv<'_>, buffer: &JByteBuffer<'_>) -> Result<bool> {
    env.call_method(buffer, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z())
        .map_err(|e| e.to_string())
}

/// Borrow the validated region of a Java-owned output buffer.
///
/// # Safety
/// The caller must keep its Java buffer alive, ensure the pointer is non-null,
/// writable and valid for `bytes` bytes, and prevent overlapping mutable views.
pub unsafe fn output_bytes<'a>(address: *mut u8, bytes: usize) -> &'a mut [u8] {
    unsafe { std::slice::from_raw_parts_mut(address, bytes) }
}

/// # Safety
/// `address` must denote a writable region of at least `bytes.len()` bytes which
/// does not overlap the input slice, and its Java owner must remain alive.
pub unsafe fn copy_bytes(address: *mut u8, bytes: &[u8]) {
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), address, bytes.len()) }
}
