//! `ANativeWindow` adapted to the raw-window-handle traits wgpu needs.
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, WindowHandle,
};

pub struct AndroidWindow(pub NativeWindow);

impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        // AndroidDisplayHandle carries no borrowed external display pointer.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}