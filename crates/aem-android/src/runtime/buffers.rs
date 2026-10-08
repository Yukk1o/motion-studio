//! Direct-buffer metadata and byte writes shared by the JNI adapter families.
use super::bridge::{JByteBuffer, JNIEnv};
use super::Result;

/// Capture JNI lookup results without changing an endpoint's validation order.
/// For example, GeometryBridge reports read-only buffers before lookup failures.
pub(super) struct BufferAccess {
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
}

/// Borrow the validated region of a Java-owned output buffer.
///
/// # Safety
/// The caller must keep its Java buffer alive, ensure the pointer is non-null,
/// writable and valid for `bytes` bytes, and prevent overlapping mutable views.
pub(super) unsafe fn output_bytes<'a>(address: *mut u8, bytes: usize) -> &'a mut [u8] {
    unsafe { std::slice::from_raw_parts_mut(address, bytes) }
}

/// # Safety
/// `address` must denote a writable region of at least `bytes.len()` bytes which
/// does not overlap the input slice, and its Java owner must remain alive.
pub(super) unsafe fn copy_bytes(address: *mut u8, bytes: &[u8]) {
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), address, bytes.len()) }
}
