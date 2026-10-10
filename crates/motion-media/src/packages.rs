//! Frozen project packages. Payload I/O never runs on the engine owner thread.
use crate::Result;
use motion_model::storage::{MAX_PACKAGE, MAX_PACKAGE_ARCHIVE};
use motion_model::{PackageExportSource, PackagePublication};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

static WORKERS: AtomicUsize = AtomicUsize::new(0);
struct Worker;
impl Drop for Worker {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageTaskStatus {
    pub schema_version: u32,
    pub request_id: String,
    pub operation: String,
    pub state: String,
    pub phase: String,
    pub progress: f64,
    pub bytes_processed: u64,
    pub total_bytes: u64,
    pub source_count: usize,
    pub frozen_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl PackageTaskStatus {
    fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "cancelled")
    }
}
struct Task {
    status: PackageTaskStatus,
    cancel: bool,
}
#[derive(Default)]
pub struct PackageJobs {
    tasks: Mutex<HashMap<String, Arc<Mutex<Task>>>>,
}
impl PackageJobs {
    fn task(&self, id: &str) -> Result<Arc<Mutex<Task>>> {
        self.tasks
            .lock()
            .map_err(|_| "package registry poisoned")?
            .get(id)
            .cloned()
            .ok_or_else(|| "package task does not exist".into())
    }
    pub fn contains(&self, id: &str) -> bool {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(id)
    }
    pub fn status(&self, id: &str) -> Result<PackageTaskStatus> {
        Ok(self
            .task(id)?
            .lock()
            .map_err(|_| "package task poisoned")?
            .status
            .clone())
    }
    /// Cancellation is acknowledged as running/cancelling until the worker has
    /// closed and removed its staging file. Publication and cancellation share a lock.
    pub fn cancel(&self, id: &str) -> Result<PackageTaskStatus> {
        let task = self.task(id)?;
        let mut task = task.lock().map_err(|_| "package task poisoned")?;
        if !task.status.terminal() {
            task.cancel = true;
            task.status.phase = "cancelling".into();
        }
        Ok(task.status.clone())
    }
    pub fn release(&self, id: &str) -> Result<()> {
        let mut tasks = self.tasks.lock().map_err(|_| "package registry poisoned")?;
        if !tasks
            .get(id)
            .ok_or("package task does not exist")?
            .lock()
            .map_err(|_| "package task poisoned")?
            .status
            .terminal()
        {
            return Err("wait for package task completion before release".into());
        }
        tasks.remove(id);
        Ok(())
    }
    pub fn start_export(
        &self,
        id: &str,
        snapshot: impl PackageExportSource,
        output: PathBuf,
        frozen_revision: u64,
    ) -> Result<PackageTaskStatus> {
        if id.is_empty() || id.len() > 128 {
            return Err("invalid package request id".into());
        }
        let mut tasks = self.tasks.lock().map_err(|_| "package registry poisoned")?;
        if tasks.contains_key(id) {
            return Err("media request id already exists".into());
        }
        if tasks.len() >= 64 {
            return Err("release completed package tasks before creating more".into());
        }
        if tasks.values().any(|t| {
            !t.lock()
                .unwrap_or_else(|e| e.into_inner())
                .status
                .terminal()
        }) {
            return Err("at most one package task may run per session".into());
        }
        if WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .is_err()
        {
            return Err("package worker budget exhausted".into());
        }
        let worker = Worker;
        let initial = PackageTaskStatus {
            schema_version: 1,
            request_id: id.into(),
            operation: "export_project".into(),
            state: "running".into(),
            phase: "queued".into(),
            progress: 0.,
            bytes_processed: 0,
            total_bytes: snapshot.total_bytes(),
            source_count: snapshot.source_count(),
            frozen_revision,
            path: None,
            error: None,
        };
        let task = Arc::new(Mutex::new(Task {
            status: initial.clone(),
            cancel: false,
        }));
        tasks.insert(id.into(), task.clone());
        let spawn_task = task.clone();
        let spawned = std::thread::Builder::new()
            .name("motion-package-export".into())
            .spawn(move || {
                let _worker = worker;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let parent = output.parent().ok_or_else(|| {
                        motion_model::Error::Invalid("invalid package destination".into())
                    })?;
                    std::fs::create_dir_all(parent)?;
                    if fs2::available_space(parent)?
                        < snapshot
                            .total_bytes()
                            .saturating_add(MAX_PACKAGE_ARCHIVE - MAX_PACKAGE)
                    {
                        return Err(motion_model::Error::Invalid(
                            "insufficient package output storage".into(),
                        ));
                    }
                    snapshot.prepare(&output, |done, total| {
                        let mut task = spawn_task.lock().unwrap_or_else(|e| e.into_inner());
                        if task.cancel {
                            return Err(motion_model::Error::Invalid(
                                "package export cancelled".into(),
                            ));
                        }
                        task.status.phase = "packing".into();
                        task.status.bytes_processed = done;
                        task.status.progress = done as f64 / total.max(1) as f64;
                        Ok(())
                    })
                }));
                let mut task = spawn_task.lock().unwrap_or_else(|e| e.into_inner());
                if task.cancel {
                    // Drop/close the prepared archive before advertising terminal cancellation.
                    drop(result);
                    task.status.state = "cancelled".into();
                    task.status.phase = "cancelled".into();
                    return;
                }
                let published = match result {
                    Ok(Ok(prepared)) => prepared.publish().map_err(|e| e.to_string()),
                    Ok(Err(error)) => Err(error.to_string()),
                    Err(_) => Err("package worker failed".into()),
                };
                match published {
                    Ok(path) => {
                        task.status.state = "succeeded".into();
                        task.status.phase = "complete".into();
                        task.status.progress = 1.;
                        task.status.path = Some(path);
                    }
                    Err(error) => {
                        task.status.state = "failed".into();
                        task.status.phase = "failed".into();
                        task.status.error = Some(error);
                    }
                }
            });
        if let Err(error) = spawned {
            let mut task = task.lock().unwrap_or_else(|e| e.into_inner());
            task.status.state = "failed".into();
            task.status.phase = "failed".into();
            task.status.error = Some(error.to_string());
            return Err(error.to_string());
        }
        Ok(initial)
    }
}
impl Drop for PackageJobs {
    fn drop(&mut self) {
        for task in self
            .tasks
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            let mut task = task.lock().unwrap_or_else(|e| e.into_inner());
            if !task.status.terminal() {
                task.cancel = true;
                task.status.phase = "cancelling".into();
            }
        }
    }
}
