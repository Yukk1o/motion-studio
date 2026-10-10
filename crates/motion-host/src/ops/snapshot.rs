//! Host-facing state projection.
//!
//! This snapshot is the UI's single source of truth and is identical on every
//! platform: a host that cannot satisfy a capability must report it as absent
//! here rather than silently omitting a field, which is what keeps the desktop
//! and Android feature surfaces comparable.
use crate::session::Session;
use motion_render::PreviewMode;
use serde_json::{json, Value};

impl Session {
    pub fn preview_info(&self) -> Value {
        let tier = self.preview.tier();
        let p = self.engine.project();
        let (sw, sh) = self
            .graphics
            .as_ref()
            .map_or((p.width, p.height), |g| (g.config.width, g.config.height));
        let (width, height) = self.preview.render_dimensions(p.width, p.height, sw, sh);
        json!({"mode":self.preview.mode.name(),"tier":tier.name(),"width":width,"height":height,"fps":tier.fps(),
            "scratchBudgetBytes":self.effects.scratch_budget(),"memoryPolicyVersion":1,
            "effectResolution":if self.preview.mode == PreviewMode::High {"full_layer"} else {"projected_2d"},
            "imageResolution":if self.preview.mode == PreviewMode::High {"original"} else {"proxy_max_2048"},
            "imageDecodes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_decodes),
            "imageProxyCacheHits":self.graphics.as_ref().map_or(0,|g|g.renderer.image_proxy_cache_hits),
            "imageUploadBytes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_upload_bytes),
            "imageMemoryCacheHits":self.graphics.as_ref().map_or(0,|g|g.renderer.image_memory_cache_hits),
            "imageIdleBytes":self.graphics.as_ref().map_or(0,|g|g.renderer.image_idle_bytes()),
            "imageIdleBudgetBytes":motion_render::image_resources::IDLE_TEXTURE_BYTES,
            "imagePrefetches":self.graphics.as_ref().map_or(0,|g|g.renderer.image_prefetches),
            "imagePrefetchSeconds":motion_render::image_resources::PREFETCH_SECONDS,
            "effectPlanBuilds":self.graphics.as_ref().map_or(0,|g|g.renderer.effect_plan_builds()),
            "effectPlanCacheHits":self.graphics.as_ref().map_or(0,|g|g.renderer.effect_plan_cache_hits()),
            "surfaceBounded":self.preview.mode != PreviewMode::High,
            "gpuTimingActive":self.graphics.as_ref().is_some_and(|g|g.timer.is_some()),
            "profiling":self.recorder.is_some(),
            "gpuTimestampSupported":self.graphics.as_ref().is_some_and(|g|g.renderer.device.features().contains(wgpu::Features::TIMESTAMP_QUERY)),
            "video":self.video_info()})
    }

    pub fn video_info(&self) -> Value {
        let mut info = self.video_frames.metrics();
        info["renderAttempts"] = json!(self.render_attempts);
        info["pendingAttempts"] = json!(self.video_pending_attempts);
        info["lastPrepareUs"] = json!(self.video_prepare_us);
        info["lastUploadUs"] = json!(self.video_upload_us);
        if let Some(g) = &self.graphics {
            info["uploadBytes"] = json!(g.renderer.video_upload_bytes);
            info["uploads"] = json!(g.renderer.video_uploads);
            info["gpuConversions"] = json!(g.renderer.video_gpu_conversions);
        }
        info
    }

    /// Platform-specific video capability declaration.
    ///
    /// Both hosts fill this from the same template; only `decoder`, the container
    /// list and the encoder description differ, and they differ because the
    /// underlying platform does, not because the host forgot to implement it.
    fn video_capabilities(&self) -> Value {
        json!({
            "container":"MP4","codec":"H.264 baseline/main/high, 8-bit 4:2:0 SDR",
            "containers":["MP4","MOV","3GP","Matroska","WebM"],
            "codecs":["H.264","H.265 Main","VP8","VP9 profile 0"],
            "profile":"8-bit 4:2:0 SDR","device_query":"media_capabilities",
            "max_pixels":motion_core::MAX_VIDEO_PIXELS,"max_dimension":motion_core::MAX_VIDEO_DIMENSION,
            "max_fps":motion_core::MAX_VIDEO_FPS,"max_index_frames":motion_core::MAX_VIDEO_FRAMES,
            "preserves_source_aspect_ratio":true,"arbitrary_aspect_ratio":true,"square_pixels_only":true,
            "input_is_independent_of_composition":true,"max_duration_seconds":3600,
            "async_frames":true,"frame_format":"rgba8",
            "decoder":self.platform().name(),
            "max_decoders":4,"default_with_audio":true,"frozen_source_frames":true,
            "legacy_gles_export_integrated":true
        })
    }

