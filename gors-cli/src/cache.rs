use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_MANIFEST_FILENAME: &str = ".gors_cli_cache.json";
const CACHE_MANIFEST_VERSION: u32 = 3;
const CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 256;
const CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const CACHE_ACCESS_LOCK_FILENAME: &str = ".gors-cache.lock";
const CACHE_PRUNE_MARKER_FILENAME: &str = ".last-prune";
const RESOLVER_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;
const RESOLVER_CACHE_FILENAME: &str = "resolved-modules.json";

pub struct CacheAccessLock {
    _file: std::fs::File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheRequest {
    fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InputSnapshot {
    files: BTreeMap<String, String>,
    directories: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileArtifact {
    path: String,
    content_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CliCacheManifest {
    version: u32,
    request_fingerprint: String,
    inputs: InputSnapshot,
    generated_files: BTreeMap<String, String>,
    sourcemap: Option<FileArtifact>,
    executable: Option<FileArtifact>,
    last_used_unix_ms: u64,
}

impl CacheRequest {
    pub fn new(options: CacheRequestOptions<'_>) -> Result<Self, Box<dyn std::error::Error>> {
        let gorspath = std::env::var_os("GORSPATH");
        Self::new_with_identity(
            options,
            env!("GORS_CLI_ABI_FINGERPRINT"),
            gorspath.as_deref(),
        )
    }

    fn new_with_identity(
        options: CacheRequestOptions<'_>,
        cli_abi_fingerprint: &str,
        gorspath: Option<&OsStr>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hash_part(&mut hasher, b"gors-cli-cache-request-v3");
        hash_part(&mut hasher, options.command.as_bytes());
        hash_part(
            &mut hasher,
            if options.release {
                b"release"
            } else {
                b"debug"
            },
        );
        hash_part(&mut hasher, env!("CARGO_PKG_VERSION").as_bytes());
        hash_part(&mut hasher, gors::GO_VERSION.as_bytes());
        hash_part(&mut hasher, gors::STDLIB_VERSION.as_bytes());
        hash_part(&mut hasher, gors::COMPILER_FINGERPRINT.as_bytes());
        hash_part(&mut hasher, cli_abi_fingerprint.as_bytes());
        hash_part(&mut hasher, super::RUST_TOOLCHAIN.as_bytes());
        hash_part(&mut hasher, super::RUST_EDITION.as_bytes());
        hash_part(&mut hasher, std::env::consts::OS.as_bytes());
        hash_part(&mut hasher, std::env::consts::ARCH.as_bytes());
        hash_gorspath(&mut hasher, gorspath);

        // Worker count changes scheduling only. It must not fragment semantic
        // cache entries when deterministic compilation produces the same output.
        let _operational_jobs = options.jobs;

        for source_path in options.source_paths {
            hash_part(
                &mut hasher,
                normalized_path(Path::new(source_path))?.as_bytes(),
            );
            hash_part(
                &mut hasher,
                module_context(Path::new(source_path))?.as_bytes(),
            );
        }
        if let Some(output) = options.output {
            hash_part(&mut hasher, normalized_path(output)?.as_bytes());
        }
        if let Some(sourcemap) = options.sourcemap {
            hash_part(&mut hasher, normalized_path(sourcemap)?.as_bytes());
        }

        Ok(Self {
            fingerprint: hex_digest(hasher.finalize()),
        })
    }
}

pub struct CacheRequestOptions<'a> {
    pub command: &'static str,
    pub source_paths: &'a [String],
    pub release: bool,
    pub output: Option<&'a Path>,
    pub sourcemap: Option<&'a Path>,
    pub jobs: usize,
}

impl CacheAccessLock {
    pub fn acquire_shared(cache_base: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = Self::open(cache_base)?;
        file.lock_shared()?;
        Ok(Self { _file: file })
    }

    fn acquire_exclusive(cache_base: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = Self::open(cache_base)?;
        file.lock()?;
        Ok(Self { _file: file })
    }

    fn open(cache_base: &Path) -> Result<std::fs::File, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(cache_base)?;
        Ok(std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(cache_base.join(CACHE_ACCESS_LOCK_FILENAME))?)
    }
}

impl InputSnapshot {
    pub fn capture(
        program: &gors::parser::ParsedProgram,
        invocation_sources: &[String],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut files = BTreeMap::new();
        for (path, source) in &program.main_package.files {
            files.insert(
                normalized_path(Path::new(path))?,
                sha2_hash(source.as_bytes()),
            );
        }
        for package in &program.imports {
            for (path, source) in &package.files {
                files.insert(
                    normalized_path(Path::new(path))?,
                    sha2_hash(source.as_bytes()),
                );
            }
        }

        let mut directory_paths = BTreeSet::new();
        for package in &program.imports {
            for (path, _) in &package.files {
                if let Some(parent) = Path::new(path).parent() {
                    directory_paths.insert(normalized_path(parent)?);
                }
            }
        }
        if invocation_sources.len() == 1 {
            let path = Path::new(
                invocation_sources
                    .first()
                    .map(String::as_str)
                    .unwrap_or_default(),
            );
            if path.is_dir() {
                directory_paths.insert(normalized_path(path)?);
            }
        }

        let mut directories = BTreeMap::new();
        for directory in directory_paths {
            directories.insert(directory.clone(), eligible_go_files(Path::new(&directory))?);
        }

        Ok(Self { files, directories })
    }

    pub fn is_current(&self) -> bool {
        for (path, expected_hash) in &self.files {
            let Ok(source) = std::fs::read(path) else {
                return false;
            };
            if sha2_hash(&source) != *expected_hash {
                return false;
            }
        }
        for (path, expected_files) in &self.directories {
            let Ok(actual_files) = eligible_go_files(Path::new(path)) else {
                return false;
            };
            if actual_files != *expected_files {
                return false;
            }
        }
        true
    }
}

impl FileArtifact {
    pub fn capture(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            path: normalized_path(path)?,
            content_hash: file_hash(path)?,
        })
    }

