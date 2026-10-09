//! Geometry queries and packed parameter/vertex output JNI entry points.
use super::*;

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_GeometryBridge_hitCandidates(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    x: jdouble,
    y: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.sample()?;
            let mut picking=s.scene.clone();
            for layer in &mut picking.layers {
                *layer=aem_core::selection_geometry::picking_layer(layer,&s.scene,&s.effects.registry);
            }
            let candidates = picking
                .hit_candidates([x as f32, y as f32])
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"candidates":candidates,"coordinates":"composition_pixels","selection":"geometry_bounds"}),
            )
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_GeometryBridge_sampleGeometryInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
    parameters: JByteBuffer,
    vertices: JByteBuffer,
) -> jstring {
    let BufferAccess {
        address: parameter_address,
        capacity: parameter_capacity,
    } = BufferAccess::capture(&env, &parameters);
    let BufferAccess {
        address: vertex_address,
        capacity: vertex_capacity,
    } = BufferAccess::capture(&env, &vertices);
    let parameter_readonly = env
        .call_method(&parameters, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z());
    let vertex_readonly = env
        .call_method(&vertices, "isReadOnly", "()Z", &[])
        .and_then(|v| v.z());
    string_result(&mut env, || {
        if parameter_readonly.map_err(|e| e.to_string())?
            || vertex_readonly.map_err(|e| e.to_string())?
        {
            return Err("geometry buffers must be writable".into());
        }
        let pa = parameter_address.map_err(|e| e.to_string())?;
        let pc = parameter_capacity.map_err(|e| e.to_string())?;
        let va = vertex_address.map_err(|e| e.to_string())?;
        let vc = vertex_capacity.map_err(|e| e.to_string())?;
        with_session(id, |s| {
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
            if pc < pb || vc < vb {
                return Err(format!(
                    "geometry buffers too small: need {pb} parameter bytes and {vb} vertex bytes"
                ));
            }
            let pend = (pa as usize)
                .checked_add(pb)
                .ok_or("parameter address overflow")?;
            let vend = (va as usize)
                .checked_add(vb)
                .ok_or("vertex address overflow")?;
            if pb > 0 && vb > 0 && (pa as usize) < vend && (va as usize) < pend {
                return Err("geometry buffers must not overlap".into());
            }
            let parameters = unsafe { buffers::output_bytes(pa, pb) };
            let vertices = unsafe { buffers::output_bytes(va, vb) };
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
            Ok(
                json!({"batches":s.geometry.batches.len(),"vertices":s.geometry.vertices.len(),
                "parameterBytes":pb,"vertexBytes":vb,"batchStrideBytes":128,"vertexStrideBytes":20,
                "videoLayers":s.scene.layers.iter().filter_map(|l|l.video.as_ref().map(|v|json!({"object":l.id,"asset":v.asset,
                    "texture_slot":-(l.order as i64+1),"source_time_us":v.source_time_us}))).collect::<Vec<_>>()}),
            )
        })
    })
}
