use crate::{ensure, Camera, Result, Track};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_LAYERS: usize = 128;
pub const MAX_FRAMES: u32 = 36_000;
pub const MAX_COMPOSITION_FPS: u32 = 240;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: u64,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Adjustment,
    Vector {
        vector: crate::vector::VectorContent,
    },
    Composition { clip: crate::CompositionClip },
    Video {
        video: crate::VideoClip,
    },
    Audio {
        audio: crate::AudioClip,
    },
    Null,
    Solid {
        color: [f32; 4],
    },
    Image {
        asset: u64,
    },
    Text {
        text: String,
        font: String,
        color: [f32; 4],
        raster_asset: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub position: Track<[f32; 3]>,
    pub rotation: Track<[f32; 3]>,
    pub scale: Track<[f32; 3]>,
    pub opacity: Track<f32>,
    /// Normalized anchor in the layer's source rectangle, with Y pointing down.
    pub anchor: [f32; 2],
}
impl Transform {
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position: Track::constant(position),
            rotation: Track::constant([0.0; 3]),
            scale: Track::constant([100.0; 3]),
            opacity: Track::constant(1.0),
            anchor: [0.5; 2],
        }
    }
    pub fn validate(&self, frames: u32) -> Result<()> {
        self.validate_tracks(Some(frames))
    }
    pub fn validate_local(&self) -> Result<()> {
        self.validate_tracks(None)
    }
    fn validate_tracks(&self, frames: Option<u32>) -> Result<()> {
        if let Some(frames) = frames {
            self.position.validate(frames)?;
            self.rotation.validate(frames)?;
            self.scale.validate(frames)?;
            self.opacity.validate(frames)?;
        } else {
            self.position.validate_local()?;
            self.rotation.validate_local()?;
            self.scale.validate_local()?;
            self.opacity.validate_local()?;
        }
        ensure(
            self.anchor
                .into_iter()
                .all(|v| v.is_finite() && v.abs() <= 1000.0),
            "invalid anchor",
        )?;
        for (track, bound) in [
            (&self.position, 10_000_000.0),
            (&self.rotation, 1_000_000.0),
            (&self.scale, 100_000.0),
        ] {
            track.validate_bound(bound)?;
        }
        for value in
            std::iter::once(self.opacity.value).chain(self.opacity.keys.iter().map(|k| k.value))
        {
            ensure(
                (0.0..=1.0).contains(&value),
                "opacity must be between zero and one",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub content: Content,
    pub size: [f32; 2],
    pub transform: Transform,
    pub visible: bool,
    pub locked: bool,
    /// New layers are flat until the user explicitly enables spatial transforms.
    #[serde(default)]
    pub three_d: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentLink>,
    #[serde(default)]
    pub effects: Vec<crate::EffectInstance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<crate::masks::LayerMask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<LayerTimeline>,
}
impl Layer {
    pub fn solid(id: u64, name: &str, size: [f32; 2], position: [f32; 3], color: [f32; 4]) -> Self {
        Self {
            id,
            name: name.into(),
            content: Content::Solid { color },
            size,
            transform: Transform::new(position),
            visible: true,
            locked: false,
            three_d: false,
            parent: None,
            effects: Vec::new(),
            masks: Vec::new(),
            timeline: None,
        }
    }
    pub fn clip(&self, frames: u32) -> LayerTimeline {
        self.timeline.unwrap_or_else(|| LayerTimeline::full(frames))
    }
    pub fn local_frame(&self, composition_frame: f64) -> f64 {
        composition_frame - f64::from(self.timeline.map_or(0, |t| t.offset_frame))
    }
    pub fn active(&self, frame: f64, frames: u32) -> bool {
        let t = self.clip(frames);
        self.visible && frame >= f64::from(t.in_frame) && frame < f64::from(t.out_frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerTimeline {
    pub in_frame: u32,
    pub out_frame: u32,
    pub offset_frame: i32,
}
impl LayerTimeline {
    pub fn full(frames: u32) -> Self {
        Self {
            in_frame: 0,
            out_frame: frames,
            offset_frame: 0,
        }
    }
    pub fn validate(&self, frames: u32) -> Result<()> {
        ensure(
            self.in_frame < self.out_frame && self.out_frame <= frames,
            "invalid layer clip interval",
        )?;
        self.edit_frame(0)?;
        self.edit_frame(frames - 1)?;
        Ok(())
    }
    pub fn edit_frame(&self, frame: u32) -> Result<i32> {
        i32::try_from(i64::from(frame) - i64::from(self.offset_frame))
            .map_err(|_| crate::Error::Invalid("local keyframe time overflow".into()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentLink {
    pub object: Option<u64>,
    pub bind: [[f32; 4]; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    #[serde(default = "crate::composition::main_composition")]
    pub composition_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compositions: Vec<crate::Composition>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub background: [f32; 4],
    pub assets: Vec<Asset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_assets: Vec<crate::AudioAsset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_assets: Vec<crate::VideoAsset>,
    pub camera: Camera,
    /// Index zero is the back of the same-depth stack.
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub plugin_dependencies: Vec<crate::PluginDependency>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<crate::PropertyExpression>,
}
impl Project {
    pub fn new(width: u32, height: u32, fps: u32, frames: u32) -> Result<Self> {
        ensure(width > 0 && height > 0, "composition size must be positive")?;
        let mut project = Self {
            version: 8,
            composition_id: crate::MAIN_COMPOSITION.into(),
            compositions: Vec::new(),
            name: "空间练习 01".into(),
            width,
            height,
            fps,
            frames,
            background: [0.05, 0.06, 0.09, 1.0],
            assets: Vec::new(),
            audio_assets: Vec::new(),
            video_assets: Vec::new(),
            camera: Camera::new(width, height),
            layers: Vec::new(),
            plugin_dependencies: Vec::new(),
            expressions: Vec::new(),
        };
        project.camera.created = false;
        project.validate()?;
        Ok(project)
    }
    pub fn demo() -> Self {
        let mut p = Self::new(1080, 1920, 30, 180).unwrap();
        p.camera.created = true;
        p.layers = vec![
            Layer::solid(
                1,
                "背景",
                [1800.0, 2400.0],
                [540.0, 960.0, 500.0],
                [0.07, 0.18, 0.25, 1.0],
            ),
            Layer::solid(
                2,
                "主体卡片",
                [560.0, 760.0],
                [540.0, 960.0, 0.0],
                [0.22, 0.83, 0.75, 1.0],
            ),
            Layer::solid(
                3,
                "前景",
                [240.0, 400.0],
                [260.0, 1280.0, -500.0],
                [0.88, 0.73, 0.43, 0.85],
            ),
        ];
        // This is an explicitly spatial example, not the empty-project default.
        for layer in &mut p.layers {
            layer.three_d = true;
        }
        p
    }
    pub fn validate(&self) -> Result<()> {
        ensure((1..=8).contains(&self.version), "unsupported project format")?;
        ensure(self.plugin_dependencies == self.composition_dependencies(),
               "plugin dependency list does not match effect instances")?;
        self.validate_one(self)?;
        self.validate_compositions()
    }
    pub(crate) fn validate_one(&self, document: &Project) -> Result<()> {
        ensure(
            (1..=8).contains(&self.version),
            "unsupported project format",
        )?;
        ensure(
            self.version >= 2 || self.layers.iter().all(|l| l.effects.is_empty()),
            "version 1 projects cannot contain effects",
        )?;
        ensure(
            self.version >= 4
                || self
                    .layers
                    .iter()
                    .all(|l| l.effects.iter().all(|e| e.scene.is_none())),
            "scene generator projects require format 4",
        )?;
        ensure(
            (1..=8192).contains(&self.width) && (1..=8192).contains(&self.height),
            "invalid composition size",
        )?;
        ensure(
            (1..=MAX_COMPOSITION_FPS).contains(&self.fps),
            "composition frame rate must be 1..240",
        )?;
        ensure(
            (1..=MAX_FRAMES).contains(&self.frames),
            "invalid composition duration",
        )?;
        ensure(self.name.len() <= 1024, "project name too long")?;
        validate_color(self.background)?;
        ensure(self.layers.len() <= MAX_LAYERS, "layer limit exceeded")?;
        ensure(self.assets.len() <= MAX_LAYERS * 2, "asset limit exceeded")?;
        self.camera.validate(self.frames)?;
        ensure(
            self.version >= 5 || self.video_assets.is_empty(),
            "video requires project format five",
        )?;
        ensure(
            self.video_assets.len() <= MAX_LAYERS * 2,
            "video asset limit exceeded",
        )?;
        ensure(
            self.version >= 4 || self.audio_assets.is_empty(),
            "audio requires project format four",
        )?;
        let mut asset_ids = HashSet::new();
        for asset in &self.assets {
            ensure(
                asset.id != 0 && asset_ids.insert(asset.id),
                "asset IDs must be unique and nonzero",
            )?;
            ensure(
                (1..=16384).contains(&asset.width) && (1..=16384).contains(&asset.height),
                "invalid source image size",
            )?;
            crate::storage::validate_relative_path(&asset.path)?;
        }
        ensure(
            self.audio_assets.len() + self.video_assets.len() + self.assets.len() <= MAX_LAYERS * 2,
            "asset limit exceeded",
        )?;
        for asset in &self.audio_assets {
            ensure(
                asset.id != 0 && asset_ids.insert(asset.id),
                "asset IDs must be unique and nonzero",
            )?;
            asset.validate()?;
        }
        for asset in &self.video_assets {
            ensure(
                asset.id != 0 && asset_ids.insert(asset.id),
                "asset IDs must be unique and nonzero",
            )?;
            asset.validate()?;
            if let Some(id) = asset.audio_asset {
                let audio = self
                    .audio_assets
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or_else(|| crate::Error::Invalid("video audio asset missing".into()))?;
                ensure(
                    audio.path == asset.path
                        && audio.bytes == asset.bytes
                        && audio.duration_us <= asset.duration_us,
                    "video/audio source timeline mismatch",
                )?;
            }
        }
        let mut audio_paths = HashSet::new();
        for a in &self.audio_assets {
            ensure(
                audio_paths.insert(a.path.to_lowercase()),
                "audio assets cannot share different track caches at one path",
            )?;
            if let Some(v) = self
                .video_assets
                .iter()
                .find(|v| v.path.eq_ignore_ascii_case(&a.path))
            {
                ensure(
                    v.path == a.path && v.audio_asset == Some(a.id),
                    "shared MP4 must reference its matching audio asset",
                )?;
            }
        }
        let mut video_paths = HashSet::new();
        for a in &self.video_assets {
            ensure(
                video_paths.insert(a.path.to_lowercase()),
                "duplicate video source path",
            )?;
        }
        let mut ids = HashSet::new();
        for layer in &self.layers {
            ensure(self.version >= 8 || layer.masks.is_empty(), "layer masks require project format eight")?;
            crate::masks::validate(&layer.masks).map_err(|e| crate::Error::Invalid(format!("layer {}: {e}",layer.id)))?;
            ensure(layer.masks.is_empty() || !matches!(layer.content, Content::Null | Content::Audio {..} | Content::Adjustment),
                "source masks require an ordinary visual layer; adjustment masks require composition compositing")?;
            ensure(
                layer.id != 0 && ids.insert(layer.id),
                "layer IDs must be unique and nonzero",
            )?;
            ensure(layer.name.len() <= 1024, "layer name too long")?;
            ensure(
                matches!(layer.content, Content::Audio { .. }) && layer.size == [0.0; 2]
                    || layer
                        .size
                        .into_iter()
                        .all(|v| v.is_finite() && v > 0.0 && v <= 32768.0),
                "invalid layer size",
            )?;
            layer.clip(self.frames).validate(self.frames)?;
            if self.version == 1 && layer.timeline.is_none() {
                layer.transform.validate(self.frames)?;
            } else {
                layer.transform.validate_local()?;
            }
            ensure(
                layer.effects.len() <= aem_effects::MAX_EFFECTS_PER_LAYER,
                "too many layer effects",
            )?;
            ensure(
                !matches!(layer.content, Content::Null) || layer.effects.is_empty(),
                "null layers cannot contain effects",
            )?;
            let mut effect_ids = HashSet::new();
            for e in &layer.effects {
                ensure(effect_ids.insert(e.id), "duplicate effect instance ID")?;
                e.validate(self.frames)?;
            }
            match &layer.content {
                Content::Adjustment => {
                    ensure(self.version >= 6, "adjustment layers require format six")?;
                    ensure(!layer.three_d, "adjustment layers must be 2D")?;
                    ensure(
                        layer.effects.iter().all(|e| e.scene.is_none()),
                        "adjustment layers cannot contain scene generators",
                    )?;
                }
                Content::Vector { vector } => {
                    ensure(self.version >= 6, "vector layers require format six")?;
                    vector.validate().map_err(|e| {
                        crate::Error::Invalid(format!("layer {} vector: {e}", layer.id))
                    })?;
                }
                Content::Composition { clip } => {
                    document.composition_view(&clip.composition)?;
                    ensure(clip.volume.is_finite() && (0.0..=4.0).contains(&clip.volume)
                        && clip.source_start_frame.unsigned_abs() <= MAX_FRAMES, "invalid composition source interval or volume")?;
                }
                Content::Video { video } => {
                    video.validate()?;
                    let asset = self
                        .video_assets
                        .iter()
                        .find(|a| a.id == video.asset)
                        .ok_or_else(|| crate::Error::Invalid("video asset missing".into()))?;
                    let clip = layer.clip(self.frames);
                    let start = i128::from(video.source_offset_us) * i128::from(self.fps)
                        + (i128::from(clip.in_frame) - i128::from(clip.offset_frame)) * 1_000_000;
                    let last = i128::from(video.source_offset_us) * i128::from(self.fps)
                        + (i128::from(clip.out_frame) - 1 - i128::from(clip.offset_frame))
                            * 1_000_000;
                    ensure(
                        start >= 0 && last < i128::from(asset.duration_us) * i128::from(self.fps),
                        "video clip exceeds recoverable source interval",
                    )?;
                }
                Content::Audio { audio } => {
                    audio.validate()?;
                    let asset = self
                        .audio_assets
                        .iter()
                        .find(|a| a.id == audio.asset)
                        .ok_or_else(|| {
                            crate::Error::Invalid("audio references a missing audio asset".into())
                        })?;
                    ensure(
                        !layer.three_d
                            && layer.parent.is_none()
                            && layer.size == [0.0; 2]
                            && layer.transform == Transform::new([0.0; 3]),
                        "audio has no spatial properties or parent",
                    )?;
                    let clip = layer.clip(self.frames);
                    let start = i128::from(audio.source_offset_us) * i128::from(self.fps)
                        + (i128::from(clip.in_frame) - i128::from(clip.offset_frame)) * 1_000_000;
                    let last = i128::from(audio.source_offset_us) * i128::from(self.fps)
                        + (i128::from(clip.out_frame) - 1 - i128::from(clip.offset_frame))
                            * 1_000_000;
                    ensure(
                        start >= 0 && last < i128::from(asset.duration_us) * i128::from(self.fps),
                        "audio clip exceeds recoverable source interval",
                    )?;
                }
                Content::Null => {}
                Content::Solid { color } => validate_color(*color)?,
                Content::Image { asset } => ensure(
                    self.assets.iter().any(|a| a.id == *asset),
                    "image references a missing asset",
                )?,
                Content::Text {
                    text,
                    font,
                    color,
                    raster_asset,
                } => {
                    ensure(
                        text.len() <= 65536 && font.len() <= 256,
                        "text or font identifier too long",
                    )?;
                    validate_color(*color)?;
                    ensure(
                        self.assets.iter().any(|a| a.id == *raster_asset),
                        "text raster asset is missing",
                    )?;
                }
            }
        }
        crate::hierarchy::validate(self)?;
        crate::expressions::validate(self)?;
        Ok(())
    }
    pub fn frame_pts_us(&self, frame: u32) -> Result<i64> {
        ensure(frame < self.frames, "output frame outside composition")?;
        Ok((u64::from(frame) * 1_000_000 + u64::from(self.fps) / 2) as i64 / i64::from(self.fps))
    }
    pub fn layer_audio(&self, layer: &Layer) -> Option<crate::AudioClip> {
        match &layer.content {
            Content::Audio { audio } => Some(audio.clone()),
            Content::Video { video } => self
                .video_assets
                .iter()
                .find(|a| a.id == video.asset)
                .and_then(|a| a.audio_asset)
                .map(|asset| crate::AudioClip {
                    asset,
                    source_offset_us: video.source_offset_us,
                    volume: video.volume,
                    muted: video.muted,
                }),
            _ => None,
        }
    }
    /// Upgrade in memory only. Opening an old project never overwrites its file.
    pub fn migrate(mut self) -> Result<Self> {
        self.validate()?;
        if self.version == 1 {
            self.version = 2;
            for layer in &mut self.layers {
                layer
                    .timeline
                    .get_or_insert_with(|| LayerTimeline::full(self.frames));
            }
        }
        if self.version < 3 {
            // Versions 1/2 rendered every layer in 3D. Preserve that appearance.
            for layer in &mut self.layers {
                layer.three_d = true;
            }
            self.version = 3;
        }
        self.version = 8;
        Ok(self)
    }
    pub fn edit_frame(&self, object: u64, frame: u32) -> Result<i32> {
        ensure(frame < self.frames, "edit frame outside the composition")?;
        if object == 0 {
            return Ok(frame as i32);
        }
        self.layers
            .iter()
            .find(|l| l.id == object)
            .ok_or(crate::Error::Missing(object))?
            .clip(self.frames)
            .edit_frame(frame)
    }
    pub fn layer_mut(&mut self, id: u64) -> Result<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or(crate::Error::Missing(id))
    }
    pub fn estimated_bytes(&self) -> usize {
        // A bounded approximation for history, independent of image/GPU caches.
        serde_json::to_vec(self).map_or(usize::MAX, |v| v.len())
    }
    pub fn rebuild_plugin_dependencies(&mut self) {
        self.plugin_dependencies = self.composition_dependencies();
    }
    fn composition_dependencies(&self) -> Vec<crate::PluginDependency> {
        crate::effects::dependencies(self.layers.iter().chain(self.compositions.iter().flat_map(|c|c.layers.iter())))
    }
}
fn validate_color(color: [f32; 4]) -> Result<()> {
    ensure(
        color
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
        "invalid RGBA color",
    )
}
