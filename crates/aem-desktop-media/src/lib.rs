//! Desktop implementation of [`aem_host::Platform`].
//!
//! Surface creation is plain wgpu/winit, so it is always available. Video
//! probing, decoding and encoding require libav and live behind the `ffmpeg`
//! feature; without it the host still edits, previews and exports audio, and
//! reports the missing capability instead of failing silently.
use aem_core::{AudioAsset, VideoAsset};
use aem_host::platform::{Platform, SurfaceTarget, VideoDecoder, VideoQuery};
use aem_host::Result;
use aem_media::VideoProbe;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

#[cfg(feature = "ffmpeg")]
pub mod av;

/// Presentation backends tried in order.
///
/// Vulkan first because it gives the compositor the widest effect support and
/// the best multi-queue behaviour; GL is the fallback that always works on
/// older drivers. Only one backend's surface may stay alive for a window.
pub const SURFACE_BACKENDS: [wgpu::Backends; 4] = [
    wgpu::Backends::VULKAN,
    wgpu::Backends::METAL,
    wgpu::Backends::DX12,
    wgpu::Backends::GL,
];

pub struct DesktopPlatform {
    surface_backends: Vec<wgpu::Backends>,
}

impl Default for DesktopPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopPlatform {
    pub fn new() -> Self {
        Self {
            surface_backends: SURFACE_BACKENDS.to_vec(),
        }
    }

    /// Restrict the presentation backends, for troubleshooting a driver.
    pub fn with_backends(backends: Vec<wgpu::Backends>) -> Self {
        Self {
            surface_backends: backends,
        }
    }

    fn video_available() -> bool {
        cfg!(feature = "ffmpeg")
    }
}

impl Platform for DesktopPlatform {
    fn name(&self) -> &'static str {
        if Self::video_available() {
            "libav"
        } else {
            "no video backend"
        }
    }

    #[cfg(feature = "ffmpeg")]
    fn probe_video(
        &self,
        path: &Path,
        selected: Option<u32>,
        audio_selected: Option<u32>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<VideoProbe> {
        av::probe(path, selected, audio_selected, check)
    }

    #[cfg(not(feature = "ffmpeg"))]
    fn probe_video(
        &self,
        _path: &Path,
        _selected: Option<u32>,
        _audio_selected: Option<u32>,
        _check: &dyn Fn() -> Result<()>,
    ) -> Result<VideoProbe> {
        Err("this build has no video backend; rebuild with --ffmpeg".into())
    }

    #[cfg(feature = "ffmpeg")]
    fn open_decoder(
        &self,
        path: &Path,
        asset: VideoAsset,
        pts: Vec<u64>,
    ) -> Result<Box<dyn VideoDecoder>> {
        Ok(Box::new(av::Decoder::open(path, asset, pts)?))
    }

    #[cfg(not(feature = "ffmpeg"))]
    fn open_decoder(
        &self,
        _path: &Path,
        _asset: VideoAsset,
        _pts: Vec<u64>,
    ) -> Result<Box<dyn VideoDecoder>> {
        Err("this build has no video backend; rebuild with --ffmpeg".into())
    }

    /// Symphonia already covers every audio container the Android host imports
    /// portably, so desktop has no platform audio fallback. This keeps Opus and
    /// HE-AAC on the portable path rather than widening a damaged source into a
    /// permissive partial decode.
    fn decode_audio(
        &self,
        path: &Path,
        pcm: &Path,
        selected_track: Option<u32>,
        max_pcm_bytes: u64,
        check: &mut dyn FnMut(f64) -> Result<()>,
    ) -> Result<AudioAsset> {
        aem_media::decode_audio(path, pcm, selected_track, max_pcm_bytes, check)
    }

    fn media_capabilities(&self, query: Option<&VideoQuery>) -> Result<Value> {
        Ok(capabilities(Self::video_available(), query))
    }

    fn attach_surface(
        &self,
        window: Arc<dyn wgpu::WindowHandle + Send + Sync>,
        width: u32,
        height: u32,
    ) -> Result<SurfaceTarget> {
        let mut failures = Vec::new();
        for backend in &self.surface_backends {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: *backend,
                ..Default::default()
            });
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(error) => {
                    failures.push(format!("{backend:?}: {error}"));
                    continue;
                }
            };
            match pollster::block_on(aem_render::Renderer::new_profiled(
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
                    // Immediate presents keep input latency low while still being
                    // paced by the compositor, matching the Android Fifo path.
                    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                        wgpu::PresentMode::Mailbox
                    } else {
                        wgpu::PresentMode::Fifo
                    };
                    let config = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format,
                        width,
                        height,
                        present_mode,
                        desired_maximum_frame_latency: 2,
                        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                        view_formats: vec![],
                    };
                    return Ok(SurfaceTarget {
                        instance,
                        surface,
                        config,
                        renderer,
                    });
                }
                Err(error) => failures.push(format!("{backend:?}: {error}")),
            }
        }
        Err(format!("no usable GPU backend: {}", failures.join("; ")))
    }
}

