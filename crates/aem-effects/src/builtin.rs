use crate::{EffectPackage, Error, PluginManifest, Result};
use std::sync::{Arc, OnceLock};
pub const PLUGIN_ID: &str = "com.motionstudio.effects.ae2021";
pub fn manifest() -> PluginManifest {
    serde_json::from_str(include_str!("../builtin-library/manifest.json"))
        .expect("checked-in builtin manifest")
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
        legacy_current_package()?,
        package()?,
        legacy_scene_package()?,
        scene_package()?,
        particle_package()?,
    ])
}

/// Catalogue aliases/presets, never duplicate implementations or saved IDs.
pub fn effect_aliases() -> serde_json::Value {
    use serde_json::json;
    let mut aliases = vec![
        json!({"name":"Duotone","plugin":PLUGIN_ID,"effect":"tint","parameters":{}}),
        json!({"name":"RectangularCoordinates","plugin":PLUGIN_ID,"effect":"polar_coordinates","parameters":{"p0001":[1,0,0,0],"p0002":[2,0,0,0]}}),
        json!({"name":"ChromaticAberration","plugin":PLUGIN_ID,"effect":"warp_chroma","parameters":{}}),
        json!({"name":"AngularBlur","plugin":PLUGIN_ID,"effect":"radial_blur","parameters":{"p0003":[1,0,0,0]}}),
        json!({"name":"SwirlDistortion","plugin":PLUGIN_ID,"effect":"twirl","parameters":{}}),
        json!({"name":"BlockNoise","plugin":PLUGIN_ID,"effect":"fractal_noise","parameters":{"noise_type":[1,0,0,0]}}),
    ];
    for (name, mode) in [
        ("LinearGradient", 0),
        ("RadialGradient", 1),
        ("ConicGradient", 2),
        ("DiamondGradient", 3),
        ("ColorWheel", 4),
        ("DirectionalRainbow", 5),
    ] {
        aliases.push(json!({"name":name,"plugin":PLUGIN_ID,"effect":"gradient","parameters":{"shape":[mode,0,0,0]}}));
    }
    for (name, mode) in [
        ("PerlinNoise", 0),
        ("SimplexNoise", 1),
        ("Voronoi", 2),
        ("GaborNoise", 3),
        ("BlueNoise", 5),
    ] {
        aliases.push(json!({"name":name,"plugin":PLUGIN_ID,"effect":"noise_generator","parameters":{"basis":[mode,0,0,0]}}));
    }
    for (name, mode) in [("ProgressiveBlur", 0), ("TiltShift", 1)] {
        aliases.push(json!({"name":name,"plugin":PLUGIN_ID,"effect":"region_blur","parameters":{"mode":[mode,0,0,0]}}));
    }
    aliases.push(json!({"name":"MeshGradient","plugin":PLUGIN_ID,"effect":"flow_gradient","parameters":{}}));
    json!(aliases)
}

/// The creative subset belongs to the one builtin package; no second package.
pub fn creative_effects() -> Vec<crate::EffectDefinition> {
    manifest().effects.into_iter().filter(|d|d.compatibility_profile=="motion-native-v1").collect()
}
pub fn legacy_current_package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE.get_or_init(|| EffectPackage::from_bytes(include_bytes!("../library/core-effects.msfx").to_vec())
        .map(Arc::new).map_err(|e|e.to_string())).clone().map_err(Error::Invalid)
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
        EffectPackage::from_bytes(include_bytes!("../builtin-library/builtin-effects.msfx").to_vec())?;
    if package.manifest != manifest() {
        return Err(Error::Invalid("bundled package is stale; run effect_tool pack crates/aem-effects/builtin-library crates/aem-effects/builtin-library/builtin-effects.msfx".into()));
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
