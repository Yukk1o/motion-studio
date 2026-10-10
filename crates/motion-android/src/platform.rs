//! Android implementation of the shared host platform traits.
//!
//! This is the whole Android-specific runtime surface: MediaCodec video decode,
//! MediaCodec audio fallback, the MediaCodecList inventory and Surface
//! presentation. Everything else lives in `motion-host`.
use crate::android_window::AndroidWindow;
use crate::audio_decode;
use crate::media_capabilities;
use crate::video_decode;
use motion_core::{AudioAsset, VideoAsset};
use motion_host::platform::{Platform, SurfaceTarget, VideoDecoder, VideoQuery};
use motion_host::Result;
use motion_media::VideoProbe;
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// Shared handle stored in a session-static for JNI entry points that do not
/// receive a `Context` (capability queries, surface binding, render ticks).
static PLATFORM: std::sync::OnceLock<Arc<AndroidPlatform>> = std::sync::OnceLock::new();

/// Install the process-wide Android platform. Returns the shared handle.
pub fn install() -> Arc<AndroidPlatform> {
    PLATFORM.get_or_init(|| Arc::new(AndroidPlatform)).clone()
}

/// The installed Android platform, if the runtime has been started.
pub fn current() -> Option<Arc<AndroidPlatform>> {
    PLATFORM.get().cloned()
}

pub struct AndroidPlatform;

impl AndroidPlatform {
    /// Present the composition on an `ANativeWindow` from a `SurfaceView`.
    pub fn attach_native_window(
        &self,
        window: ndk::native_window::NativeWindow,
        width: u32,
        height: u32,
    ) -> Result<SurfaceTarget> {
        let handle: Arc<dyn wgpu::WindowHandle + Send + Sync> =
            Arc::new(AndroidWindow(window.clone()));
        self.attach_surface(handle, width, height)
    }
}

impl Platform for AndroidPlatform {
    fn name(&self) -> &'static str {
        "Android MediaCodec"
    }

    fn probe_video(
        &self,
        path: &Path,
        selected: Option<u32>,
        audio_selected: Option<u32>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<VideoProbe> {
        video_decode::probe(path, selected, audio_selected, check)
    }

    fn open_decoder(
        &self,
        path: &Path,
        asset: VideoAsset,
        pts: Vec<u64>,
    ) -> Result<Box<dyn VideoDecoder>> {
        Ok(Box::new(video_decode::Decoder::new(path, asset, pts)?))
    }

    fn decode_audio(
        &self,
        path: &Path,
        pcm: &Path,
        selected_track: Option<u32>,
        max_pcm_bytes: u64,
        check: &mut dyn FnMut(f64) -> Result<()>,
    ) -> Result<AudioAsset> {
        audio_decode::decode(path, pcm, selected_track, max_pcm_bytes, check)
    }

    fn media_capabilities(&self, query: Option<&VideoQuery>) -> Result<Value> {
        media_capabilities::query(query)
    }

    fn attach_surface(
        &self,
        window: Arc<dyn wgpu::WindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<SurfaceTarget> {
        // Creating a Vulkan surface can connect the Android buffer producer even
        // when no Vulkan adapter exists, so only one backend's surface may stay
        // alive or the GLES fallback cannot connect to the same window.
        let mut failures = Vec::new();
        for backend in [wgpu::Backends::VULKAN, wgpu::Backends::GL] {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: backend,
                ..Default::default()
            });
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(error) => {
                    failures.push(error.to_string());
                    continue;
                }
            };
            match pollster::block_on(motion_render::Renderer::new_profiled(
                &instance,
                Some(&surface),
                wgpu::TextureFormat::Rgba8UnormSrgb,
                true,
            )) {
                Ok(renderer) => {
                    let caps = surface.get_capabilities(&renderer.adapter);
                    let format = caps
                        .formats
                        .iter()
                        .copied()
                        .find(|f| f.is_srgb())
                        .or_else(|| caps.formats.first().copied())
                        .ok_or("no supported surface pixel format")?;
                    let config = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format,
                        width,
                        height,
                        present_mode: wgpu::PresentMode::Fifo,
                        desired_maximum_frame_latency: 2,
                        alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque)
                        {
                            wgpu::CompositeAlphaMode::Opaque
                        } else {
                            *caps.alpha_modes.first().ok_or("no surface alpha mode")?
                        },
                        view_formats: vec![],
                    };
                    return Ok(SurfaceTarget {
                        instance,
                        surface,
                        config,
                        renderer,
                    });
                }
                Err(error) => failures.push(error.to_string()),
            }
        }
        Err(format!(
            "no Android surface backend: {}",
            failures.join("; ")
        ))
    }
}
