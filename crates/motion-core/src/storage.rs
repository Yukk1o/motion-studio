use crate::{ensure, Project, Result};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_ASSET: u64 = 64 * 1024 * 1024;
pub const MAX_MEDIA_ASSET: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PACKAGE: u64 = 4 * 1024 * 1024 * 1024;
/// The payload budget plus bounded ZIP headers/compression overhead.
pub const MAX_PACKAGE_ARCHIVE: u64 = MAX_PACKAGE + 4 * 1024 * 1024;
pub const TRANSFER_BUFFER_BYTES: usize = 64 * 1024;

pub fn validate_relative_path(path: &str) -> Result<()> {
    ensure(
        path.len() <= 1024 && path.starts_with("assets/"),
        "assets must be inside the assets directory",
    )?;
    ensure(
        !path.contains('\\') && !path.contains(':') && !path.contains('\0'),
        "invalid asset path",
    )?;
    ensure(
        path.split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".."),
        "invalid asset path component",
    )
}
fn read_json(path: &Path) -> Result<Project> {
    let file = File::open(path)?;
    ensure(
        file.metadata()?.len() <= MAX_JSON,
        "project JSON exceeds the size limit",
    )?;
    let project: Project = serde_json::from_reader(file.take(MAX_JSON + 1))?;
    project.migrate()
}
pub fn validate_assets(root: &Path, project: &Project) -> Result<()> {
    let base = root.canonicalize()?;
    let mut seen = std::collections::HashMap::new();
    let mut total = 0u64;
    for (asset_path, limit, bytes) in project
        .assets
        .iter()
        .map(|a| (&a.path, MAX_ASSET, None))
        .chain(project.fonts.iter().map(|a| (&a.path, 32*1024*1024, None)))
        .chain(
            project
                .audio_assets
                .iter()
                .map(|a| (&a.path, MAX_MEDIA_ASSET, Some(a.bytes))),
        )
        .chain(
            project
                .video_assets
                .iter()
                .map(|a| (&a.path, MAX_MEDIA_ASSET, Some(a.bytes))),
        )
    {
        validate_relative_path(asset_path)?;
        if let Some((previous_path, previous_limit, previous_bytes)) =
            seen.get(&asset_path.to_lowercase())
        {
            ensure(
                previous_path == asset_path
                    && *previous_bytes == bytes
                    && ((*previous_limit == 32*1024*1024 && limit == 32*1024*1024 && project.fonts.iter().any(|f|f.path==*asset_path))
                        || (*previous_limit == MAX_MEDIA_ASSET && limit == MAX_MEDIA_ASSET && project
                        .video_assets
                        .iter()
                        .any(|v| v.path == *asset_path && v.audio_asset.is_some()))),
                "duplicate asset path",
            )?;
            continue;
        }
        seen.insert(
            asset_path.to_lowercase(),
            (asset_path.clone(), limit, bytes),
        );
        let path = root.join(asset_path).canonicalize().map_err(|error| {
            crate::Error::Invalid(format!("素材无法读取：{asset_path} ({error})"))
        })?;
        ensure(
            path.starts_with(&base),
            "asset resolves outside project directory",
        )?;
        let meta = path.metadata()?;
        total = total
            .checked_add(meta.len())
            .ok_or_else(|| crate::Error::Invalid("project resource size overflow".into()))?;
        ensure(total <= MAX_PACKAGE, "project resource budget exceeded")?;
        ensure(
            meta.is_file() && meta.len() <= limit && bytes.is_none_or(|n| n == meta.len()),
            "asset is invalid or too large",
        )?;
    }
    Ok(())
}
pub fn load(root: &Path) -> Result<Project> {
    let mut project = read_json(&root.join("project.json"))?;
    project.activate_composition(crate::MAIN_COMPOSITION)?;
    validate_assets(root, &project)?;
    Ok(project)
}
pub fn save(root: &Path, project: &Project) -> Result<()> {
    let canonical = project.composition(crate::MAIN_COMPOSITION)?;
    let project = &canonical;
    project.validate()?;
    fs::create_dir_all(root)?;
    validate_assets(root, project)?;
    let bytes = serde_json::to_vec_pretty(project)?;
    ensure(
        bytes.len() as u64 <= MAX_JSON,
        "project JSON exceeds the size limit",
    )?;
    let temp = root.join("project.json.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, root.join("project.json"))?;
    Ok(())
}
struct PackageSource {
    name: String,
    file: File,
    bytes: u64,
}
/// Retains open source files and an immutable, canonical project description.
/// Creating this snapshot is cheap; payload reads happen in `prepare`.
pub struct PackageSnapshot {
    json: Vec<u8>,
    sources: Vec<PackageSource>,
    total: u64,
    root: PathBuf,
}
pub struct PreparedPackage {
    temporary: PathBuf,
    output: PathBuf,
}
impl Drop for PreparedPackage {
    fn drop(&mut self) {
        // This exact temporary file was exclusively created by this transaction.
        let _ = fs::remove_file(&self.temporary);
    }
}
impl PreparedPackage {
    /// Call under the task's cancellation lock to make publication atomic with
    /// cancellation. Until this point an existing output remains untouched.
    pub fn publish(self) -> Result<PathBuf> {
        fs::rename(&self.temporary, &self.output)?;
        Ok(self.output.clone())
    }
}
impl PackageSnapshot {
    pub fn new(root: &Path, project: &Project) -> Result<Self> {
        let project = project.composition(crate::MAIN_COMPOSITION)?;
        project.validate()?;
        validate_assets(root, &project)?;
        let root = root.canonicalize()?;
        let json = serde_json::to_vec_pretty(&project)?;
        ensure(
            json.len() as u64 <= MAX_JSON,
            "project JSON exceeds the size limit",
        )?;
        let mut total = json.len() as u64;
        let mut sources = Vec::new();
        let mut written = HashSet::new();
        for (name, limit, declared_bytes) in project
            .assets
            .iter()
            .map(|a| (&a.path, MAX_ASSET, None))
        .chain(project.fonts.iter().map(|a| (&a.path, 32*1024*1024, None)))
            .chain(
                project
                    .audio_assets
                    .iter()
                    .map(|a| (&a.path, MAX_MEDIA_ASSET, Some(a.bytes))),
            )
            .chain(
                project
                    .video_assets
                    .iter()
                    .map(|a| (&a.path, MAX_MEDIA_ASSET, Some(a.bytes))),
            )
        {
            if !written.insert(name) {
                continue;
            }
            let path = root.join(name).canonicalize()?;
            ensure(
                path.starts_with(&root),
                "asset resolves outside project directory",
            )?;
            let file = File::open(path)?;
            let metadata = file.metadata()?;
            let bytes = metadata.len();
            ensure(
                metadata.is_file() && bytes <= limit && declared_bytes.is_none_or(|b| b == bytes),
                "source changed before package snapshot",
            )?;
            total = total
                .checked_add(bytes)
                .ok_or_else(|| crate::Error::Invalid("package size overflow".into()))?;
            ensure(total <= MAX_PACKAGE, "package resource budget exceeded")?;
            sources.push(PackageSource {
                name: name.clone(),
                file,
                bytes,
            });
        }
        Ok(Self {
            json,
            sources,
            total,
            root,
        })
    }
    pub fn total_bytes(&self) -> u64 {
        self.total
    }
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
    pub fn prepare(
        mut self,
        output: &Path,
        mut progress: impl FnMut(u64, u64) -> Result<()>,
    ) -> Result<PreparedPackage> {
        let parent = output
            .parent()
            .ok_or_else(|| crate::Error::Invalid("invalid package destination".into()))?;
        fs::create_dir_all(parent)?;
        let parent = parent.canonicalize()?;
        let output = parent.join(
            output
                .file_name()
                .ok_or_else(|| crate::Error::Invalid("invalid package filename".into()))?,
        );
        ensure(
            output != self.root.join("project.json")
                && !self
                    .sources
                    .iter()
                    .any(|s| output == self.root.join(&s.name)),
            "package output must not replace project sources",
        )?;
        progress(0, self.total)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = parent.join(format!(".ms-package-{}-{stamp}.tmp", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let prepared = PreparedPackage { temporary, output };
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "project.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        zip.write_all(&self.json)?;
        let mut done = self.json.len() as u64;
        let mut buffer = [0; TRANSFER_BUFFER_BYTES];
        progress(done, self.total)?;
        for source in &mut self.sources {
            source.file.seek(SeekFrom::Start(0))?;
            // Video, PNG and compressed audio have already been encoded. Avoid
            // recompressing multi-GiB media and reserve ZIP64 entry sizes.
            zip.start_file(
                &source.name,
                SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Stored)
                    .large_file(true),
            )?;
            let mut copied = 0;
            loop {
                progress(done, self.total)?;
                let n = source.file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                copied += n as u64;
                ensure(copied <= source.bytes, "source changed while packaging")?;
                zip.write_all(&buffer[..n])?;
                done += n as u64;
            }
            ensure(copied == source.bytes, "source changed while packaging")?;
        }
        let file = zip.finish()?;
        ensure(
            file.metadata()?.len() <= MAX_PACKAGE_ARCHIVE,
            "package archive exceeds its size limit",
        )?;
        file.sync_all()?;
        drop(file);
        progress(done, self.total)?;
        Ok(prepared)
    }
}
pub fn export_package(root: &Path, project: &Project, output: &Path) -> Result<()> {
    export_package_with_progress(root, project, output, |_, _| Ok(()))
}
pub fn export_package_with_progress(
    root: &Path,
    project: &Project,
    output: &Path,
    progress: impl FnMut(u64, u64) -> Result<()>,
) -> Result<()> {
    PackageSnapshot::new(root, project)?
        .prepare(output, progress)?
        .publish()?;
    Ok(())
}
/// Import into a brand-new destination, leaving any existing project untouched.
pub fn import_package(input: &Path, destination: &Path) -> Result<Project> {
    import_package_with_progress(input, destination, |_, _| Ok(()))
}
pub fn import_package_with_progress(
    input: &Path,
    destination: &Path,
    mut progress: impl FnMut(u64, u64) -> Result<()>,
) -> Result<Project> {
    ensure(!destination.exists(), "import destination already exists")?;
    let parent = destination
        .parent()
        .ok_or_else(|| crate::Error::Invalid("invalid import destination".into()))?;
    fs::create_dir_all(parent)?;
    let base = parent.canonicalize()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = base.join(format!(".motion-import-{}-{stamp}", std::process::id()));
    fs::create_dir(&stage)?;
    let cleanup = StageCleanup {
        stage: stage.clone(),
        base: base.clone(),
    };
    let input = File::open(input)?;
    ensure(
        input.metadata()?.len() <= MAX_PACKAGE_ARCHIVE,
        "package archive exceeds its size limit",
    )?;
    let mut zip = ZipArchive::new(input)?;
    ensure(
        zip.len() <= crate::MAX_LAYERS * 2 + 1,
        "too many package entries",
    )?;
    let mut total = 0u64;
    let mut expected = 0u64;
    for i in 0..zip.len() {
        expected = expected
            .checked_add(zip.by_index(i)?.size())
            .ok_or_else(|| crate::Error::Invalid("archive size overflow".into()))?;
        ensure(expected <= MAX_PACKAGE, "archive resource budget exceeded")?;
    }
    let mut done = 0;
    progress(done, expected)?;
    let mut names = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_owned();
        ensure(names.insert(name.to_lowercase()), "duplicate archive entry")?;
        ensure(!entry.is_dir(), "unexpected directory entry")?;
        ensure(
            entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "symbolic links are not project assets",
        )?;
        if name != "project.json" {
            validate_relative_path(&name)?;
        }
        let limit = if name == "project.json" {
            MAX_JSON
        } else {
            MAX_MEDIA_ASSET
        };
        ensure(
            entry.size() <= limit,
            "archive entry exceeds its size budget",
        )?;
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| crate::Error::Invalid("archive size overflow".into()))?;
        ensure(total <= MAX_PACKAGE, "archive resource budget exceeded")?;
        let path = stage.join(&name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = File::create(path)?;
        let mut copied = 0;
        let mut buffer = [0; TRANSFER_BUFFER_BYTES];
        loop {
            progress(done, expected)?;
            let n = entry.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            ensure(
                copied <= limit && copied <= entry.size(),
                "archive entry length mismatch",
            )?;
            file.write_all(&buffer[..n])?;
            done += n as u64;
        }
        ensure(
            copied == entry.size() && copied <= limit,
            "archive entry length mismatch",
        )?;
        file.sync_all()?;
    }
    let project = load(&stage)?;
    let referenced: HashSet<_> = project
        .assets
        .iter()
        .map(|a| a.path.to_lowercase())
        .chain(project.audio_assets.iter().map(|a| a.path.to_lowercase()))
        .chain(project.video_assets.iter().map(|a| a.path.to_lowercase()))
        .chain(project.fonts.iter().map(|a| a.path.to_lowercase()))
        .chain(std::iter::once("project.json".into()))
        .collect();
    ensure(
        names == referenced,
        "package contains missing or unreferenced files",
    )?;
    progress(done, expected)?;
    let destination_name = destination
        .file_name()
        .ok_or_else(|| crate::Error::Invalid("invalid import name".into()))?;
    let resolved_destination = base.join(destination_name);
    ensure(
        stage.canonicalize()?.starts_with(&base)
            && destination.parent().unwrap().canonicalize()? == base
            && resolved_destination.starts_with(&base)
            && resolved_destination != base,
        "import rename must remain in its selected parent directory",
    )?;
    fs::rename(&stage, resolved_destination)?;
    drop(cleanup);
    Ok(project)
}
struct StageCleanup {
    stage: PathBuf,
    base: PathBuf,
}
impl Drop for StageCleanup {
    fn drop(&mut self) {
        // Only remove this importer-owned staging directory after resolving it.
        if let Ok(path) = self.stage.canonicalize() {
            let owned_name = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(".motion-import-"));
            if path.starts_with(&self.base) && path != self.base && owned_name {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
}