/// Device capability report in the shared `media_capabilities` shape.
///
/// The desktop inventory is fixed rather than probed: unlike Android there is no
/// per-device codec list to enumerate, so the report states what the build
/// supports and keeps `requires_probe` set so the UI still probes real files.
pub fn capabilities(has_video: bool, query: Option<&VideoQuery>) -> Value {
    let query = query.and_then(|q| q.mime.as_ref()).map(|mime| {
        json!({"mime":mime,"width":query.and_then(|q| q.width),"height":query.and_then(|q| q.height),
            "frame_rate":query.and_then(|q| q.frame_rate)})
    });
    json!({
        "schema_version": 1,
        "backend_enabled": has_video,
        "backend": if has_video { "libav" } else { "none" },
        "decoders": if has_video { video_decoders() } else { Value::Array(Vec::new()) },
        "encoders": if has_video { video_encoders() } else { Value::Array(Vec::new()) },
        "video": {
            "containers": ["MP4","MOV","3GP","Matroska","WebM"],
            "mime_types": aem_media::VIDEO_MIMES,
            "profiles": {
                "video/avc": ["Baseline","Main","High"],
                "video/hevc": ["Main"],
                "video/x-vnd.on2.vp8": ["8-bit"],
                "video/x-vnd.on2.vp9": ["0"],
            },
            "max_pixels": aem_core::MAX_VIDEO_PIXELS,
            "max_dimension": aem_core::MAX_VIDEO_DIMENSION,
            "max_fps": aem_core::MAX_VIDEO_FPS,
            "max_index_frames": aem_core::MAX_VIDEO_FRAMES,
            "hdr": false,
            "bit_depth": 8,
            "preserves_source_timestamps": true,
            "preserves_source_aspect_ratio": true,
            "arbitrary_aspect_ratio": true,
            "square_pixels_only": true,
            "input_is_independent_of_composition": true,
            "max_stream_cache_bytes": aem_host::video_cache::MAX_CACHE_BYTES,
            "output": "H.264 + AAC in MP4",
        },
        "audio": {
            "portable_formats": ["M4A/AAC-LC","M4A/ALAC","MP3","FLAC","Ogg/Vorbis/Opus","WAV/PCM8/16/24/32","WAV/float32/64","AIFF/PCM"],
            "platform_formats": Value::Array(Vec::new()),
            "native_mime_types": Value::Array(Vec::new()),
            "min_sample_rate": 8000,
            "max_sample_rate": 192000,
            "channels": [1, 2],
            "output_rate": 48000,
            "output_channels": 2,
        },
        "requires_probe": true,
        "probe_operation": "probe_media",
        "decoder_presence_guarantees_file_support": false,
        "max_source_bytes": aem_core::storage::MAX_MEDIA_ASSET,
        "max_duration_seconds": 3600,
        "max_pcm_cache_bytes": aem_media::Limits::default().cache_bytes,
        "video_query": query,
    })
}

fn video_decoders() -> Value {
    json!([
        "video/avc",
        "video/hevc",
        "video/x-vnd.on2.vp8",
        "video/x-vnd.on2.vp9"
    ])
}

fn video_encoders() -> Value {
    json!(["video/avc"])
}

/// Shared handle used by the desktop shell.
pub fn platform() -> Arc<DesktopPlatform> {
    Arc::new(DesktopPlatform::new())
}
