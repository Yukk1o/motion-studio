//! Shared font and raster API; called on the session's existing worker thread.
use super::*;
use aem_media::fonts::{FontInfo, FontStore, Raster};

impl Session {
    fn fonts(&mut self) -> Result<&mut FontStore> {
        if self.font_store.is_none() {
            self.font_store = Some(FontStore::new(&self.root.join("assets/fonts"))?);
        }
        Ok(self.font_store.as_mut().unwrap())
    }
    fn font_commands(&self, info: &FontInfo) -> Vec<Command> {
        if self.engine.project().fonts.iter().any(|f| f.id == info.id) {
            return vec![];
        }
        let hash = info.id.split('-').next().unwrap();
        vec![Command::RegisterFontAsset {
            asset: aem_core::FontAsset {
                id: info.id.clone(),
                path: format!("assets/fonts/{hash}.ttf"),
                name: info.name.clone(),
                face_index: info.face_index,
                license: info.license.clone(),
            },
        }]
    }
    fn raster_asset(&mut self, raster: &Raster) -> Result<(aem_core::Asset, Vec<Command>)> {
        let path = format!(
            "assets/font-raster-{}.png",
            aem_media::fonts::raster_key(raster)
        );
        if let Some(asset) = self.engine.project().assets.iter().find(|a| a.path == path) {
            return Ok((asset.clone(), vec![]));
        }
        aem_media::fonts::save_raster(&self.root, &path, raster)?;
        let p = self.engine.project();
        let id = p
            .assets
            .iter()
            .map(|a| a.id)
            .chain(p.audio_assets.iter().map(|a| a.id))
            .chain(p.video_assets.iter().map(|a| a.id))
            .max()
            .unwrap_or(0)
            + 1;
        let asset = aem_core::Asset {
            id,
            path,
            width: raster.width,
            height: raster.height,
        };
        Ok((asset.clone(), vec![Command::RegisterAsset { asset }]))
    }
    pub(super) fn font_request(&mut self, v: &Value) -> Result<Value> {
        let op = v["op"].as_str().ok_or("missing font operation")?;
        match op {
            "font_catalogue" => {
                let fonts = self.fonts()?.catalogue()?;
                return Ok(
                    json!({"fonts":fonts,"projectFonts":self.engine.project().fonts,"builtin":FontStore::builtin_id(),"systemCatalogue":"NativeBridge.systemFonts","maxFontBytes":aem_media::fonts::MAX_FONT_BYTES,
                    "sizes":[4,256],"formats":["ttf","otf","ttc"],"glyphCacheBytes":aem_media::fonts::GLYPH_CACHE_BYTES,"textLayout":"ltr_kerning_wrap","complexShaping":false}),
                );
            }
            "font_import" => {
                let path = v["path"].as_str().ok_or("missing staged font path")?;
                let license = v["license"]
                    .as_str()
                    .unwrap_or("User-provided font; original licensing applies");
                let index = u32::try_from(v["face_index"].as_u64().unwrap_or(0))
                    .map_err(|_| "invalid font face index")?;
                let info = self
                    .fonts()?
                    .import_face(&PathBuf::from(path), license, index)?;
                let commands = self.font_commands(&info);
                if !commands.is_empty() {
                    self.apply_command_batch(commands)?;
                }
                return Ok(json!({"font":info,"revision":self.engine.revision()}));
            }
            "font_atlas" | "font_text" => {
                let id = v["font"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(FontStore::builtin_id);
                let size = v["size"].as_f64().unwrap_or(32.) as f32;
                let info =
                    if let Some(font) = self.engine.project().fonts.iter().find(|f| f.id == id) {
                        FontInfo {
                            id: id.clone(),
                            name: font.name.clone(),
                            glyphs: 0,
                            source_bytes: 0,
                            license: font.license.clone(),
                            face_index: font.face_index,
                        }
                    } else {
                        self.fonts()?
                            .catalogue()?
                            .into_iter()
                            .find(|f| f.id == id)
                            .ok_or("font is not imported")?
                    };
                let (raster, layout) = if op == "font_atlas" {
                    let characters = v["characters"].as_str().unwrap_or(" .:-=+*#%@");
                    let atlas = self.fonts()?.ascii_atlas(&id, characters, size)?;
                    let layout = json!({"characters":atlas.characters,"cell":atlas.cell,"coverage":atlas.coverage});
                    (atlas.raster, layout)
                } else {
                    let text = v["text"].as_str().ok_or("missing text")?;
                    let width = u32::try_from(v["width"].as_u64().unwrap_or(1024))
                        .map_err(|_| "invalid text width")?;
                    (
                        self.fonts()?.raster_text(&id, text, size, width)?,
                        json!({"layout":"ltr_kerning_wrap","font":id}),
                    )
                };
                let (asset, mut commands) = self.raster_asset(&raster)?;
                commands.extend(self.font_commands(&info));
                if let Some(object) = v["object"].as_u64() {
                    if op != "font_atlas" {
                        return Err("font_text returns a reusable raster asset; layer creation uses the content API".into());
                    }
                    let instance = v["instance"].as_u64().ok_or("missing ASCII instance")?;
                    let layer = self
                        .engine
                        .project()
                        .layers
                        .iter()
                        .find(|l| l.id == object)
                        .ok_or("layer does not exist")?;
                    let effect = layer
                        .effects
                        .iter()
                        .find(|e| e.id == instance)
                        .ok_or("effect instance does not exist")?;
                    if effect.plugin != aem_effects::builtin::PLUGIN_ID
                        || effect.effect != "ascii"
                    {
                        return Err("font atlas target must be the official ASCII effect".into());
                    }
                    let count = layout["characters"].as_str().unwrap().chars().count() as f32;
                    let cell = layout["cell"].as_array().unwrap();
                    let aspect = cell[0].as_f64().unwrap() / cell[1].as_f64().unwrap();
                    commands.push(Command::Effect {
                        object,
                        action: aem_core::EffectAction::SetImageInput {
                            effect: instance,
                            input: Some(aem_core::EffectImageInput::Asset { asset: asset.id }),
                        },
                    });
                    for (param, value) in [("glyph_count", count), ("glyph_aspect", aspect as f32)]
                    {
                        commands.push(Command::Effect {
                            object,
                            action: aem_core::EffectAction::Set {
                                effect: instance,
                                param: param.into(),
                                frame: 0,
                                value: [value, 0., 0., 0.],
                            },
                        });
                    }
                }
                if !commands.is_empty() {
                    self.apply_command_batch(commands)?;
                }
                return Ok(
                    json!({"asset":asset,"font":id,"layout":layout,"revision":self.engine.revision(),"glyphCacheBytes":self.fonts()?.cache_bytes()}),
                );
            }
            _ => return Err("unknown font operation".into()),
        }
    }
}
