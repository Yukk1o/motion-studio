//! Explicit host-supplied capabilities. Implementations keep their own runtime,
//! history and file publication; the data layer does not select or install one.
use crate::{Command, EditResult, Project, Result};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

/// Evaluate one composition at its actual (possibly fractional) source frame.
pub trait FrameEvaluator {
    fn evaluate<'a>(&self, project: &'a Project, frame: f64) -> Result<Cow<'a, Project>>;
}

/// Apply an import on the editor owner thread, preserving atomic save and undo.
pub trait EditSink {
    fn project(&self) -> &Project;
    fn apply_batch_saved(&mut self, commands: Vec<Command>, root: &Path)
        -> Result<Vec<EditResult>>;
}

/// A prepared archive publishes while the caller holds its cancellation lock.
/// Dropping it before publication must close and remove its staging file.
pub trait PackagePublication {
    fn publish(self) -> Result<PathBuf>;
}

/// Owned frozen sources move into a worker; concrete handles and IO stay with
/// the provider. Preparation must propagate progress/cancellation failures.
pub trait PackageExportSource: Send + 'static {
    type Prepared: PackagePublication;
    fn total_bytes(&self) -> u64;
    fn source_count(&self) -> usize;
    fn prepare(
        self,
        output: &Path,
        progress: impl FnMut(u64, u64) -> Result<()>,
    ) -> Result<Self::Prepared>;
}
