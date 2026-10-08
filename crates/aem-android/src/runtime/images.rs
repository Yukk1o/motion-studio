//! Image inspection, bounded preview proxies and full-resolution direct-buffer transfer.
use super::*;

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_imageInfo(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jstring {
    let path = read_string(&mut env, &path);
    string_result(&mut env, || {
        let (width, height, format, bytes) =
            aem_render::image_resources::inspect(std::path::Path::new(&path?))?;
        Ok(
            json!({"version":1,"width":width,"height":height,"format":format!("{format:?}"),"bytes":bytes,
            "maxEncodedBytes":aem_render::image_resources::MAX_ENCODED_BYTES,"previewMaxEdge":aem_render::image_resources::MAX_PREVIEW_EDGE}),
        )
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_prepareImage(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
    path: JString,
) -> jstring {
    let root = read_string(&mut env, &root);
    let path = read_string(&mut env, &path);
    string_result(&mut env, || {
        let root = PathBuf::from(root?);
        let path = path?;
        aem_core::storage::validate_relative_path(&path).map_err(|e| e.to_string())?;
        let (width, height, format, bytes) =
            aem_render::image_resources::inspect(&root.join(&path))?;
        let asset = aem_core::Asset {
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
        Ok(
            json!({"version":1,"path":path,"width":width,"height":height,"format":format!("{format:?}"),
            "bytes":bytes,"validated":true,"proxyWidth":proxy.width,"proxyHeight":proxy.height,"proxyCached":proxy.cached}),
        )
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_assetPixelsInto(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    asset: jlong,
    buffer: JByteBuffer,
) -> jstring {
    let destination = (|| -> Result<_> {
        if env
            .call_method(&buffer, "isReadOnly", "()Z", &[])
            .and_then(|v| v.z())
            .map_err(|e| e.to_string())?
        {
            return Err("image output buffer is read-only".into());
        }
        let (address, capacity) = BufferAccess::capacity_first(&env, &buffer)?;
        Ok((capacity, address))
    })();
    string_result(&mut env, || {
        let (capacity, address) = destination?;
        with_session(id, |s| {
            let a = s
                .engine
                .project()
                .assets
                .iter()
                .find(|a| a.id == asset as u64)
                .ok_or("asset not found")?;
            let bytes = u64::from(a.width) * u64::from(a.height) * 4;
            if address.is_null()
                || bytes > aem_render::image_resources::MAX_DECODED_BYTES
                || (capacity as u64) < bytes
            {
                return Err("image direct buffer is too small or exceeds 128 MiB".into());
            }
            let source = aem_render::image_resources::Source::new(&s.root, a)?;
            // JNI owns this direct buffer for the duration of this synchronous call.
            let output = unsafe { buffers::output_bytes(address, bytes as usize) };
            aem_render::image_resources::decode_into(
                &source,
                aem_render::image_resources::Resolution::Full,
                output,
            )?;
            Ok(
                json!({"version":1,"asset":a.id,"width":a.width,"height":a.height,"bytes":bytes,"resolution":"original"}),
            )
        })
    })
}
