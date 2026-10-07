//! Versioned native UI slots. Descriptions contain data, never executable UI code.
use crate::{ensure, ParamDefinition, ParamKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEditorDefinition {
    pub id: String,
    pub protocol: u32,
    pub title: String,
    pub sections: Vec<NativeSection>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSection {
    pub id: String,
    pub title: String,
    pub slots: Vec<NativeSlot>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeSlot {
    Parameters { params: Vec<String> },
    Preview { max_size: u32 },
    Timeline,
    LayerSource,
    ImageSprite,
    Seed,
    Transform,
    Note { text: String },
}
impl NativeEditorDefinition {
    pub(crate) fn validate(
        &self,
        params: &[ParamDefinition],
        renderer: crate::RendererKind,
    ) -> Result<()> {
        ensure(
            crate::valid_id(&self.id)
                && self.protocol == 1
                && !self.title.is_empty()
                && self.title.len() <= 256,
            "invalid native editor identity or protocol",
        )?;
        ensure(
            (1..=16).contains(&self.sections.len())
                && self.sections.iter().map(|s| s.slots.len()).sum::<usize>() <= 128,
            "native editor exceeds section/slot limits",
        )?;
        let mut ids = BTreeSet::new();
        let mut used = BTreeSet::new();
        let mut singletons = BTreeSet::new();
        for section in &self.sections {
            ensure(
                crate::valid_id(&section.id)
                    && ids.insert(&section.id)
                    && !section.title.is_empty()
                    && section.title.len() <= 256
                    && (1..=32).contains(&section.slots.len()),
                "invalid or duplicate native section",
            )?;
            for slot in &section.slots {
                let singleton = match slot {
                    NativeSlot::Parameters { params: names } => {
                        ensure(
                            (1..=32).contains(&names.len()),
                            "invalid native parameter slot",
                        )?;
                        for name in names {
                            ensure(used.insert(name), "duplicate native parameter binding")?;
                            let param = params.iter().find(|p| p.id == *name).ok_or_else(|| {
                                crate::Error::Invalid(format!("native parameter {name} missing"))
                            })?;
                            ensure(
                                param.kind != ParamKind::Curve,
                                "native UI protocol 1 does not include a curve-object control",
                            )?;
                        }
                        None
                    }
                    NativeSlot::Preview { max_size } => {
                        ensure(
                            (1..=512).contains(max_size),
                            "native preview size exceeds 512",
                        )?;
                        Some("preview")
                    }
                    NativeSlot::Timeline => Some("timeline"),
                    NativeSlot::LayerSource => {
                        ensure(
                            renderer == crate::RendererKind::ParticleEmitter,
                            "native source slot requires a particle emitter",
                        )?;
                        Some("source")
                    }
                    NativeSlot::ImageSprite => {
                        ensure(
                            renderer == crate::RendererKind::ParticleEmitter,
                            "native image slot requires a particle emitter",
                        )?;
                        Some("sprite")
                    }
                    NativeSlot::Seed => {
                        ensure(
                            renderer != crate::RendererKind::Image,
                            "native seed slot requires a generator",
                        )?;
                        Some("seed")
                    }
                    NativeSlot::Transform => Some("transform"),
                    NativeSlot::Note { text } => {
                        ensure(
                            !text.is_empty() && text.len() <= 2048,
                            "invalid native note",
                        )?;
                        None
                    }
                };
                if let Some(key) = singleton {
                    ensure(singletons.insert(key), "duplicate native singleton slot")?;
                }
            }
        }
        Ok(())
    }
}