    fn is_current(&self) -> bool {
        file_hash(Path::new(&self.path)).is_ok_and(|hash| hash == self.content_hash)
    }
}

impl CliCacheManifest {
    pub fn new(
        request: &CacheRequest,
        inputs: InputSnapshot,
        generated_files: BTreeMap<String, String>,
        sourcemap: Option<FileArtifact>,
    ) -> Self {
        Self {
            version: CACHE_MANIFEST_VERSION,
            request_fingerprint: request.fingerprint.clone(),
            inputs,
            generated_files,
            sourcemap,
            executable: None,
            last_used_unix_ms: unix_time_ms(),
        }
    }

    pub fn load_if_generated_valid(output_dir: &Path, request: &CacheRequest) -> Option<Self> {
        let content = std::fs::read(output_dir.join(CACHE_MANIFEST_FILENAME)).ok()?;
        let mut manifest: Self = serde_json::from_slice(&content).ok()?;
        if manifest.version != CACHE_MANIFEST_VERSION
            || manifest.request_fingerprint != request.fingerprint
            || !manifest.inputs.is_current()
            || !generated_files_are_current(output_dir, &manifest.generated_files)
            || manifest
                .sourcemap
                .as_ref()
                .is_some_and(|artifact| !artifact.is_current())
        {
            return None;
        }

        let build_manifest = gors::compiler::manifest::BuildManifest::load(output_dir)?;
        if build_manifest.modules.len() != manifest.generated_files.len() {
            return None;
        }
        for (filename, content_hash) in &manifest.generated_files {
            let entry = build_manifest.modules.get(filename)?;
            if entry.output_file != *filename
                || build_manifest.needs_recompile(filename, content_hash)
            {
                return None;
            }
        }

        manifest.last_used_unix_ms = unix_time_ms();
        if manifest.save(output_dir).is_err() {
            return None;
        }
        Some(manifest)
    }

    pub fn executable_is_valid(&self, expected_path: &Path) -> bool {
        let Ok(expected_path) = normalized_path(expected_path) else {
            return false;
        };
        self.executable
            .as_ref()
            .is_some_and(|artifact| artifact.path == expected_path && artifact.is_current())
    }

    pub fn generated_file_count(&self) -> usize {
        self.generated_files.len()
    }

    pub fn set_executable(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.executable = Some(FileArtifact::capture(path)?);
        self.last_used_unix_ms = unix_time_ms();
        Ok(())
    }

