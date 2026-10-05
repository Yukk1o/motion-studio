//! Standalone backend probe for decoder/parity and resource acceptance.
use aem_core::{Engine, Project};
use aem_media::{AudioJobs, ImportOptions, Limits};
use std::{
    fs::File,
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: audio_probe SOURCE NEW_PROJECT_DIR".into());
    }
    let root = PathBuf::from(&args[2]);
    if root.exists() {
        return Err("project directory must be new".into());
    }
    std::fs::create_dir_all(&root)?;
    let mut engine = Engine::new(Project::new(256, 256, 30, 36000)?)?;
    let jobs = AudioJobs::new(root.clone(), Limits::default())?;
    let start = Instant::now();
    let file = File::open(&args[1])?;
    let bytes = file.metadata()?.len();
    jobs.start(
        Box::new(file),
        Some(bytes),
        ImportOptions {
            request_id: "acceptance".into(),
            at_frame: 0,
            name: "probe".into(),
            track: None,
        },
        false,
    )?;
    loop {
        let task = jobs.status("acceptance")?;
        if task.state != "running" {
            if task.state != "ready" {
                return Err(task.error.unwrap_or(task.state).into());
            }
            break;
        }
        if start.elapsed() > Duration::from_secs(600) {
            jobs.cancel("acceptance")?;
            return Err("audio import timeout".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut report = serde_json::to_value(jobs.commit("acceptance", &mut engine)?)?;
    report["elapsed_ms"] = serde_json::json!(start.elapsed().as_millis());
    if report["state"] != "succeeded" {
        return Err(report.to_string().into());
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
