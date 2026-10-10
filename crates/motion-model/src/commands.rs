use crate::{Asset, CameraMode, Content, Ease, Easing, Layer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    Position,
    Rotation,
    Scale,
    Opacity,
    Target,
    Roll,
    Fov,
    Radius,
    Azimuth,
    Elevation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Zero edits the active composition background. Visual layer source
    /// colors remain static; animated color uses a color effect or vector paint.
    SetColor { object:u64, value:[f32;4] },
    AddAdjustment {
        id: u64,
        name: String,
    },
    AddShape {
        id: u64,
        name: String,
        shape: crate::vector::ShapeKind,
        size: [f32; 2],
        position: [f32; 3],
    },
    Vector {
        object: u64,
        action: crate::vector::VectorAction,
    },
    Mask { object: u64, action: crate::masks::MaskAction },
    SetLayerBlend {object:u64,#[serde(default)]mode:Option<crate::compositing::BlendMode>,#[serde(default)]space:Option<crate::compositing::BlendSpace>},
    SetTrackMatte {object:u64,matte:Option<crate::compositing::TrackMatte>},
    InComposition { composition: String, command: Box<Command> },
    Composition { action: crate::CompositionAction },
    RegisterAudioAsset {
        asset: crate::AudioAsset,
    },
    RegisterVideoAsset {
        asset: crate::VideoAsset,
    },
    SetAudio {
        object: u64,
        #[serde(default)]
        volume: Option<f32>,
        #[serde(default)]
        muted: Option<bool>,
    },
    #[serde(rename = "set_layer_3d")]
    SetLayer3d {
        object: u64,
        enabled: bool,
    },
    SetExpression {
        expression: crate::PropertyExpression,
        frame: u32,
    },
    RemoveExpression {
        target: crate::ExpressionTarget,
    },
    Effect {
        object: u64,
        action: crate::EffectAction,
    },
    SeparateDimensions {
        object: u64,
        property: Property,
    },
    SetComponent {
        object: u64,
        property: Property,
        axis: crate::Axis,
        frame: u32,
        value: f32,
    },
    MoveLayerClip {
        object: u64,
        in_frame: u32,
    },
    TrimLayerClip {
        object: u64,
        in_frame: u32,
        out_frame: u32,
    },
    SplitLayerClip {
        object: u64,
        frame: u32,
    },
    Curve {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        easing: Easing,
    },
    Spatial {
        object: u64,
        property: Property,
        frame: u32,
        tangents: Option<crate::SpatialTangents<[f32; 3]>>,
    },
    CreateCamera,
    Remove {
        object: u64,
        frame: u32,
    },
    Parent {
        object: u64,
        parent: Option<u64>,
        frame: u32,
    },
    CopyKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        from: u32,
        to: u32,
    },
    RegisterAsset {
        asset: Asset,
    },
    RegisterFontAsset {
        asset: crate::FontAsset,
    },
    Content {
        object: u64,
        content: Content,
        size: [f32; 2],
    },
    SetVector {
        object: u64,
        property: Property,
        frame: u32,
        value: [f32; 3],
    },
    SetScalar {
        object: u64,
        property: Property,
        frame: u32,
        value: f32,
    },
    Animate {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        enabled: bool,
    },
    MoveKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        from: u32,
        to: u32,
    },
    DeleteKey {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
    },
    Ease {
        object: u64,
        property: Property,
        #[serde(default)]
        axis: Option<crate::Axis>,
        frame: u32,
        ease: Ease,
    },
    Add {
        layer: Layer,
    },
    Delete {
        object: u64,
    },
    Duplicate {
        object: u64,
    },
    Rename {
        object: u64,
        name: String,
    },
    Reorder {
        object: u64,
        index: usize,
    },
    Flags {
        object: u64,
        visible: bool,
        locked: bool,
    },
    Anchor {
        object: u64,
        anchor: [f32; 2],
    },
    CameraMode {
        mode: CameraMode,
    },
    Dolly {
        frame: u32,
        amount: f32,
    },
    Pan {
        frame: u32,
        x: f32,
        y: f32,
    },
}

impl Command {
    pub fn changes_resources(&self) -> bool {
        match self {
            Self::InComposition { command, .. } => command.changes_resources(),
            Self::RegisterAsset { .. } | Self::Content { .. } | Self::Add { .. } | Self::Composition { .. } => true,
            _ => false,
        }
    }
    pub fn composition_transaction(&self) -> bool {
        match self { Self::InComposition {command,..} => command.composition_transaction(), Self::Composition {..} => true, _ => false }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditResult {
    SplitLayerClip { left_object: u64, right_object: u64 },
    Composition { result: serde_json::Value },
}