    pub fn save(&self, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(output_dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(output_dir)?;
        serde_json::to_writer_pretty(temp.as_file_mut(), self)?;
        use std::io::Write as _;
        temp.as_file_mut().write_all(b"\n")?;
        temp.as_file_mut().sync_all()?;
        temp.persist(output_dir.join(CACHE_MANIFEST_FILENAME))
            .map_err(|error| error.error)?;
        Ok(())
    }

    #[cfg(test)]
    fn with_last_used(mut self, last_used_unix_ms: u64) -> Self {
        self.last_used_unix_ms = last_used_unix_ms;
        self
    }
}

pub fn generated_file_hashes(output: &gors::printer::GeneratedOutput) -> BTreeMap<String, String> {
    output
        .files
        .iter()
        .map(|(filename, source)| (filename.clone(), sha2_hash(source.as_bytes())))
        .collect()
}

pub fn remove_legacy_incremental(cache_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let legacy = cache_dir.join("rustc-incremental");
    if legacy.is_dir() {
        std::fs::remove_dir_all(legacy)?;
    } else if legacy.exists() {
        std::fs::remove_file(legacy)?;
    }
    Ok(())
}

pub fn import_resolver_cache(cache_base: &Path) -> bool {
    let path = resolver_cache_path(cache_base);
    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    if bytes.len() > RESOLVER_CACHE_MAX_BYTES
        || gors::resolve::import_resolved_module_cache(&bytes).is_err()
    {
        let _ = std::fs::remove_file(path);
        return false;
    }
    true
}

pub fn export_resolver_cache(cache_base: &Path) -> bool {
    let Ok(bytes) = gors::resolve::export_resolved_module_cache() else {
        return false;
    };
    if bytes.len() > RESOLVER_CACHE_MAX_BYTES {
        return false;
    }
    let directory = cache_base.join("resolver").join("current");
    if std::fs::create_dir_all(&directory).is_err() {
        return false;
    }
    let Ok(mut temp) = tempfile::NamedTempFile::new_in(&directory) else {
        return false;
    };
    use std::io::Write as _;
    if temp.as_file_mut().write_all(&bytes).is_err()
        || temp.as_file_mut().sync_all().is_err()
        || temp
            .persist(directory.join(RESOLVER_CACHE_FILENAME))
            .is_err()
    {
        return false;
    }
    true
}

pub fn maybe_prune_cli_cache(
    cache_base: &Path,
    keep: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    maybe_prune_cli_cache_inner(cache_base, keep, || {})
}

fn maybe_prune_cli_cache_inner(
    cache_base: &Path,
    keep: Option<&Path>,
    before_exclusive_lock: impl FnOnce(),
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(cache_base)?;
    let marker = cache_base.join(CACHE_PRUNE_MARKER_FILENAME);
    if prune_marker_is_recent(&marker) {
        return Ok(());
    }

    before_exclusive_lock();
    let _cache_lock = CacheAccessLock::acquire_exclusive(cache_base)?;
    if prune_marker_is_recent(&marker) {
        return Ok(());
    }

    prune_cli_cache(cache_base, keep, SystemTime::now())?;
    std::fs::write(marker, unix_time_ms().to_string())?;
    Ok(())
}

fn prune_marker_is_recent(marker: &Path) -> bool {
    marker
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|elapsed| elapsed < PRUNE_INTERVAL)
}

fn prune_cli_cache(
    cache_base: &Path,
    keep: Option<&Path>,
    now: SystemTime,
) -> Result<(), Box<dyn std::error::Error>> {
    let keep = keep.map(normalized_path).transpose()?;
    let mut entries = Vec::new();
    for category in ["build", "resolver", "run"] {
        let category_path = cache_base.join(category);
        let Ok(children) = std::fs::read_dir(category_path) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if !path.is_dir() {
                continue;
            }
            let normalized = normalized_path(&path)?;
            let manifest = std::fs::read(path.join(CACHE_MANIFEST_FILENAME))
                .ok()
                .and_then(|content| serde_json::from_slice::<CliCacheManifest>(&content).ok());
            let last_used_unix_ms = manifest.as_ref().map_or_else(
                || modified_unix_ms(&path),
                |manifest| manifest.last_used_unix_ms,
            );
            entries.push(CacheEntry {
                path,
                normalized,
                size: directory_size(&child.path())?,
                last_used_unix_ms,
            });
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_used_unix_ms));
    let now_ms = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    let max_age_ms: u64 = CACHE_MAX_AGE.as_millis().try_into().unwrap_or(u64::MAX);
    let mut retained_bytes = 0_u64;
    let mut retained_entries = 0_usize;

    for entry in entries {
        let is_keep = keep
            .as_ref()
            .is_some_and(|keep_path| *keep_path == entry.normalized);
        let expired = now_ms.saturating_sub(entry.last_used_unix_ms) > max_age_ms;
        let exceeds_count = retained_entries >= CACHE_MAX_ENTRIES;
        let exceeds_bytes = retained_bytes.saturating_add(entry.size) > CACHE_MAX_BYTES;
        if !is_keep && (expired || exceeds_count || exceeds_bytes) {
            std::fs::remove_dir_all(entry.path)?;
            continue;
        }
        retained_bytes = retained_bytes.saturating_add(entry.size);
        retained_entries = retained_entries.saturating_add(1);
    }
    Ok(())
}

