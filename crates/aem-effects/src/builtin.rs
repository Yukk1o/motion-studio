use crate::{EffectPackage, Error, PluginManifest, Result};
use std::sync::{Arc, OnceLock};
pub const PLUGIN_ID: &str = "com.motionstudio.effects.ae2021";
pub fn manifest() -> PluginManifest {
    serde_json::from_str(include_str!("../library/manifest.json"))
        .expect("checked-in core manifest")
}
pub fn package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| build().map(Arc::new).map_err(|e| e.to_string()))
        .clone()
        .map_err(Error::Invalid)
}
/// Retain the published 1.0.0 bytes so existing projects keep their exact hash.
pub fn packages() -> Result<Vec<Arc<EffectPackage>>> {
    static PREVIOUS: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    let previous = PREVIOUS
        .get_or_init(|| {
            EffectPackage::from_bytes(
                include_bytes!("../library/legacy/core-effects-1.0.0.msfx").to_vec(),
            )
            .map(Arc::new)
            .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Error::Invalid)?;
    static CREATIVE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    let creative = CREATIVE
        .get_or_init(|| {
            EffectPackage::from_bytes(
                include_bytes!("../library/legacy/core-effects-1.1.0.msfx").to_vec(),
            )
            .map(Arc::new)
            .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Error::Invalid)?;
    static COMMON: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    let common = COMMON
        .get_or_init(|| {
            EffectPackage::from_bytes(
                include_bytes!("../library/legacy/core-effects-1.2.0.msfx").to_vec(),
            )
            .map(Arc::new)
            .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Error::Invalid)?;
    static TILING: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    let tiling = TILING.get_or_init(|| {
        EffectPackage::from_bytes(include_bytes!("../library/legacy/core-effects-1.3.0.msfx").to_vec())
            .map(Arc::new).map_err(|e| e.to_string())
    }).clone().map_err(Error::Invalid)?;
    static SPATIAL: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    let spatial = SPATIAL.get_or_init(|| {
        EffectPackage::from_bytes(include_bytes!("../library/legacy/core-effects-1.4.0.msfx").to_vec())
            .map(Arc::new).map_err(|e| e.to_string())
    }).clone().map_err(Error::Invalid)?;
    Ok(vec![
        previous,
        creative,
        common,
        tiling,
        spatial,
        package()?,
        legacy_scene_package()?,
        scene_package()?,
        particle_package()?,
    ])
}
pub fn particle_package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE.get_or_init(|| EffectPackage::from_bytes(include_bytes!("../particle-library/particle-effects.msfx").to_vec())
        .map(Arc::new).map_err(|e| e.to_string())).clone().map_err(Error::Invalid)
}
fn build() -> Result<EffectPackage> {
    // Every host installs the exact published package bytes, independent of the
    // target platform's ZIP writer and source checkout line endings.
    let package =
        EffectPackage::from_bytes(include_bytes!("../library/core-effects.msfx").to_vec())?;
    if package.manifest != manifest() {
        return Err(Error::Invalid("bundled package is stale; run effect_tool pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx".into()));
    }
    Ok(package)
}

/// SDK 2 generators use a distinct stable identity; SDK 1 package bytes stay pinned.
pub fn legacy_scene_package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| {
            EffectPackage::from_bytes(
                include_bytes!("../scene-library/legacy/scene-effects-1.0.0.msfx").to_vec(),
            )
            .map(Arc::new)
            .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Error::Invalid)
}

/// Latest scene package; already saved instances keep their exact dependency.
pub fn scene_package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| {
            EffectPackage::from_bytes(
                include_bytes!("../scene-library/scene-effects.msfx").to_vec(),
            )
            .map(Arc::new)
            .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(Error::Invalid)
}
