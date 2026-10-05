use crate::{EffectPackage, Error, PluginManifest, Result};
use std::sync::{Arc, OnceLock};
pub const PLUGIN_ID: &str = "com.motionstudio.effects.ae2021";
pub fn manifest() -> PluginManifest {
    serde_json::from_str(include_str!("../library/manifest.json")).expect("checked-in AE manifest")
}
pub fn package() -> Result<Arc<EffectPackage>> {
    static PACKAGE: OnceLock<std::result::Result<Arc<EffectPackage>, String>> = OnceLock::new();
    PACKAGE
        .get_or_init(|| build().map(Arc::new).map_err(|e| e.to_string()))
        .clone()
        .map_err(Error::Invalid)
}
fn build() -> Result<EffectPackage> {
    // Every host installs the exact published package bytes, independent of the
    // target platform's ZIP writer and source checkout line endings.
    let package = EffectPackage::from_bytes(include_bytes!("../library/core-effects.msfx").to_vec())?;
    if package.manifest != manifest() {
        return Err(Error::Invalid("bundled package is stale; run effect_tool pack crates/aem-effects/library crates/aem-effects/library/core-effects.msfx".into()));
    }
    Ok(package)
}