fn resolver_cache_path(cache_base: &Path) -> PathBuf {
    cache_base
        .join("resolver")
        .join("current")
        .join(RESOLVER_CACHE_FILENAME)
}

struct CacheEntry {
    path: PathBuf,
    normalized: String,
    size: u64,
    last_used_unix_ms: u64,
}

fn generated_files_are_current(output_dir: &Path, expected: &BTreeMap<String, String>) -> bool {
    for (filename, expected_hash) in expected {
        let path = output_dir.join(filename);
        if !file_hash(&path).is_ok_and(|hash| hash == *expected_hash) {
            return false;
        }
    }

    let Ok(entries) = std::fs::read_dir(output_dir) else {
        return false;
    };
    let actual_rust_files: BTreeSet<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_file() && path.extension().is_some_and(|extension| extension == "rs"))
                .then(|| path.file_name()?.to_str().map(ToString::to_string))
                .flatten()
        })
        .collect();
    let expected_rust_files: BTreeSet<String> = expected
        .keys()
        .filter(|filename| {
            Path::new(filename)
                .extension()
                .is_some_and(|extension| extension == "rs")
        })
        .cloned()
        .collect();
    actual_rust_files == expected_rust_files
}

fn eligible_go_files(directory: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_file()
            && filename.ends_with(".go")
            && !filename.ends_with("_test.go")
            && !filename.starts_with('.')
            && !filename.starts_with('_')
        {
            files.push(normalized_path(&path)?);
        }
    }
    files.sort();
    Ok(files)
}

fn module_context(source_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut directory = if source_path.is_dir() {
        source_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    if directory.is_relative() {
        directory = std::env::current_dir()?.join(directory);
    }
    loop {
        let go_mod = directory.join("go.mod");
        if go_mod.is_file() {
            return Ok(format!(
                "{}:{}",
                normalized_path(&go_mod)?,
                file_hash(&go_mod)?
            ));
        }
        if !directory.pop() {
            return Ok("no-go-mod".to_string());
        }
    }
}

fn hash_gorspath(hasher: &mut Sha256, gorspath: Option<&OsStr>) {
    hash_part(hasher, b"gorspath");
    let Some(gorspath) = gorspath else {
        hash_part(hasher, b"unset");
        return;
    };

    hash_part(hasher, b"set");
    hash_os_part(hasher, gorspath);
    for (index, root) in std::env::split_paths(gorspath).enumerate() {
        hash_part(
            hasher,
            &u64::try_from(index).unwrap_or(u64::MAX).to_le_bytes(),
        );
        hash_gorspath_root(hasher, &root);
    }
}

fn hash_gorspath_root(hasher: &mut Sha256, root: &Path) {
    hash_part(hasher, b"root");
    hash_os_part(hasher, root.as_os_str());
    if root.as_os_str().is_empty() {
        hash_part(hasher, b"empty");
        return;
    }

    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(current_dir) => current_dir.join(root),
            Err(error) => {
                hash_io_error(hasher, b"current-directory-error", root, &error);
                return;
            }
        }
    };
    let canonical = match std::fs::canonicalize(&absolute) {
        Ok(canonical) => canonical,
        Err(error) => {
            hash_io_error(hasher, b"missing-or-inaccessible-root", &absolute, &error);
            return;
        }
    };
    hash_part(hasher, b"canonical");
    hash_os_part(hasher, canonical.as_os_str());

    if canonical.is_file() {
        hash_part(hasher, b"file");
    } else if canonical.is_dir() {
        // GORSPATH config identity belongs in the pre-parse lookup key. Exact
        // resolved source contents and eligible directory membership are
        // validated by InputSnapshot. Recursively reading every possible Go
        // file here would make even a warm cache hit O(the entire search tree).
        hash_part(hasher, b"directory");
    } else {
        hash_part(hasher, b"unsupported-root-kind");
    }
}

