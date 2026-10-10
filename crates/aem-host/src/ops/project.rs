//! Project and session lifecycle.
use super::stamp;
use crate::platform::Platform;
use crate::session::Result;
use aem_core::Project;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// Directory names the project browser is allowed to open.
fn project_name_allowed(name: &str, current: &str) -> bool {
    name == current
        || name == "default"
        || ["project-", "import-"].iter().any(|prefix| {
            name.strip_prefix(*prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

pub fn create(root: PathBuf, project_text: &str, platform: Arc<dyn Platform>) -> Result<i64> {
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let project = if project_text.is_empty() {
        if root.join("project.json").exists() {
            aem_core::storage::load(&root).map_err(|e| e.to_string())?
        } else {
            Project::new(1080, 1920, 30, 180).map_err(|e| e.to_string())?
        }
    } else {
        serde_json::from_str(project_text).map_err(|e| e.to_string())?
    };
    crate::session::open(root, project, platform).map_err(|error| {
        crate::session::set_creation_error(error.clone());
        error
    })
}

pub fn template(kind: i32) -> Result<Value> {
    if kind != 0 {
        return Err("unknown project template".into());
    }
    serde_json::to_value(Project::demo()).map_err(|e| e.to_string())
}

pub fn state(id: i64) -> Result<Value> {
    crate::session::with_session(id, |s| Ok(s.snapshot()))
}

pub fn save(id: i64) -> Result<Value> {
    crate::session::with_session(id, |s| {
        aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
        Ok(s.snapshot())
    })
}

pub fn new_project(id: i64, text: &str) -> Result<Value> {
    let project: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !project.assets.is_empty()
        || !project.audio_assets.is_empty()
        || !project.video_assets.is_empty()
    {
        return Err("new project must have no external assets".into());
    }
    let engine = aem_core::Engine::new(project).map_err(|e| e.to_string())?;
    crate::session::with_session(id, |s| {
        let parent = s.root.parent().ok_or("project parent is missing")?;
        let destination = parent.join(format!("project-{}", stamp()?));
        if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
            aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
        aem_core::storage::save(&destination, engine.project()).map_err(|e| e.to_string())?;
        s.replace_project(engine, destination)?;
        Ok(s.snapshot())
    })
}

pub fn open_project(id: i64, name: &str) -> Result<Value> {
    crate::session::with_session(id, |s| {
        let current = s
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid active project directory")?;
        if !project_name_allowed(name, current) || name.contains('/') || name.contains('\\') {
            return Err("invalid project directory".into());
        }
        let parent = s
            .root
            .parent()
            .ok_or("project parent is missing")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let destination = parent
            .join(name)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if destination.parent() != Some(parent.as_path()) {
            return Err("project resolves outside its library".into());
        }
        let project = aem_core::storage::load(&destination).map_err(|e| e.to_string())?;
        let engine = aem_core::Engine::new(project).map_err(|e| e.to_string())?;
        if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
            aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
        }
        s.replace_project(engine, destination)?;
        Ok(s.snapshot())
    })
}

pub fn replace(id: i64, text: &str) -> Result<Value> {
    let project: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
    crate::session::with_session(id, |s| {
        aem_core::storage::validate_assets(&s.root, &project).map_err(|e| e.to_string())?;
        let engine = aem_core::Engine::new(project).map_err(|e| e.to_string())?;
        s.replace_project(engine, s.root.clone())?;
        Ok(s.snapshot())
    })
}

/// Open an explicitly selected desktop project directory. Loading and asset
/// validation finish before the current project is saved or replaced.
pub fn open_directory(id: i64, directory: &std::path::Path) -> Result<Value> {
    let destination = directory.canonicalize().map_err(|e| e.to_string())?;
    let project = aem_core::storage::load(&destination).map_err(|e| e.to_string())?;
    let engine = aem_core::Engine::new(project).map_err(|e| e.to_string())?;
    crate::session::with_session(id, |s| {
        aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
        s.replace_project(engine, destination)?;
        Ok(s.snapshot())
    })
}

pub fn import_project(id: i64, file: &std::path::Path) -> Result<Value> {
    crate::session::with_session(id, |s| {
        let parent = s.root.parent().ok_or("project parent is missing")?;
        let destination = parent.join(format!("import-{}", stamp()?));
        let project =
            aem_core::storage::import_package(file, &destination).map_err(|e| e.to_string())?;
        let engine = aem_core::Engine::new(project).map_err(|e| e.to_string())?;
        aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
        s.replace_project(engine, destination)?;
        Ok(s.snapshot())
    })
}
