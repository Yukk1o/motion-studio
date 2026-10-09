//! Image inspection, bounded preview proxies and full-resolution decode.
use crate::session::Result;
use aem_core::Asset;
use serde_json::json;
use std::path::PathBuf;

/// Encoded image metadata used before an asset is registered.
pub fn image_info(path: &str) -> Result<serde_json::Value> {
    let (width, height, format, bytes) =
        aem_render::image_resources::inspect(std::path::Path::new(path))?;
    Ok(json!({"version":1,"width":width,"height":height,"format":format!("{format:?}"),"bytes":bytes,
        "maxEncodedBytes":aem_render::image_resources::MAX_ENCODED_BYTES,
        "previewMaxEdge":aem_render::image_resources::MAX_PREVIEW_EDGE}))
}

/// Validate a project-relative image path and warm its preview proxy.
pub fn prepare_image(root: PathBuf, path: String) -> Result<serde_json::Value> {
    aem_core::storage::validate_relative_path(&path).map_err(|e| e.to_string())?;
    let (width, height, format, bytes) =
        aem_render::image_resources::inspect(&root.join(&path))?;
    let asset = Asset {
        id: 1,
        path: path.clone(),
        width,
        height,
    };
    let source = aem_render::image_resources::Source::new(&root, &asset)?;
    let proxy = aem_render::image_resources::decode(
        &source,
        aem_render::image_resources::Resolution::Preview(
            aem_render::image_resources::MAX_PREVIEW_EDGE,
        ),
    )?;
    Ok(json!({"version":1,"path":path,"width":width,"height":height,"format":format!("{format:?}"),
        "bytes":bytes,"validated":true,
        "proxyWidth":proxy.width,"proxyHeight":proxy.height,"proxyCached":proxy.cached}))
}

/// Decode one registered asset at original resolution into a caller buffer.
pub fn asset_pixels_into(id: i64, asset: i64, out: &mut [u8]) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| asset_pixels_into_inner(s, asset, out))
}

pub fn asset_pixels_into_inner(
    s: &mut crate::session::Session,
    asset: i64,
    out: &mut [u8],
) -> Result<serde_json::Value> {
    {
        let a = s
            .engine
            .project()
            .assets
            .iter()
            .find(|a| a.id == asset as u64)
            .ok_or("asset not found")?;
        let bytes = u64::from(a.width) * u64::from(a.height) * 4;
        if bytes > aem_render::image_resources::MAX_DECODED_BYTES || (out.len() as u64) < bytes {
            return Err("image buffer is too small or exceeds 128 MiB".into());
        }
        let source = aem_render::image_resources::Source::new(&s.root, a)?;
        aem_render::image_resources::decode_into(
            &source,
            aem_render::image_resources::Resolution::Full,
            &mut out[..bytes as usize],
        )?;
        Ok(json!({"version":1,"asset":a.id,"width":a.width,"height":a.height,"bytes":bytes,"resolution":"original"}))
    }
}