    pub fn snapshot(&self) -> Value {
        let original = self.engine.project();
        let p = self.scene.sampled_project(original);
        let f = self.frame;
        let camera = json!({"position":p.camera.position_at(f),"target":p.camera.target.sample(f),
            "fov":p.camera.fov.sample(f).clamp(10.0,120.0),"roll":p.camera.roll.sample(f),
            "radius":p.camera.radius.sample(f).clamp(1.0,10_000_000.0),
            "azimuth":p.camera.azimuth.sample(f),"elevation":p.camera.elevation.sample(f).clamp(-89.0,89.0)});
        let layers: Vec<_> = p
            .layers
            .iter()
            .map(|l| {
                let local = l.local_frame(f);
                json!({"id":l.id,"position":l.transform.position.sample(local),
            "rotation":l.transform.rotation.sample(local),"scale":l.transform.scale.sample(local),
            "opacity":l.transform.opacity.sample(local).clamp(0.0,1.0),"active":l.active(f,p.frames),"three_d":l.three_d})
            })
            .collect();
        let mut projected: Vec<_> = self
            .scene
            .layers
            .iter()
            .filter_map(|layer| {
                let corners = motion_render::selection_geometry::polygon(layer, &self.scene, &self.effects.registry)?;
                let anchor = p
                    .layers
                    .iter()
                    .find(|l| l.id == layer.id)
                    .and_then(|l| self.scene.project_node(l.id));
                Some(json!({"id":layer.id,"anchor":anchor,"corners":corners,"selection_space":"effect_output"}))
            })
            .collect();
        for layer in p
            .layers
            .iter()
            .filter(|l| matches!(l.content, motion_core::Content::Null) && l.active(f, p.frames))
        {
            if let Some(point) = self.scene.project_node(layer.id) {
                let [x, y, _] = point;
                if x.is_finite() && y.is_finite() {
                    projected.push(json!({"id":layer.id,"anchor":point,"null":true,
                    "corners":[[x-12.0,y-12.0],[x+12.0,y-12.0],[x+12.0,y+12.0],[x-12.0,y+12.0]]}));
                }
            }
        }
        let mask_layers:Vec<_>=self.scene.layers.iter().filter_map(|layer| {
            let stored=p.layers.iter().find(|l|l.id==layer.id)?;
            if stored.masks.is_empty(){return None;}
            let local=stored.local_frame(f);
            let masks:Vec<_>=stored.masks.iter().map(|m|{
                let nodes:Vec<_>=m.path.nodes.iter().map(|n|json!({"id":n.id,"geometry":n.geometry.sample(local)})).collect();
                json!({"id":m.id,"opacity":m.opacity.sample(local).clamp(0.,100.),
                    "feather":m.feather.sample(local).map(|v|v.clamp(0.,motion_core::masks::MAX_MASK_DISTANCE)),
                    "expansion":m.expansion.sample(local),
                    "path":{"id":m.path.id,"closed":m.path.closed,"nodes":nodes}})
            }).collect();
            Some(json!({"id":layer.id,"mvp":(layer.view_projection*layer.model).to_cols_array(),"masks":masks}))
        }).collect();
        let vector_layers: Vec<_> = self.scene.layers.iter().filter_map(|layer| {
            let vector=layer.vector.as_ref()?;
            let stored=p.layers.iter().find(|l|l.id==layer.id).and_then(|l|match &l.content {
                motion_core::Content::Vector{vector}=>Some(&vector.source),_=>None,
            });
            let paths:Vec<_>=vector.paths.iter().enumerate().map(|(i,path)| {
                let original_path=match stored {Some(motion_core::vector::VectorSource::Paths{paths})=>paths.get(i),_=>None};
                let nodes:Vec<_>=path.nodes.iter().enumerate().map(|(j,geometry)|json!({"id":original_path.and_then(|p|p.nodes.get(j)).map_or(j as u64+1,|n|n.id),"geometry":geometry})).collect();
                json!({"id":original_path.map_or(i as u64+1,|p|p.id),"closed":path.closed,"nodes":nodes})
            }).collect();
            let parameters=match stored {
                Some(motion_core::vector::VectorSource::Shape{parameters,..})=>{
                    let offset=p.layers.iter().find(|l|l.id==layer.id).map_or(0,|l|l.clip(p.frames).offset_frame);
                    parameters.iter().map(|(name,track)|(name.clone(),json!(track.sample(f-f64::from(offset))))).collect::<serde_json::Map<_,_>>()
                },_=>serde_json::Map::new(),
            };
            Some(json!({"id":layer.id,"canvas_size":layer.source_size,"source_rect":layer.source_rect,
                "mvp":(layer.view_projection*layer.model).to_cols_array(),"paths":paths,"parameters":parameters,
                "fill":vector.fill,"stroke":vector.stroke.map(|s|json!({"color":s.0,"width":s.1,"cap":s.2,"join":s.3,"miter_limit":s.4}))}))
        }).collect();
        let camera_properties: Vec<&str> = if !p.camera.created {
            vec![]
        } else if p.camera.mode == motion_core::CameraMode::Position {
            vec!["position", "target"]
        } else {
            vec!["target"]
        };
        json!({"project":original,"root":self.root.to_string_lossy(),"frame":f,"revision":self.engine.revision(),"canUndo":self.engine.can_undo(),
            "main_composition":"comp-main","composition":p.composition_id,"compositions":p.composition_list(),
            "has_audio":p.audio_voices().is_ok_and(|v|!v.is_empty()),
            "composition_context":self.composition_context_snapshot(),
            "vector_layers":vector_layers,"mask_layers":mask_layers,
            "platform":{"name":self.platform().name(),"graphics":"wgpu"},
            "capabilities":{"spatial_paths":{"supported":true,"version":1,"query":"position_path","command":"spatial","parameter_scope":true,"max_visible_keys":512,"max_trace_samples":257,"coordinates":"property_local","temporal_easing":"independent","separated_axes":"trace_only"},"adjustment_layers":{"supported":true,"command":"add_adjustment","composite":"lower_layers","mask":"transformed_rectangle","background":"excluded","three_d":false},"vector_drawing":{"supported":true,"protocol":1,"command":"vector","coordinates":"centered_canvas_pixels_y_down","max_paths":motion_core::vector::MAX_PATHS,"max_nodes":motion_core::vector::MAX_NODES,"fill_rules":["non_zero","even_odd"],"stroke_caps":["butt","round","square"],"stroke_joins":["miter","round","bevel"],"shape_catalog":motion_core::vector::shape_catalog()},"scene_effects":{"sdk_version":motion_effects::SDK_VERSION,"plugin_editor_protocol":1,"max_particles_per_effect":20000,"max_sprites_per_frame":65536,"occlusion":"source_alpha_planes","simulation":"analytic_world_birth_or_legacy_local_space","particle_birth_history":true,"particle_history_expressions":false,"particle_rate_animation":false},"native_plugin_ui":{"supported":true,"protocol":1,"slots":["preview","timeline","parameters","layer_source","image_sprite","seed","transform","note"],"preview":"shared_wgpu_surface","timeline":"shared_composition_clock"},"property_expressions":{"supported":true,"profile":motion_core::EXPRESSION_PROFILE,"engine":"QuickJS-NG","source_max_bytes":8192,"max_expressions":motion_core::MAX_EXPRESSIONS,"cross_property_references":false,"opacity_unit":"percent"},"layer_clips":true,"layer_3d":{"supported":true,"default":false,"activation":"explicit","command":"set_layer_3d"},
                "planar_intersections":{"supported":true,"method":"bsp","geometry_api":"sampleGeometryInto","max_batches":8192,"max_vertices":65536},
                "separate_dimensions":{"supported":true,"activation":"explicit",
                "layer_properties":["position","rotation","scale"],"camera_properties":camera_properties,"axes":["x","y","z"]},
                "composition":{"fps_range":[1,motion_core::MAX_COMPOSITION_FPS],"fps_presets":[24,25,30,50,60,90,120,144,240],"fps_type":"integer"},
                "project_package":crate::ops::media::package_limits(),"multiple_compositions":true,"precompose":true,"composition_api":{"version":1,"project_format":9,"max_compositions":motion_core::composition::MAX_COMPOSITIONS,"max_depth":motion_core::composition::MAX_COMPOSITION_DEPTH,"depth_includes_root":true,"max_instances":motion_core::composition::MAX_COMPOSITION_INSTANCES,"max_render_instances":motion_core::composition::MAX_RENDER_COMPOSITION_INSTANCES,"reference_3d":true,"collapse_transformations":false,"precompose_modes":["move_all_attributes"],"precompose_range":["composition"],"precompose_contiguous":true,"precompose_3d":false,"history_scope":"project"},"video_import":true,"audio_import":true,"model_import":false,"prerender":false,
                "layer_masks":{"version":1,"supported":true,"space":"source_pixels_y_down","stage":"before_image_effects","max_masks":16,"max_nodes":2048,"modes":["none","add","subtract","intersect","lighten","darken","difference"],"animated":["path","opacity","feather","expansion"],"variable_feather":false,"adjustment_masks":false},
                "video":self.video_capabilities(),
                "effect_image_inputs":{"supported":true,"sdk":6,"command":"image_input","sources":["package","asset","layer","empty"],"stages":["source","effects"],"default_stage":"effects","hidden_source":true,"feedback":false,"gpu_readbacks":0},
                "font_rasterization":{"supported":true,"system_catalogue":"NativeBridge.systemFonts","import":"NativeBridge.importFont","operations":["font_catalogue","font_import","font_atlas","font_text"],"formats":["ttf","otf","ttc"],"max_font_bytes":motion_media::fonts::MAX_FONT_BYTES,"glyph_cache_bytes":motion_media::fonts::GLYPH_CACHE_BYTES,"shaping":"ltr_kerning_wrap","variable_axes":false},
                "audio":{"supported_formats":["M4A/AAC-LC/ALAC","MP3","FLAC","Ogg/Vorbis/Opus","ADTS/AAC","WAV/PCM8/16/24/32/float","AIFF"],"sample_rates":[8000,11025,12000,16000,22050,24000,32000,44100,48000,88200,96000,176400,192000],"sample_rate_range":[8000,192000],"channels":[1,2],"device_query":"media_capabilities",
                "output_rate":48000,"output_channels":2,"pcm":"f32le_interleaved","waveform_bucket_us":10000,
                "source_limit_bytes":motion_core::storage::MAX_MEDIA_ASSET,"source_duration_limit_seconds":3600,
                "pcm_block_limit_frames":motion_media::MAX_BLOCK_FRAMES,"async_import":true,"ui_playback_integrated":true,"mp4_audio_mux_integrated":true}},
            "canRedo":self.engine.can_redo(),"observing":self.observing,"sampledCamera":camera,
            "sampledLayers":layers,"timeline_layers":original.timeline_layers(f),
            "timeline_camera":{"position":original.camera.position.timeline(0),"target":original.camera.target.timeline(0)},
            "projectedLayers":projected,"presented":self.presented,"cpuPrepareUs":self.last_cpu_us,
            "renderError":self.last_error,
            "effectErrors":self.graphics.as_ref().map(|g|&g.renderer.effect_diagnostics),
            "sampledEffects":self.scene.effects.iter().map(|e|json!({"layer":e.layer,"instance":e.instance,
                "values":e.param_ids.iter().enumerate().map(|(i,id)|(id.clone(),json!(e.values[i]))).collect::<serde_json::Map<String,Value>>(),
                "curve_lut":e.lut.map(|i|&self.scene.curve_luts[i][..])})).collect::<Vec<_>>(),
            "lastPresentedFrame":self.last_presented_frame,
            "lastPresentedRevision":self.last_presented_revision,"viewRevision":self.view_revision,
            "lastPresentedViewRevision":self.last_presented_view_revision,"surfaceEpoch":self.surface_epoch,
            "diagnosticsEnabled":cfg!(feature="diagnostics"),
            "observationView":match self.observer.view {motion_core::ObservationView::Free=>"free",motion_core::ObservationView::Top=>"top",motion_core::ObservationView::Side=>"side"},
            "preview":self.preview_info(),
            "graphics":self.graphics.as_ref().map(|g|json!({"width":g.config.width,"height":g.config.height,
                "renderWidth":g.scratch.width,"renderHeight":g.scratch.height,
                "renderTargetBytes":g.scratch.texture_bytes(),"previewImageReadbackBytes":0,
                "adapter":g.renderer.adapter_info.name,
                "backend":format!("{:?}",g.renderer.adapter_info.backend),
                "textureBytes":g.renderer.texture_bytes()}))})
    }
}