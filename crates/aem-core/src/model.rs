use crate::{ensure, Camera, Result, Track};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_LAYERS: usize = 128;
pub const MAX_FRAMES: u32 = 36_000;

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
        self.position.validate(frames)?;
        self.rotation.validate(frames)?;
        self.scale.validate(frames)?;
        self.opacity.validate(frames)?;
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
            for value in std::iter::once(track.value).chain(track.keys.iter().map(|k| k.value)) {
                ensure(
                    value.into_iter().all(|v| v.abs() <= bound),
                    "transform value exceeds its numeric range",
                )?;
            }
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub background: [f32; 4],
    pub assets: Vec<Asset>,
    pub camera: Camera,
    /// Index zero is the back of the same-depth stack.
    pub layers: Vec<Layer>,
}
impl Project {
    pub fn new(width: u32, height: u32, fps: u32, frames: u32) -> Result<Self> {
        ensure(width > 0 && height > 0, "composition size must be positive")?;
        let project = Self {
            version: 1,
            name: "空间练习 01".into(),
            width,
            height,
            fps,
            frames,
            background: [0.05, 0.06, 0.09, 1.0],
            assets: Vec::new(),
            camera: Camera::new(width, height),
            layers: Vec::new(),
        };
        project.validate()?;
        Ok(project)
    }
    pub fn demo() -> Self {
        let mut p = Self::new(1080, 1920, 30, 180).unwrap();
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
        p
    }
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == 1, "unsupported project format")?;
        ensure(
            (1..=8192).contains(&self.width) && (1..=8192).contains(&self.height),
            "invalid composition size",
        )?;
        ensure(
            matches!(self.fps, 30 | 60),
            "composition frame rate must be 30 or 60",
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
        let mut ids = HashSet::new();
        for layer in &self.layers {
            ensure(
                layer.id != 0 && ids.insert(layer.id),
                "layer IDs must be unique and nonzero",
            )?;
            ensure(layer.name.len() <= 1024, "layer name too long")?;
            ensure(
                layer
                    .size
                    .into_iter()
                    .all(|v| v.is_finite() && v > 0.0 && v <= 32768.0),
                "invalid layer size",
            )?;
            layer.transform.validate(self.frames)?;
            match &layer.content {
                Content::Solid { color } => validate_color(*color)?,
                Content::Image { asset } => ensure(
                    asset_ids.contains(asset),
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
                        asset_ids.contains(raster_asset),
                        "text raster asset is missing",
                    )?;
                }
            }
        }
        Ok(())
    }
    pub fn frame_pts_us(&self, frame: u32) -> Result<i64> {
        ensure(frame < self.frames, "output frame outside composition")?;
        Ok((u64::from(frame) * 1_000_000 + u64::from(self.fps) / 2) as i64 / i64::from(self.fps))
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
}
fn validate_color(color: [f32; 4]) -> Result<()> {
    ensure(
        color
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
        "invalid RGBA color",
    )
}
