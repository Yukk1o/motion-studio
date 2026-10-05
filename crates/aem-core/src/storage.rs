use crate::{ensure, Project, Result};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_ASSET: u64 = 64 * 1024 * 1024;
pub const MAX_MEDIA_ASSET: u64 = 512 * 1024 * 1024;
pub const MAX_PACKAGE: u64 = 4 * 1024 * 1024 * 1024;

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
                    && *previous_limit == MAX_MEDIA_ASSET
                    && limit == MAX_MEDIA_ASSET
                    && *previous_bytes == bytes
                    && project
                        .video_assets
                        .iter()
                        .any(|v| v.path == *asset_path && v.audio_asset.is_some()),
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
    let project = read_json(&root.join("project.json"))?;
    validate_assets(root, &project)?;
    Ok(project)
}
pub fn save(root: &Path, project: &Project) -> Result<()> {
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
pub fn export_package(root: &Path, project: &Project, output: &Path) -> Result<()> {
    project.validate()?;
    validate_assets(root, project)?;
    let parent = output
        .parent()
        .ok_or_else(|| crate::Error::Invalid("invalid package destination".into()))?;
    fs::create_dir_all(parent)?;
    let temp = output.with_extension("aem.tmp");
    let file = File::create(&temp)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("project.json", options)?;
    zip.write_all(&serde_json::to_vec_pretty(project)?)?;
    let mut total = 0;
    let mut written = HashSet::new();
    for path in project
        .assets
        .iter()
        .map(|a| &a.path)
        .chain(project.audio_assets.iter().map(|a| &a.path))
        .chain(project.video_assets.iter().map(|a| &a.path))
    {
        if !written.insert(path) {
            continue;
        }
        total += fs::metadata(root.join(path))?.len();
        ensure(total <= MAX_PACKAGE, "package resource budget exceeded")?;
        zip.start_file(path, options)?;
        std::io::copy(&mut File::open(root.join(path))?, &mut zip)?;
    }
    let file = zip.finish()?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, output)?;
    Ok(())
}
/// Import into a brand-new destination, leaving any existing project untouched.
pub fn import_package(input: &Path, destination: &Path) -> Result<Project> {
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
    let stage = base.join(format!(".aem-import-{}-{stamp}", std::process::id()));
    fs::create_dir(&stage)?;
    let cleanup = StageCleanup {
        stage: stage.clone(),
        base: base.clone(),
    };
    let mut zip = ZipArchive::new(File::open(input)?)?;
    ensure(
        zip.len() <= crate::MAX_LAYERS * 2 + 1,
        "too many package entries",
    )?;
    let mut total = 0u64;
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
        let copied = std::io::copy(&mut (&mut entry).take(limit + 1), &mut file)?;
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
        .chain(std::iter::once("project.json".into()))
        .collect();
    ensure(
        names == referenced,
        "package contains missing or unreferenced files",
    )?;
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
                .is_some_and(|n| n.to_string_lossy().starts_with(".aem-import-"));
            if path.starts_with(&self.base) && path != self.base && owned_name {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
}
