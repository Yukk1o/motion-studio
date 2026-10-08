//! Project/session lifecycle, storage and persisted state JNI entry points.
use super::*;

use std::cell::RefCell;
thread_local! {static CREATION_ERROR:RefCell<String>=const {RefCell::new(String::new())};}

fn project_name_allowed(name: &str, current: &str) -> bool {
    name == current
        || name == "default"
        || ["project-", "import-"].iter().any(|prefix| {
            name.strip_prefix(*prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_resourceInfo(
    mut env: JNIEnv,
    _class: JClass,
    directory: JString,
) -> jstring {
    let parsed = read_string(&mut env, &directory);
    string_result(&mut env, || {
        let root = PathBuf::from(parsed?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
        let mut count = 0u64;
        let mut graphics = 0u64;
        let mut assets = 0u64;
        let mut targets = 0u64;
        for session in registry.values().filter(|s| {
            s.root
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&root))
        }) {
            count += 1;
            if let Some(g) = &session.graphics {
                graphics += 1;
                assets += g.renderer.texture_bytes();
                targets += g.scratch.texture_bytes();
            }
        }
        Ok(
            json!({"sessions":count,"graphics":graphics,"assetTextureBytes":assets,"renderTargetBytes":targets,
            "scope":"Application-owned sessions and textures in the requested project directory; not driver/system allocations"}),
        )
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_create(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
    project: JString,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i64> {
        let root = PathBuf::from(read_string(&mut env, &root)?);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let text = read_string(&mut env, &project)?;
        let project = if text.is_empty() {
            if root.join("project.json").exists() {
                aem_core::storage::load(&root).map_err(|e| e.to_string())?
            } else {
                Project::new(1080, 1920, 30, 180).map_err(|e| e.to_string())?
            }
        } else {
            serde_json::from_str(&text).map_err(|e| e.to_string())?
        };
        let session = Session::new(project, root)?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        sessions()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, session);
        Ok(id)
    }));
    match result {
        Ok(Ok(id)) => {
            CREATION_ERROR.with(|e| e.borrow_mut().clear());
            id
        }
        Ok(Err(error)) => {
            CREATION_ERROR.with(|e| *e.borrow_mut() = error);
            0
        }
        Err(_) => {
            CREATION_ERROR.with(|e| *e.borrow_mut() = "原生工程初始化失败".into());
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_projectTemplate(
    mut env: JNIEnv,
    _class: JClass,
    kind: jint,
) -> jstring {
    string_result(&mut env, || {
        if kind != 0 {
            return Err("unknown project template".into());
        }
        serde_json::to_value(Project::demo()).map_err(|e| e.to_string())
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_creationError(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    CREATION_ERROR.with(|e| {
        env.new_string(e.borrow().as_str())
            .map_or(std::ptr::null_mut(), |s| s.into_raw())
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_state(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || with_session(id, |s| Ok(s.snapshot())))
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_save(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jstring {
    string_result(&mut env, || {
        with_session(id, |s| {
            aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_newProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || {
        with_session(id, |s| {
            let project: Project = serde_json::from_str(&text?).map_err(|e| e.to_string())?;
            if !project.assets.is_empty()
                || !project.audio_assets.is_empty()
                || !project.video_assets.is_empty()
            {
                return Err("new project must have no external assets".into());
            }
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            let parent = s.root.parent().ok_or("project parent is missing")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let destination = parent.join(format!("project-{stamp}"));
            if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
                aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            }
            std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
            aem_core::storage::save(&destination, engine.project()).map_err(|e| e.to_string())?;
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_openProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    directory: JString,
) -> jstring {
    let name = read_string(&mut env, &directory);
    string_result(&mut env, || {
        with_session(id, |s| {
            let name = name?;
            let current = s
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("invalid active project directory")?;
            if !project_name_allowed(&name, current) || name.contains('/') || name.contains('\\') {
                return Err("invalid project directory".into());
            }
            let parent = s
                .root
                .parent()
                .ok_or("project parent is missing")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let destination = parent
                .join(&name)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if destination.parent() != Some(parent.as_path()) {
                return Err("project resolves outside its library".into());
            }
            let project = aem_core::storage::load(&destination).map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            if aem_core::storage::validate_assets(&s.root, s.engine.project()).is_ok() {
                aem_core::storage::save(&s.root, s.engine.project()).map_err(|e| e.to_string())?;
            }
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_replace(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    text: JString,
) -> jstring {
    let text = read_string(&mut env, &text);
    string_result(&mut env, || {
        with_session(id, |s| {
            let project: Project = serde_json::from_str(&text?).map_err(|e| e.to_string())?;
            aem_core::storage::validate_assets(&s.root, &project).map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            s.replace_project(engine, s.root.clone())?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_importProject(
    mut env: JNIEnv,
    _class: JClass,
    id: jlong,
    file: JString,
) -> jstring {
    let path = read_string(&mut env, &file);
    string_result(&mut env, || {
        with_session(id, |s| {
            let input = PathBuf::from(path?);
            let parent = s.root.parent().ok_or("project parent is missing")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let destination = parent.join(format!("import-{stamp}"));
            let project = aem_core::storage::import_package(&input, &destination)
                .map_err(|e| e.to_string())?;
            let engine = Engine::new(project).map_err(|e| e.to_string())?;
            s.replace_project(engine, destination)?;
            Ok(s.snapshot())
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_com_motionstudio_editor_NativeBridge_destroy(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut registry = sessions().lock().unwrap_or_else(|e| e.into_inner());
        if registry
            .get(&id)
            .is_some_and(|s| s.owner == thread::current().id())
        {
            if let Some(mut s) = registry.remove(&id) {
                s.detach();
            }
        }
    }));
}
