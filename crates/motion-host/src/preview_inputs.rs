//! Independent original-source inputs for effect/node authoring.
//! The caller owns this reader; it never seeks, edits or renders a Session.
use crate::{video_frame::DecodedFrame, video_frames::VideoFrames, Platform, Result};
use motion_core::{Content, Project};
use motion_render::image_resources::{DecodeTask, Pixels, Resolution, Source, MAX_PREVIEW_EDGE};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputSource {
    ImageAsset { asset: u64 },
    Layer { composition: String, object: u64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRequest {
    pub sequence: u64,
    pub source: InputSource,
    pub frame: f64,
    pub max_edge: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct InputClock {
    pub composition_frame: f64,
    pub local_frame: f64,
    pub seconds: f64,
    pub fps: u32,
    pub source_time_us: Option<i64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct InputInfo {
    pub sequence: u64,
    pub revision: u64,
    pub source: InputSource,
    pub logical_size: [f32; 2],
    pub source_size: [u32; 2],
    pub raster_size: [u32; 2],
    pub clock: InputClock,
    pub stage: &'static str,
    pub active: bool,
    pub working_space: motion_effects::WorkingSpace,
    pub alpha_mode: motion_effects::AlphaMode,
    pub video_pts_us: Option<u64>,
    pub video_end_us: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct InputChoice {
    pub source: InputSource,
    pub name: String,
    pub media: &'static str,
}
/// Supported registered images, text rasters and video layers. Audio, geometry
/// and composition outputs need different source contracts and are omitted.
pub fn input_choices(project: &Project) -> Vec<InputChoice> {
    let mut choices: Vec<_> = project
        .assets
        .iter()
        .map(|asset| InputChoice {
            source: InputSource::ImageAsset { asset: asset.id },
            name: asset.path.clone(),
            media: "image",
        })
        .collect();
    for composition in std::iter::once(project.composition_id.as_str()).chain(
        project
            .compositions
            .iter()
            .map(|composition| composition.id.as_str()),
    ) {
        if let Ok(view) = project.composition_view(composition) {
            for layer in view.layers {
                let media = match &layer.content {
                    Content::Image { .. } => "image",
                    Content::Text { .. } => "text_raster",
                    Content::Video { .. } => "video",
                    _ => continue,
                };
                choices.push(InputChoice {
                    source: InputSource::Layer {
                        composition: composition.into(),
                        object: layer.id,
                    },
                    name: layer.name.clone(),
                    media,
                });
            }
        }
    }
    choices
}
pub enum InputPixels {
    Image(Arc<Pixels>),
    Video(Arc<DecodedFrame>),
    Transparent,
}
pub struct PreparedInput {
    pub info: InputInfo,
    pub pixels: InputPixels,
}
impl PreparedInput {
    /// The destination owns separate textures on the caller's GPU. Image bytes
    /// are already linear-premultiplied; codec YUV stays on the GPU conversion path.
    pub fn upload(
        &self,
        target: &mut motion_render::preview_input::PreviewInputTexture,
    ) -> Result<()> {
        let edge = self.info.raster_size[0].max(self.info.raster_size[1]);
        match &self.pixels {
            InputPixels::Image(pixels) => {
                target.upload_rgba(pixels.width, pixels.height, &pixels.rgba, true, edge)
            }
            InputPixels::Video(frame) => match &frame.pixels {
                crate::video_frame::VideoPixels::Yuv(pixels) => target.upload_yuv(pixels, edge),
                crate::video_frame::VideoPixels::Rgba(pixels) => {
                    target.upload_rgba(frame.width, frame.height, pixels, false, edge)
                }
            },
            InputPixels::Transparent => target.transparent(),
        }
    }
}
pub enum InputPoll {
    Pending(InputInfo),
    Ready(PreparedInput),
}
#[derive(Clone, PartialEq, Eq)]
struct ImageKey {
    source: Source,
    resolution: Resolution,
    len: u64,
    modified: Option<SystemTime>,
}
impl ImageKey {
    fn new(source: Source, resolution: Resolution) -> Result<Self> {
        let metadata = source.path.metadata().map_err(|error| error.to_string())?;
        Ok(Self {
            source,
            resolution,
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
    fn current(&self) -> bool {
        Self::new(self.source.clone(), self.resolution).is_ok_and(|current| current == *self)
    }
}
enum DesiredSource {
    Image(ImageKey),
    Video(Box<Project>, PathBuf, u64, ImageKey),
    Transparent,
}
struct Desired {
    request: InputRequest,
    info: InputInfo,
    source: DesiredSource,
    root: PathBuf,
    error: Option<String>,
}
pub struct PreviewInputReader {
    video: VideoFrames,
    image: DecodeTask,
    decoding: Option<ImageKey>,
    cached: Option<(ImageKey, Arc<Pixels>)>,
    desired: Option<Desired>,
    last_sequence: u64,
}
impl PreviewInputReader {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self {
            video: VideoFrames::new(platform),
            image: DecodeTask::default(),
            decoding: None,
            cached: None,
            desired: None,
            last_sequence: 0,
        }
    }
    /// Sequence is strictly increasing for new requests. An identical current
    /// request can be repeated. Revision binds results to the caller's snapshot.
    pub fn request(
        &mut self,
        project: &Project,
        root: &Path,
        revision: u64,
        request: InputRequest,
    ) -> Result<InputInfo> {
        if request.sequence == 0
            || request.sequence > i64::MAX as u64
            || !request.frame.is_finite()
            || request.frame < 0.
            || request.frame > f64::from(motion_core::MAX_FRAMES)
            || !(1..=MAX_PREVIEW_EDGE).contains(&request.max_edge)
        {
            return Err("invalid preview input sequence, time or raster budget".into());
        }
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if let Some(current) = &self.desired {
            if request.sequence == current.request.sequence
                && request == current.request
                && revision == current.info.revision
                && root == current.root
            {
                return Ok(current.info.clone());
            }
        }
        if request.sequence <= self.last_sequence {
            return Err("preview input request superseded or sequence reused".into());
        }
        let mut snapshot = project.clone();
        if let InputSource::Layer { composition, .. } = &request.source {
            snapshot
                .activate_composition(composition)
                .map_err(|error| error.to_string())?;
        }
        if snapshot.fps == 0
            || snapshot.fps > motion_core::MAX_COMPOSITION_FPS
            || snapshot.frames == 0
        {
            return Err("invalid preview composition clock".into());
        }
        let mut clock = InputClock {
            composition_frame: request.frame,
            local_frame: request.frame,
            seconds: request.frame / f64::from(snapshot.fps),
            fps: snapshot.fps,
            source_time_us: None,
        };
        let mut logical = None;
        let mut outside = false;
        let image_asset = match &request.source {
            InputSource::ImageAsset { asset } => Some(*asset),
            InputSource::Layer { object, .. } => {
                let layer = snapshot
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == *object)
                    .ok_or("preview input layer missing")?;
                logical = Some(layer.size);
                clock.local_frame = layer.local_frame(request.frame);
                clock.seconds = clock.local_frame / f64::from(snapshot.fps);
                let clip = layer.clip(snapshot.frames);
                outside = request.frame < f64::from(clip.in_frame)
                    || request.frame >= f64::from(clip.out_frame);
                // Original sources remain selectable when hidden in the editor.
                layer.visible = true;
                match &layer.content {
                    Content::Image { asset } => Some(*asset),
                    Content::Text { raster_asset, .. } => Some(*raster_asset),
                    Content::Video { video } => {
                        clock.source_time_us =
                            Some(video.source_time_us(clock.local_frame, snapshot.fps));
                        None
                    }
                    _ => return Err("preview input layer has no supported original pixels".into()),
                }
            }
        };
        let (size, source) = if let Some(asset_id) = image_asset {
            let asset = snapshot
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .ok_or("preview image asset missing")?;
            let size = [asset.width, asset.height];
            let source = if outside {
                DesiredSource::Transparent
            } else {
                DesiredSource::Image(ImageKey::new(
                    Source::new(&root, asset)?,
                    Resolution::Preview(request.max_edge),
                )?)
            };
            (size, source)
        } else {
            let InputSource::Layer { object, .. } = &request.source else {
                unreachable!()
            };
            let layer = snapshot
                .layers
                .iter()
                .find(|layer| layer.id == *object)
                .unwrap();
            let Content::Video { video } = &layer.content else {
                unreachable!()
            };
            let asset = snapshot
                .video_assets
                .iter()
                .find(|asset| asset.id == video.asset)
                .ok_or("preview video asset missing")?;
            let size = [asset.display_width, asset.display_height];
            let time = clock.source_time_us.unwrap();
            outside |= time < asset.video_start_us as i64 || time >= asset.video_end_us as i64;
            let source = if outside {
                DesiredSource::Transparent
            } else {
                let signature = ImageKey::new(
                    Source::new(
                        &root,
                        &motion_core::Asset {
                            id: asset.id,
                            path: asset.path.clone(),
                            width: size[0],
                            height: size[1],
                        },
                    )?,
                    Resolution::Full,
                )?;
                if signature.len != asset.bytes {
                    return Err("owned preview video source changed".into());
                }
                DesiredSource::Video(Box::new(snapshot), root.clone(), *object, signature)
            };
            (size, source)
        };
        if size.contains(&0) {
            return Err("preview input has empty dimensions".into());
        }
        let (width, height) = Resolution::Preview(request.max_edge).dimensions(size[0], size[1]);
        let info = InputInfo {
            sequence: request.sequence,
            revision,
            source: request.source.clone(),
            logical_size: logical.unwrap_or([size[0] as f32, size[1] as f32]),
            source_size: size,
            raster_size: if outside { [1, 1] } else { [width, height] },
            clock,
            stage: "original",
            active: !outside,
            working_space: motion_effects::WorkingSpace::Linear,
            alpha_mode: motion_effects::AlphaMode::Premultiplied,
            video_pts_us: None,
            video_end_us: None,
        };
        if info
            .logical_size
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.)
        {
            return Err("invalid preview logical size".into());
        }
        let same_video = matches!((&source,self.desired.as_ref().map(|desired|(&desired.source,&desired.root))),
            (DesiredSource::Video(_,_,object,key),Some((DesiredSource::Video(_,_,old,old_key),old_root))) if object==old && old_root==&root && key==old_key);
        if !same_video {
            self.video.clear();
        }
        if let DesiredSource::Image(key) = &source {
            if self
                .decoding
                .as_ref()
                .is_some_and(|decoding| decoding != key)
            {
                self.image.cancel();
            }
            self.video.clear();
        } else {
            self.image.cancel();
            self.cached = None;
        }
        if !matches!(&source, DesiredSource::Video(..)) {
            self.video.clear();
        }
        self.last_sequence = request.sequence;
        self.desired = Some(Desired {
            request,
            info: info.clone(),
            source,
            root,
            error: None,
        });
        if let DesiredSource::Video(project, root, object, _) =
            &self.desired.as_ref().unwrap().source
        {
            self.video.request(
                project,
                root,
                *object,
                self.desired.as_ref().unwrap().request.frame,
                self.last_sequence,
            )?;
        }
        self.start_image()?;
        Ok(info)
    }
    fn start_image(&mut self) -> Result<()> {
        let Some(Desired {
            source: DesiredSource::Image(key),
            ..
        }) = &self.desired
        else {
            return Ok(());
        };
        if !self.image.busy() && self.cached.as_ref().is_none_or(|(cached, _)| cached != key) {
            self.image
                .start(key.source.clone(), key.resolution, false)?;
            self.decoding = Some(key.clone());
        }
        Ok(())
    }
    pub fn poll(&mut self, sequence: u64, revision: u64) -> Result<InputPoll> {
        let current = self
            .desired
            .as_ref()
            .ok_or("preview input request missing")?;
        if current.request.sequence != sequence || current.info.revision != revision {
            return Err("preview input snapshot superseded".into());
        }
        if let Some(error) = &current.error {
            return Err(error.clone());
        }
        if let Some((_, _, _, result)) = self.image.poll() {
            if let Some(key) = self.decoding.take() {
                if matches!(&self.desired.as_ref().unwrap().source,DesiredSource::Image(desired) if desired==&key)
                {
                    let pixels = match result {
                        Ok(pixels) => pixels,
                        Err(error) => {
                            self.desired.as_mut().unwrap().error = Some(error.clone());
                            return Err(error);
                        }
                    };
                    if !key.current() {
                        return Err("preview image changed while decoding".into());
                    }
                    self.cached = Some((key, Arc::new(pixels)));
                }
            }
        }
        self.start_image()?;
        let current = self.desired.as_ref().unwrap();
        let mut info = current.info.clone();
        let pixels = match &current.source {
            DesiredSource::Transparent => InputPixels::Transparent,
            DesiredSource::Image(key) => {
                if !key.current() {
                    return Err("preview image source changed; request a new snapshot".into());
                }
                let Some((cached, pixels)) = &self.cached else {
                    return Ok(InputPoll::Pending(info));
                };
                if cached != key {
                    return Ok(InputPoll::Pending(info));
                }
                InputPixels::Image(pixels.clone())
            }
            DesiredSource::Video(project, root, object, signature) => {
                if !signature.current() {
                    return Err("preview video source changed; request a new snapshot".into());
                }
                let status =
                    self.video
                        .request(project, root, *object, current.request.frame, sequence)?;
                match status["state"].as_str() {
                    Some("ready") => {
                        let frame = self.video.frame(project, *object, sequence)?;
                        info.video_pts_us = Some(frame.pts);
                        info.video_end_us = Some(frame.end);
                        InputPixels::Video(frame)
                    }
                    Some("outside") => InputPixels::Transparent,
                    Some("pending") => return Ok(InputPoll::Pending(info)),
                    _ => {
                        return Err(status["error"]
                            .as_str()
                            .unwrap_or("preview video decode failed")
                            .into())
                    }
                }
            }
        };
        Ok(InputPoll::Ready(PreparedInput { info, pixels }))
    }
    /// Cancellation invalidates every outstanding packet. Sequence IDs remain
    /// monotonic across cancellation, including reuse after switching projects.
    pub fn cancel(&mut self) {
        self.desired = None;
        self.image.cancel();
        self.cached = None;
        self.video.clear();
    }
}
impl Drop for PreviewInputReader {
    fn drop(&mut self) {
        self.cancel();
    }
}