fn hash_io_error(hasher: &mut Sha256, marker: &[u8], path: &Path, error: &std::io::Error) {
    hash_part(hasher, marker);
    hash_os_part(hasher, path.as_os_str());
    hash_part(hasher, format!("{:?}", error.kind()).as_bytes());
}

#[cfg(unix)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::unix::ffi::OsStrExt as _;
    hash_part(hasher, part.as_bytes());
}

#[cfg(windows)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::windows::ffi::OsStrExt as _;
    let bytes = part
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    hash_part(hasher, &bytes);
}

#[cfg(not(any(unix, windows)))]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    hash_part(hasher, part.to_string_lossy().as_bytes());
}

fn normalized_path(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical.to_string_lossy().into_owned());
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.clone();
    let mut missing_components = Vec::new();
    while !ancestor.exists() {
        let Some(component) = ancestor.file_name().map(ToOwned::to_owned) else {
            return Ok(absolute.to_string_lossy().into_owned());
        };
        missing_components.push(component);
        if !ancestor.pop() {
            return Ok(absolute.to_string_lossy().into_owned());
        }
    }
    let mut normalized = std::fs::canonicalize(ancestor)?;
    for component in missing_components.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized.to_string_lossy().into_owned())
}

fn file_hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(sha2_hash(&std::fs::read(path)?))
}

fn sha2_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hex_digest(hasher.finalize())
}

