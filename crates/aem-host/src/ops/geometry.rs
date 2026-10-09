//! Geometry queries for hit testing and packed parameter/vertex output.
use crate::session::Result;
use serde_json::json;

/// Topmost layers under a composition-space point, front to back.
pub fn hit_candidates(id: i64, x: f64, y: f64) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        s.sample()?;
        let mut picking = s.scene.clone();
        for layer in &mut picking.layers {
            *layer = aem_core::selection_geometry::picking_layer(layer, &s.scene, &s.effects.registry);
        }
        let candidates = picking
            .hit_candidates([x as f32, y as f32])
            .map_err(|e| e.to_string())?;
        Ok(json!({"candidates":candidates,"coordinates":"composition_pixels","selection":"geometry_bounds"}))
    })
}

/// Packed plane parameters and vertices for one frame.
///
/// Buffers are written as `batches * 128` parameter bytes followed by
/// `vertices * 20` vertex bytes. Video layers are referenced by a negative
/// texture slot so the host can upload decoded frames by layer order.
pub fn sample_geometry_into(
    id: i64,
    frame: f64,
    parameters: &mut [u8],
    vertices: &mut [u8],
) -> Result<serde_json::Value> {
    crate::session::with_session(id, |s| {
        sample_geometry_into_inner(s, frame, parameters, vertices)
    })
}

pub fn sample_geometry_into_inner(
    s: &mut crate::session::Session,
    frame: f64,
    parameters: &mut [u8],
    vertices: &mut [u8],
) -> Result<serde_json::Value> {
    {
        s.scene
            .sample(s.engine.project(), frame, None)
            .map_err(|e| e.to_string())?;
        if s.scene
            .layers
            .iter()
            .any(|l| l.adjustment || l.vector.is_some())
        {
            return Err("vector and adjustment sources require render plan version 4".into());
        }
        s.geometry.prepare(&s.scene).map_err(|e| e.to_string())?;
        let pb = s.geometry.batches.len() * 128;
        let vb = s.geometry.vertices.len() * 20;
        if parameters.len() < pb || vertices.len() < vb {
            return Err(format!(
                "geometry buffers too small: need {pb} parameter bytes and {vb} vertex bytes"
            ));
        }
        for (i, batch) in s.geometry.batches.iter().enumerate() {
            let layer = &s.scene.layers[batch.layer];
            let mut data = [0.0f32; 32];
            data[..16].copy_from_slice(&layer.view_projection.to_cols_array());
            data[16..20].copy_from_slice(&layer.color);
            for c in &mut data[16..19] {
                *c = if *c <= 0.04045 {
                    *c / 12.92
                } else {
                    ((*c + 0.055) / 1.055).powf(2.4)
                };
            }
            data[20] = layer.size[0];
            data[21] = layer.size[1];
            data[22] = layer.opacity;
            data[24] = layer.asset.map_or(0.0, |id| {
                s.engine
                    .project()
                    .assets
                    .iter()
                    .position(|a| a.id == id)
                    .map_or(0.0, |i| i as f32 + 1.0)
            });
            if layer.video.is_some() {
                data[24] = -(layer.order as f32 + 1.0);
            }
            data[25] = batch.vertices.start as f32;
            data[26] = (batch.vertices.end - batch.vertices.start) as f32;
            data[27] = layer.order as f32;
            data[28] = if layer.three_d { 1.0 } else { 0.0 };
            parameters[i * 128..(i + 1) * 128].copy_from_slice(bytemuck::cast_slice(&data));
        }
        for (i, v) in s.geometry.vertices.iter().enumerate() {
            let data = [
                v.position[0],
                v.position[1],
                v.position[2],
                v.uv[0],
                v.uv[1],
            ];
            vertices[i * 20..(i + 1) * 20].copy_from_slice(bytemuck::cast_slice(&data));
        }
        s.frame = frame;
        Ok(json!({"batches":s.geometry.batches.len(),"vertices":s.geometry.vertices.len(),
            "parameterBytes":pb,"vertexBytes":vb,
            "batchStrideBytes":128,"vertexStrideBytes":20,
            "videoLayers":s.scene.layers.iter().filter_map(|l|l.video.as_ref().map(|v|json!({"object":l.id,"asset":v.asset,
                "texture_slot":-(l.order as i64+1),"source_time_us":v.source_time_us}))).collect::<Vec<_>>()}))
    }
}