//! Preview/observer controls, Surface binding and profiling JNI entry points.
use super::*;

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_configureMemory(
    mut env: JNIEnv, _class: JClass, id: jlong, total_mem: jlong, guarded: jboolean,
) -> jstring {
    string_result(&mut env, || with_session(id, |s| {
        if total_mem < 0 { return Err("invalid device physical memory".into()); }
        let budget = aem_render::resource_policy::scratch_budget(total_mem as u64, guarded != 0);
        s.effects.set_scratch_budget(budget)?;
        if let Some(g) = &mut s.graphics {
            g.renderer.set_scratch_budget(budget).map_err(|e| e.to_string())?;
        }
        if let Some(renderer) = &mut s.editor_renderer {
            renderer.set_scratch_budget(budget).map_err(|e| e.to_string())?;
        }
        Ok(json!({"scratchBudgetBytes":budget,"policyVersion":1}))
    }))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_seek(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
                return Err("invalid frame".into());
            }
            s.frame = frame;
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_observe(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    enabled: jboolean,
    az: jdouble,
    el: jdouble,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.observing = enabled != 0;
            s.view_revision += 1;
            if s.observer.view == aem_core::ObservationView::Free {
                s.observer
                    .orbit(az as f32, el as f32)
                    .map_err(|e| e.to_string())?;
            } else {
                s.observer
                    .pan(
                        az as f32 * 4.0,
                        el as f32 * 4.0,
                        s.engine.project().width,
                        s.engine.project().height,
                    )
                    .map_err(|e| e.to_string())?;
            }
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_navigate(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    dx: jdouble,
    dy: jdouble,
    zoom: jdouble,
    multi: jboolean,
    width: jint,
    height: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !s.observing || width <= 0 || height <= 0 || !dx.is_finite() || !dy.is_finite() {
                return Err("invalid observation navigation".into());
            }
            s.observer.zoom(zoom as f32).map_err(|e| e.to_string())?;
            if multi != 0 || s.observer.view != aem_core::ObservationView::Free {
                s.sample()?;
                let target = s.observer.camera.target.value;
                let delta = s
                    .scene
                    .screen_translation(
                        target,
                        [dx as f32, dy as f32],
                        [width as u32, height as u32],
                    )
                    .map_err(|e| e.to_string())?;
                s.observer.camera.target.value = std::array::from_fn(|i| target[i] - delta[i]);
            } else {
                s.observer
                    .orbit(dx as f32 * 0.18, dy as f32 * 0.18)
                    .map_err(|e| e.to_string())?;
            }
            s.view_revision += 1;
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_view(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            s.observing = kind != 0;
            s.view_revision += 1;
            s.observer.view = match kind {
                0 | 1 => aem_core::ObservationView::Free,
                2 => aem_core::ObservationView::Top,
                3 => aem_core::ObservationView::Side,
                _ => return Err("invalid observation view".into()),
            };
            s.sample()?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_surface(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    surface: JObject,
    width: jint,
    height: jint,
) -> jstring {
    let window = if surface.is_null() {
        None
    } else {
        unsafe { NativeWindow::from_surface(env.get_native_interface(), surface.as_raw()) }
    };
    string_result(&mut env, || {
        with_session(id, |s| {
            if let Some(window) = window {
                if width <= 0 || height <= 0 {
                    return Err("surface dimensions must be positive".into());
                }
                s.attach(window, width as u32, height as u32)?;
            } else {
                s.detach();
            }
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_previewInfo(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || with_session(id, |s| Ok(s.preview_info())))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_previewMode(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    mode: jint,
    thermal: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            let mode = PreviewMode::from_id(mode).ok_or("invalid preview mode")?;
            let previous_tier = s.preview.tier();
            if s.preview.mode != mode {
                s.preview.set_mode(mode);
            }
            s.preview.set_thermal(thermal);
            if previous_tier != s.preview.tier() {
                s.view_revision += 1;
            }
            Ok(s.preview_info())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_startProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    limit: jint,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if !(1..=65_536).contains(&limit) {
                return Err("invalid frame recording limit".into());
            }
            let g = s.graphics.as_mut().ok_or("no preview surface")?;
            g.renderer.device.poll(wgpu::Maintain::Wait);
            g.timer = GpuTimer::new(&g.renderer.device, &g.renderer.queue);
            s.recorder = Some(FrameRecorder::new(s.presented + 1, limit as usize));
            Ok(s.preview_info())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_stopProfiling(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            if let Some(g) = s.graphics.as_mut() {
                g.renderer.device.poll(wgpu::Maintain::Wait);
                if let Some(timer) = &mut g.timer {
                    for t in timer.collect().into_iter().flatten() {
                        if let Some(r) = &mut s.recorder {
                            r.timing(t);
                        }
                    }
                }
            }
            let r = s.recorder.as_ref().ok_or("frame recording is not active")?;
            let metadata = json!({"preview":s.preview_info(),"projectWidth":s.engine.project().width,"projectHeight":s.engine.project().height,
            "projectFps":s.engine.project().fps,"projectFrames":s.engine.project().frames,"layerCount":s.engine.project().layers.len(),"surfaceEpoch":s.surface_epoch,
            "graphics":s.graphics.as_ref().map(|g|json!({"surfaceWidth":g.config.width,"surfaceHeight":g.config.height,"renderWidth":g.scratch.width,"renderHeight":g.scratch.height,
                "adapter":g.renderer.adapter_info.name,"driver":g.renderer.adapter_info.driver,"driverInfo":g.renderer.adapter_info.driver_info,"backend":format!("{:?}",g.renderer.adapter_info.backend),
                "assetTextureBytes":g.renderer.texture_bytes(),"renderTargetBytes":g.scratch.texture_bytes(),"previewImageReadbackBytes":0,
                "timestampReadbackBytesPerSample":if g.timer.is_some(){32}else{0},"timingSkipped":g.timer.as_ref().map(|t|t.skipped),"timingErrors":g.timer.as_ref().map(|t|t.errors)}))});
            let folder = s.root.join("exports");
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let path = folder.join("preview-performance.json");
            std::fs::write(
                &path,
                serde_json::to_vec(&r.report(metadata)).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            s.recorder = None;
            Ok(json!({"file":path.to_string_lossy()}))
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_injectGraphicsFault(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        #[cfg(not(feature = "diagnostics"))]
        {
            let _ = (id, kind);
            Err("GPU fault injection is not included in this build".into())
        }
        #[cfg(feature = "diagnostics")]
        {
            with_session(id, |s| {
                let g = s.graphics.as_ref().ok_or("no active GPU surface")?;
                match kind {
                    0 => {
                        g.renderer.device.destroy();
                        g.renderer.device.poll(wgpu::Maintain::Wait);
                    }
                    1 => {
                        let _ = g.renderer.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("diagnostic invalid mapped size"),
                            size: 1,
                            usage: wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: true,
                        });
                    }
                    _ => return Err("unknown GPU diagnostic fault".into()),
                }
                g.renderer.device.poll(wgpu::Maintain::Poll);
                Ok(json!({"diagnostics":true,"kind":kind,"gpuError":g.renderer.gpu_error()}))
            })
        }
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_render(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
    frame: jdouble,
) -> jboolean {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_session(id, |s| {
            // A queued UI frame may precede playback start or belong to the
            // previous composition. Skip it before touching sampled state;
            // invalid timing is not a GPU/device failure.
            if !frame.is_finite() || frame < 0.0 || frame >= f64::from(s.engine.project().frames) {
                return Ok(false);
            }
            match s.render(frame) {
                Ok(rendered) => Ok(rendered),
                Err(error) => {
                    s.last_error = Some(error);
                    Ok(false)
                }
            }
        })
    }));
    u8::from(
        result
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or(false),
    )
}