fn hash_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update(part.len().to_le_bytes());
    hasher.update(part);
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn directory_size(path: &Path) -> Result<u64, Box<dyn std::error::Error>> {
    let mut total = 0_u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn modified_unix_ms(path: &Path) -> u64 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_millis().try_into().ok())
        .unwrap_or_default()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn request() -> CacheRequest {
        CacheRequest {
            fingerprint: "request".to_string(),
        }
    }

    fn request_options<'a>(source_paths: &'a [String], jobs: usize) -> CacheRequestOptions<'a> {
        CacheRequestOptions {
            command: "build",
            source_paths,
            release: false,
            output: None,
            sourcemap: None,
            jobs,
        }
    }

    fn write_expired_manifest(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let old_timestamp = unix_time_ms()
            .saturating_sub(CACHE_MAX_AGE.as_millis().try_into().unwrap())
            .saturating_sub(1);
        CliCacheManifest::new(
            &request(),
            InputSnapshot {
                files: BTreeMap::new(),
                directories: BTreeMap::new(),
            },
            BTreeMap::new(),
            None,
        )
        .with_last_used(old_timestamp)
        .save(path)
        .unwrap();
    }

    #[test]
    fn cache_request_tracks_cli_abi_but_not_worker_count() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("main.go");
        std::fs::write(&source, "package main\n").unwrap();
        let source_paths = vec![source.to_string_lossy().into_owned()];

        let one_job =
            CacheRequest::new_with_identity(request_options(&source_paths, 1), "cli-abi-a", None)
                .unwrap();
        let many_jobs =
            CacheRequest::new_with_identity(request_options(&source_paths, 16), "cli-abi-a", None)
                .unwrap();
        let changed_abi =
            CacheRequest::new_with_identity(request_options(&source_paths, 1), "cli-abi-b", None)
                .unwrap();

        assert_eq!(one_job, many_jobs);
        assert_ne!(one_job, changed_abi);
    }

    #[test]
    fn cache_request_tracks_gorspath_identity_without_scanning_root_contents() {
        let invocation = tempfile::tempdir().unwrap();
        let source = invocation.path().join("main.go");
        std::fs::write(&source, "package main\n").unwrap();
        let source_paths = vec![source.to_string_lossy().into_owned()];

        let first_root = tempfile::tempdir().unwrap();
        let package_dir = first_root
            .path()
            .join("src")
            .join("example")
            .join("dependency");
        std::fs::create_dir_all(&package_dir).unwrap();
        let dependency = package_dir.join("dependency.go");
        std::fs::write(&dependency, "package dependency\nconst Value = 1\n").unwrap();
        let first_gorspath = std::env::join_paths([first_root.path()]).unwrap();

        let initial = CacheRequest::new_with_identity(
            request_options(&source_paths, 1),
            "cli-abi",
            Some(first_gorspath.as_os_str()),
        )
        .unwrap();
        let unchanged = CacheRequest::new_with_identity(
            request_options(&source_paths, 1),
            "cli-abi",
            Some(first_gorspath.as_os_str()),
        )
        .unwrap();
        assert_eq!(initial, unchanged);

        std::fs::write(&dependency, "package dependency\nconst Value = 2\n").unwrap();
        let edited = CacheRequest::new_with_identity(
            request_options(&source_paths, 1),
            "cli-abi",
            Some(first_gorspath.as_os_str()),
        )
        .unwrap();
        assert_eq!(initial, edited);

        std::fs::write(package_dir.join("added.go"), "package dependency\n").unwrap();
        let added = CacheRequest::new_with_identity(
            request_options(&source_paths, 1),
            "cli-abi",
            Some(first_gorspath.as_os_str()),
        )
        .unwrap();
        assert_eq!(edited, added);

        let second_root = tempfile::tempdir().unwrap();
        let second_gorspath = std::env::join_paths([second_root.path()]).unwrap();
        let changed_value = CacheRequest::new_with_identity(
            request_options(&source_paths, 1),
            "cli-abi",
            Some(second_gorspath.as_os_str()),
        )
        .unwrap();
        assert_ne!(added, changed_value);
    }

    #[test]
    fn input_snapshot_detects_file_edits_and_directory_membership_changes() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("main.go");
        std::fs::write(&source, "package main\n").unwrap();
        let directory = normalized_path(temp.path()).unwrap();
        let snapshot = InputSnapshot {
            files: std::iter::once((
                normalized_path(&source).unwrap(),
                file_hash(&source).unwrap(),
            ))
            .collect(),
            directories: std::iter::once((directory, vec![normalized_path(&source).unwrap()]))
                .collect(),
        };
        assert!(snapshot.is_current());

        std::fs::write(&source, "package changed\n").unwrap();
        assert!(!snapshot.is_current());
        std::fs::write(&source, "package main\n").unwrap();
        assert!(snapshot.is_current());

        std::fs::write(temp.path().join("added.go"), "package main\n").unwrap();
        assert!(!snapshot.is_current());
    }

    #[test]
    fn normalization_is_stable_before_and_after_output_creation() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("missing").join("output");
        let before = normalized_path(&output).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let after = normalized_path(&output).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn generated_validation_rejects_deleted_modified_and_stale_rust_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let expected = std::iter::once((
            "main.rs".to_string(),
            file_hash(&temp.path().join("main.rs")).unwrap(),
        ))
        .collect();
        assert!(generated_files_are_current(temp.path(), &expected));

        std::fs::write(temp.path().join("main.rs"), "fn changed() {}\n").unwrap();
        assert!(!generated_files_are_current(temp.path(), &expected));
        std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(temp.path().join("stale.rs"), "fn stale() {}\n").unwrap();
        assert!(!generated_files_are_current(temp.path(), &expected));
        std::fs::remove_file(temp.path().join("main.rs")).unwrap();
        assert!(!generated_files_are_current(temp.path(), &expected));
    }

    #[test]
    fn cache_manifest_round_trips_and_validates_executable_content() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("main");
        std::fs::write(&executable, "binary").unwrap();
        let mut manifest = CliCacheManifest::new(
            &request(),
            InputSnapshot {
                files: BTreeMap::new(),
                directories: BTreeMap::new(),
            },
            BTreeMap::new(),
            None,
        );
        manifest.set_executable(&executable).unwrap();
        manifest.save(temp.path()).unwrap();

        let loaded: CliCacheManifest = serde_json::from_slice(
            &std::fs::read(temp.path().join(CACHE_MANIFEST_FILENAME)).unwrap(),
        )
        .unwrap();
        assert!(loaded.executable_is_valid(&executable));
        std::fs::write(&executable, "changed").unwrap();
        assert!(!loaded.executable_is_valid(&executable));
    }

    #[test]
    fn cache_access_lock_allows_shared_users_and_excludes_pruning() {
        let temp = tempfile::tempdir().unwrap();
        let first_shared = CacheAccessLock::acquire_shared(temp.path()).unwrap();

        let shared_base = temp.path().to_path_buf();
        let (second_shared_tx, second_shared_rx) = mpsc::channel();
        let second_shared_thread = std::thread::spawn(move || {
            let lock = CacheAccessLock::acquire_shared(&shared_base).unwrap();
            second_shared_tx.send(lock).unwrap();
        });
        let second_shared = second_shared_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("a second cache user should acquire the shared lock");
        second_shared_thread.join().unwrap();

        let exclusive_base = temp.path().to_path_buf();
        let (exclusive_attempt_tx, exclusive_attempt_rx) = mpsc::channel();
        let (exclusive_tx, exclusive_rx) = mpsc::channel();
        let exclusive_thread = std::thread::spawn(move || {
            exclusive_attempt_tx.send(()).unwrap();
            let lock = CacheAccessLock::acquire_exclusive(&exclusive_base).unwrap();
            exclusive_tx.send(lock).unwrap();
        });
        exclusive_attempt_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("exclusive lock attempt");
        assert!(
            exclusive_rx
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "pruning acquired while shared cache users were active"
        );
        drop(first_shared);
        assert!(
            exclusive_rx
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "pruning acquired before every shared cache user exited"
        );
        drop(second_shared);
        let exclusive = exclusive_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("exclusive lock after shared users exit");
        exclusive_thread.join().unwrap();

        let blocked_shared_base = temp.path().to_path_buf();
        let (shared_attempt_tx, shared_attempt_rx) = mpsc::channel();
        let (blocked_shared_tx, blocked_shared_rx) = mpsc::channel();
        let blocked_shared_thread = std::thread::spawn(move || {
            shared_attempt_tx.send(()).unwrap();
            let lock = CacheAccessLock::acquire_shared(&blocked_shared_base).unwrap();
            blocked_shared_tx.send(lock).unwrap();
        });
        shared_attempt_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shared lock attempt");
        assert!(
            blocked_shared_rx
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "cache user acquired while pruning held the exclusive lock"
        );
        drop(exclusive);
        let shared = blocked_shared_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("shared lock after pruning exits");
        drop(shared);
        blocked_shared_thread.join().unwrap();
    }

    #[test]
    fn pruning_waits_for_active_cache_users_before_removing_entries() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("run").join("old");
        write_expired_manifest(&old);
        let active_user = CacheAccessLock::acquire_shared(temp.path()).unwrap();

        let cache_base = temp.path().to_path_buf();
        let (initial_check_tx, initial_check_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let prune_thread = std::thread::spawn(move || {
            maybe_prune_cli_cache_inner(&cache_base, None, || {
                initial_check_tx.send(()).unwrap();
            })
            .unwrap();
            done_tx.send(()).unwrap();
        });
        initial_check_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("initial prune check");
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "pruning completed while a cache user was active"
        );
        assert!(old.exists());

        drop(active_user);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("pruning after cache user exits");
        prune_thread.join().unwrap();
        assert!(!old.exists());
    }

    #[test]
    fn pruning_rechecks_fresh_marker_after_acquiring_exclusive_lock() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("run").join("old");
        write_expired_manifest(&old);

        let cache_base = temp.path().to_path_buf();
        let marker = temp.path().join(CACHE_PRUNE_MARKER_FILENAME);
        let (initial_check_tx, initial_check_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let prune_thread = std::thread::spawn(move || {
            maybe_prune_cli_cache_inner(&cache_base, None, || {
                initial_check_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
            })
            .unwrap();
        });
        initial_check_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("initial prune check");
        std::fs::write(marker, unix_time_ms().to_string()).unwrap();
        continue_tx.send(()).unwrap();
        prune_thread.join().unwrap();

        assert!(
            old.exists(),
            "a stale pre-lock decision ignored the fresh prune marker"
        );
    }

    #[test]
    fn prune_removes_expired_entries_but_preserves_current_entry() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("run").join("old");
        let keep = temp.path().join("run").join("keep");
        write_expired_manifest(&old);
        write_expired_manifest(&keep);

        prune_cli_cache(temp.path(), Some(&keep), SystemTime::now()).unwrap();
        assert!(!old.exists());
        assert!(keep.exists());
    }
}
