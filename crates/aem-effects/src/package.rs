use crate::{ensure, shader, validate_path, Error, PluginManifest, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_PACKAGE: usize = 16 * 1024 * 1024;
const MAX_MANIFEST: usize = 256 * 1024;
const MAX_SHADER: usize = 256 * 1024;
pub struct EffectPackage {
    pub manifest: PluginManifest,
    pub hash: String,
    pub files: BTreeMap<String, Vec<u8>>,
    pub shaders: BTreeMap<(String, usize), shader::CompiledShader>,
    pub bytes: Vec<u8>,
}
impl EffectPackage {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        ensure(bytes.len() <= MAX_PACKAGE, "effect package exceeds 16 MiB")?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let mut archive = ZipArchive::new(std::io::Cursor::new(&bytes))?;
        ensure(archive.len() <= 256, "too many effect package entries")?;
        let mut files = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_owned();
            validate_path(&name)?;
            ensure(
                !entry.is_dir() && entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "directories and symlinks are not effect resources",
            )?;
            ensure(names.insert(name.to_lowercase()), "duplicate package entry")?;
            let limit = if name == "manifest.json" {
                MAX_MANIFEST
            } else if name.starts_with("shaders/") && name.ends_with(".wgsl") {
                MAX_SHADER
            } else if name.starts_with("assets/") && name.ends_with(".png") {
                MAX_PACKAGE
            } else {
                return Err(Error::Invalid("unexpected package file".into()));
            };
            ensure(
                entry.size() <= limit as u64,
                "effect resource exceeds size limit",
            )?;
            let mut data = Vec::new();
            (&mut entry).take(limit as u64 + 1).read_to_end(&mut data)?;
            ensure(
                data.len() <= limit && data.len() as u64 == entry.size(),
                "effect resource length mismatch",
            )?;
            total = total
                .checked_add(data.len())
                .ok_or_else(|| Error::Invalid("package size overflow".into()))?;
            ensure(
                total <= MAX_PACKAGE,
                "expanded effect package exceeds 16 MiB",
            )?;
            files.insert(name, data);
        }
        let manifest: PluginManifest = serde_json::from_slice(
            files
                .get("manifest.json")
                .ok_or_else(|| Error::Invalid("manifest.json is missing".into()))?,
        )?;
        manifest.validate()?;
        let mut shaders = BTreeMap::new();
        let mut referenced = BTreeSet::from(["manifest.json".to_owned()]);
        let mut decoded_resources = BTreeSet::new();
        let mut decoded_bytes = 0u64;
        for effect in &manifest.effects {
            for (i, pass) in effect.passes.iter().enumerate() {
                referenced.insert(pass.shader.clone());
                let source =
                    std::str::from_utf8(files.get(&pass.shader).ok_or_else(|| {
                        Error::Invalid(format!("shader {} is missing", pass.shader))
                    })?)
                    .map_err(|_| Error::Invalid("shader must be UTF-8".into()))?;
                let compiled = shader::compile(source, &pass.entry)
                    .map_err(|e| Error::Invalid(format!("effect {}, pass {i}: {e}", effect.id)))?;
                shaders.insert((effect.id.clone(), i), compiled);
            }
            for name in &effect.resources {
                referenced.insert(name.clone());
                let data = files
                    .get(name)
                    .ok_or_else(|| Error::Invalid(format!("resource {name} is missing")))?;
                let reader = image::ImageReader::new(std::io::Cursor::new(data))
                    .with_guessed_format()
                    .map_err(Error::Io)?;
                let (w, h) = reader
                    .into_dimensions()
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                ensure(
                    w > 0 && h > 0 && u64::from(w) * u64::from(h) * 4 <= 128 * 1024 * 1024,
                    "effect PNG exceeds decoded texture budget",
                )?;
                if decoded_resources.insert(name.clone()) {
                    decoded_bytes += u64::from(w) * u64::from(h) * 4;
                    ensure(
                        decoded_bytes <= 128 * 1024 * 1024,
                        "package PNG resources exceed 128 MiB",
                    )?;
                    image::load_from_memory_with_format(data, image::ImageFormat::Png)
                        .map_err(|e| Error::Invalid(format!("resource {name}: {e}")))?;
                }
            }
        }
        ensure(
            referenced == files.keys().cloned().collect(),
            "effect package contains unreferenced files",
        )?;
        Ok(Self {
            manifest,
            hash,
            files,
            shaders,
            bytes,
        })
    }
    pub fn open(path: &Path) -> Result<Self> {
        ensure(
            fs::metadata(path)?.len() <= MAX_PACKAGE as u64,
            "effect package exceeds 16 MiB",
        )?;
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_PACKAGE as u64 + 1)
            .read_to_end(&mut bytes)?;
        Self::from_bytes(bytes)
    }
}
pub fn package_directory(path: &Path) -> Result<Vec<u8>> {
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(path.join("manifest.json"))?)?;
    manifest.validate()?;
    let mut names = BTreeSet::from(["manifest.json".to_owned()]);
    for effect in &manifest.effects {
        for pass in &effect.passes {
            names.insert(pass.shader.clone());
        }
        for resource in &effect.resources {
            names.insert(resource.clone());
        }
    }
    let base = path.canonicalize()?;
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let mut size = 0usize;
    for name in names {
        let file = path.join(&name).canonicalize()?;
        ensure(
            file.starts_with(&base),
            "package file resolves outside source directory",
        )?;
        let data = fs::read(file)?;
        size = size.saturating_add(data.len());
        ensure(size <= MAX_PACKAGE, "expanded package exceeds 16 MiB")?;
        writer.start_file(name, options)?;
        writer.write_all(&data)?;
    }
    let bytes = writer.finish()?.into_inner();
    EffectPackage::from_bytes(bytes.clone())?;
    Ok(bytes)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: String,
    pub hash: String,
    pub enabled: bool,
}
#[derive(Default, Clone)]
pub struct Registry {
    pub packages: BTreeMap<(String, String, String), Arc<EffectPackage>>,
    pub disabled: BTreeSet<(String, String, String)>,
    pub diagnostics: Vec<String>,
}
impl Registry {
    pub fn new_with_builtins() -> Result<Self> {
        let mut r = Self::default();
        for package in crate::builtin::packages()? {
            r.insert(package)?;
        }
        Ok(r)
    }
    pub fn insert(&mut self, package: Arc<EffectPackage>) -> Result<()> {
        for (id, version, hash) in self.packages.keys() {
            ensure(
                id != &package.manifest.id
                    || version != &package.manifest.version
                    || hash == &package.hash,
                "same plugin version has different package hash; increment the version",
            )?;
        }
        self.packages.insert(
            (
                package.manifest.id.clone(),
                package.manifest.version.clone(),
                package.hash.clone(),
            ),
            package,
        );
        Ok(())
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let mut r = Self::new_with_builtins()?;
        // Persist each bundled version before an App upgrade replaces its bytes.
        let builtins = crate::builtin::packages()?;
        fs::create_dir_all(directory)?;
        for builtin in &builtins {
            let bundled = directory.join(format!("{}.msfx", builtin.hash));
            if !bundled.exists() {
                let temp = directory.join(format!("{}.tmp", builtin.hash));
                let mut file = File::create(&temp)?;
                file.write_all(&builtin.bytes)?;
                file.sync_all()?;
                drop(file);
                fs::rename(temp, &bundled)?;
            }
        }
        if directory.exists() {
            for e in fs::read_dir(directory)? {
                let e = e?;
                if e.file_type()?.is_file() && e.path().extension().is_some_and(|v| v == "msfx") {
                    let cached = builtins
                        .iter()
                        .find(|p| e.file_name().to_string_lossy() == format!("{}.msfx", p.hash));
                    let package = if let Some(builtin) = cached {
                        // Reuse the already validated embedded package on repeated session creation.
                        fs::read(e.path())
                            .and_then(|bytes| {
                                if bytes == builtin.bytes {
                                    Ok(builtin.clone())
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "bundled package cache is corrupt",
                                    ))
                                }
                            })
                            .map_err(Error::Io)
                    } else {
                        EffectPackage::open(&e.path()).map(Arc::new)
                    };
                    match package.and_then(|p| r.insert(p)) {
                        Ok(()) => {}
                        Err(e) => r.diagnostics.push(e.to_string()),
                    }
                }
            }
        }
        if directory.join("disabled.json").exists() {
            r.disabled = serde_json::from_slice(&fs::read(directory.join("disabled.json"))?)?;
        }
        Ok(r)
    }
    pub fn resolve(&self, id: &str, version: &str, hash: &str) -> Result<Arc<EffectPackage>> {
        let key = (id.to_owned(), version.to_owned(), hash.to_owned());
        ensure(
            !self.disabled.contains(&key),
            format!("plugin {id} {version} is disabled"),
        )?;
        self.packages
            .get(&key)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("missing plugin {id} {version} ({hash})")))
    }
    pub fn install(&mut self, directory: &Path, path: &Path) -> Result<InstalledPlugin> {
        let package = Arc::new(EffectPackage::open(path)?);
        let mut next = self.clone();
        // Another session may have installed a version since this registry was opened.
        for installed in Self::load(directory)?.packages.into_values() {
            next.insert(installed)?;
        }
        next.insert(package.clone())?;
        fs::create_dir_all(directory)?;
        let destination = directory.join(format!("{}.msfx", package.hash));
        if !destination.exists() {
            let temp = directory.join(format!("{}.tmp", package.hash));
            let mut f = File::create(&temp)?;
            f.write_all(&package.bytes)?;
            f.sync_all()?;
            drop(f);
            fs::rename(temp, &destination)?;
        }
        let info = InstalledPlugin {
            id: package.manifest.id.clone(),
            version: package.manifest.version.clone(),
            hash: package.hash.clone(),
            enabled: !next.disabled.contains(&(
                package.manifest.id.clone(),
                package.manifest.version.clone(),
                package.hash.clone(),
            )),
        };
        *self = next;
        Ok(info)
    }
    pub fn enable(
        &mut self,
        directory: &Path,
        id: &str,
        version: &str,
        hash: &str,
        enabled: bool,
    ) -> Result<()> {
        let key = (id.into(), version.into(), hash.into());
        ensure(self.packages.contains_key(&key), "plugin does not exist")?;
        let mut disabled = self.disabled.clone();
        if enabled {
            disabled.remove(&key);
        } else {
            disabled.insert(key);
        }
        fs::create_dir_all(directory)?;
        let temp = directory.join("disabled.json.tmp");
        fs::write(&temp, serde_json::to_vec(&disabled)?)?;
        fs::rename(temp, directory.join("disabled.json"))?;
        self.disabled = disabled;
        Ok(())
    }
    pub fn uninstall(
        &mut self,
        directory: &Path,
        id: &str,
        version: &str,
        hash: &str,
    ) -> Result<()> {
        ensure(
            id != crate::builtin::PLUGIN_ID,
            "the preinstalled core library cannot be uninstalled",
        )?;
        let key = (id.into(), version.into(), hash.into());
        ensure(self.packages.contains_key(&key), "plugin does not exist")?;
        let path: PathBuf = directory.join(format!("{hash}.msfx"));
        ensure(
            hash.len() == 64 && hash.bytes().all(|v| v.is_ascii_hexdigit()),
            "invalid package hash",
        )?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        self.enable(directory, id, version, hash, true)?;
        self.packages.remove(&key);
        Ok(())
    }
}
