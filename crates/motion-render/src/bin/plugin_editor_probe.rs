//! JSON-lines desktop driver for the packaged editor's browser integration tests.
//! It is not exposed to plugin pages or included in the Android bridge.
use motion_core::{
    plugin_editor::{EditorRequest, PluginEditorSession},
    Command, EffectAction, EffectInstance, Engine, Layer, Project, Scene,
};
use motion_effects::{builtin, Registry};
use motion_render::Renderer;
use image::ImageEncoder;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

struct Probe {
    renderer: Renderer,
    registry: Registry,
    engine: Option<Engine>,
    editor: Option<PluginEditorSession>,
    frame: u32,
}
impl Probe {
    fn call(&mut self, input: Value) -> Result<Value, Box<dyn std::error::Error>> {
        if input["op"] == "open" {
            let package = builtin::scene_package()?;
            let id = input["effect"].as_str().ok_or("effect required")?;
            let definition = package
                .manifest
                .effects
                .iter()
                .find(|d| d.id == id)
                .ok_or("unknown effect")?;
            let mut p = Project::new(640, 360, 30, 180)?;
            p.background = [0., 0., 0., 1.];
            p.layers.push(Layer::solid(
                1,
                "效果图层",
                [640., 360.],
                [320., 180., 0.],
                [0.; 4],
            ));
            let mut light = Layer::solid(2, "光源 · Null", [1., 1.], [220., 120., 0.], [0.; 4]);
            light.content = motion_core::Content::Null;
            p.layers.push(light);
            let mut engine = Engine::new(p)?;
            let mut effect = EffectInstance::new(
                1,
                &package.manifest.id,
                &package.manifest.version,
                &package.hash,
                definition,
                [640., 360.],
            );
            if let Some(extent) = effect.params.get_mut("extent") {
                extent.track.value = [600., 340., 200., 0.];
            }
            engine.apply(Command::Effect {
                object: 1,
                action: EffectAction::Insert { instance: effect },
            })?;
            self.editor = Some(PluginEditorSession::open(
                engine.project(),
                &self.registry,
                1,
                1,
            )?);
            self.engine = Some(engine);
            self.frame = 60;
            return Ok(
                json!({"protocol":1,"token":"probe-editor","definition":definition,
                "state":self.editor.as_ref().unwrap().state(self.engine.as_ref().unwrap(), self.frame)?}),
            );
        }
        let engine = self.engine.as_mut().ok_or("open an editor first")?;
        let editor = self.editor.as_mut().ok_or("open an editor first")?;
        if input["op"] == "host" {
            match input["action"].as_str().ok_or("host action required")? {
                "undo" => {
                    engine.undo()?;
                }
                "redo" => {
                    engine.redo()?;
                }
                "lock" => {
                    engine.apply(Command::Flags {
                        object: 1,
                        visible: true,
                        locked: input["locked"].as_bool().ok_or("locked required")?,
                    })?;
                }
                "seek" => {
                    self.frame = input["frame"]
                        .as_u64()
                        .ok_or("frame required")?
                        .try_into()?;
                }
                "snapshot" => {
                    return Ok(serde_json::to_value(engine.project())?);
                }
                _ => return Err("unknown host action".into()),
            }
            return Ok(editor.state(engine, self.frame)?);
        }
        let message = input.get("message").ok_or("message required")?;
        if message["op"] == "preview" {
            let width: u32 = message["width"]
                .as_u64()
                .ok_or("width required")?
                .try_into()?;
            let height: u32 = message["height"]
                .as_u64()
                .ok_or("height required")?
                .try_into()?;
            if !(1..=512).contains(&width) || !(1..=512).contains(&height) {
                return Err("invalid preview size".into());
            }
            let mut scene = Scene::new(engine.project());
            scene.sample(engine.project(), self.frame.into(), None)?;
            let target = self.renderer.capture_target(width, height)?;
            let (rgba, stats) = self.renderer.capture(&scene, &target)?;
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png).write_image(
                &rgba,
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )?;
            return Ok(
                json!({"width":width,"height":height,"png_bytes":png,"revision":engine.revision(),"frame":self.frame,
                "instances":{"alive":stats.particles_alive,"visible":stats.particles_visible,"culled":stats.particles_culled,"upload_bytes":stats.instance_upload_bytes}}),
            );
        }
        let request: EditorRequest = serde_json::from_value(message.clone())?;
        Ok(editor.request(engine, self.frame, request)?)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut probe = Probe {
        renderer: pollster::block_on(Renderer::headless())?,
        registry: Registry::new_with_builtins()?,
        engine: None,
        editor: None,
        frame: 60,
    };
    for line in io::stdin().lock().lines() {
        let reply = match serde_json::from_str::<Value>(&line?)
            .map_err(|e| e.to_string())
            .and_then(|input| probe.call(input).map_err(|e| e.to_string()))
        {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        println!("{reply}");
        io::stdout().flush()?;
    }
    Ok(())
}
