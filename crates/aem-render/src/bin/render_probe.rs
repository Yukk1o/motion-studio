use aem_core::{Command, Engine, Project, Scene};
use aem_render::Renderer;
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "artifacts/render-probe".into()),
    );
    std::fs::create_dir_all(&root)?;
    let mut renderer = pollster::block_on(Renderer::headless())?;
    let mut engine = Engine::new(Project::demo())?;
    let mut scene = Scene::new(engine.project());
    let target = renderer.capture_target(360, 640)?;
    let mut records = Vec::new();
    for (label, movement) in [
        ("initial", None),
        (
            "dolly",
            Some(Command::Dolly {
                frame: 0,
                amount: 400.0,
            }),
        ),
        (
            "pan",
            Some(Command::Pan {
                frame: 0,
                x: 160.0,
                y: 0.0,
            }),
        ),
    ] {
        if let Some(command) = movement {
            engine.apply(command)?;
        }
        scene.sample(engine.project(), 0.0, None)?;
        let start = Instant::now();
        let (pixels, stats) = renderer.capture(&scene, &target)?;
        image::save_buffer(
            root.join(format!("{label}.png")),
            &pixels,
            target.width,
            target.height,
            image::ColorType::Rgba8,
        )?;
        records.push(serde_json::json!({"frame":label,"cpu_prepare_us":stats.cpu_prepare_us,
            "capture_ms":start.elapsed().as_secs_f64()*1000.0,"draw_calls":stats.draw_calls,
            "texture_bytes":stats.texture_bytes,"parameter_upload_bytes":stats.parameter_upload_bytes}));
    }
    let report = serde_json::json!({"adapter":renderer.adapter_info.name,
        "backend":format!("{:?}",renderer.adapter_info.backend),"environment":"Windows desktop GPU; not Android acceptance",
        "frames":records});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
