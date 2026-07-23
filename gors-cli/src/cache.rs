use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::runtime_descriptor::{
    RuntimeDependencyDescriptor, RuntimeDescriptorError, RuntimeLinkDescriptor,
};

mod identity;
mod output_manifest;

pub use identity::{GeneratedRustIdentity, GeneratedRustIdentityOptions};
pub use output_manifest::GeneratedOutputManifest;

const CACHE_MANIFEST_FILENAME: &str = ".gors_cli_cache.json";
const CACHE_MANIFEST_VERSION: u32 = 5;
const TERMINAL_MANIFEST_FILENAME: &str = ".gors_cli_terminal.json";
const TERMINAL_MANIFEST_VERSION: u32 = 6;
const CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 256;
const CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const CACHE_ACCESS_PERSIST_INTERVAL: Duration = Duration::from_secs(60 * 60);
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const CACHE_ACCESS_LOCK_FILENAME: &str = ".gors-cache.lock";
const CACHE_PRUNE_MARKER_FILENAME: &str = ".last-prune";

pub struct CacheAccessLock {
    _file: std::fs::File,
}

/// Immutable source-admission record derived from one `LoadedProgram`.
///
/// Cache comparison consumes this value directly and never rereads its source
/// paths. A cache miss compiles the same loaded revision that produced it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputSnapshot {
    files: BTreeMap<String, String>,
    directories: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileArtifact {
    path: String,
    content_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CliCacheManifest {
    version: u32,
    generated_identity: String,
    inputs: InputSnapshot,
    generated_files: BTreeMap<String, String>,
    sourcemap: Option<FileArtifact>,
    runtime_dependency: RuntimeDependencyDescriptor,
    #[serde(skip)]
    terminal: Option<TerminalState>,
    last_used_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExecutableArtifact {
    path: String,
    content_hash: String,
    size_bytes: u64,
    mode: u32,
    rustc_action_identity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TerminalState {
    version: u32,
    generated_identity: String,
    runtime: RuntimeLinkDescriptor,
    artifact_path: String,
    toolchain: crate::rustc::TerminalToolchain,
    executables: BTreeMap<String, ExecutableArtifact>,
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
        loaded: &gors::workspace::LoadedProgram,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut files = BTreeMap::new();
        for package in loaded.input().packages() {
            for file in package.files() {
                let snapshot = file.snapshot();
                files.insert(
                    normalized_path(Path::new(snapshot.diagnostic_path()))?,
                    hex_digest(snapshot.content_digest()),
                );
            }
        }

        let mut directories = BTreeMap::new();
        for directory in loaded.watched_directories() {
            let directory = normalized_path(directory)?;
            let mut selected = files
                .keys()
                .filter(|path| Path::new(path).parent() == Some(Path::new(&directory)))
                .cloned()
                .collect::<Vec<_>>();
            selected.sort();
            directories.insert(directory, selected);
        }

        Ok(Self { files, directories })
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
        identity: &GeneratedRustIdentity,
        inputs: InputSnapshot,
        generated_files: BTreeMap<String, String>,
        sourcemap: Option<FileArtifact>,
        runtime_dependency: &gors_runtime_abi::RuntimeDependency,
    ) -> Self {
        Self {
            version: CACHE_MANIFEST_VERSION,
            generated_identity: identity.fingerprint().to_string(),
            inputs,
            generated_files,
            sourcemap,
            runtime_dependency: RuntimeDependencyDescriptor::from_dependency(runtime_dependency),
            terminal: None,
            last_used_unix_ms: unix_time_ms(),
        }
    }

    /// Admit cache metadata for an executable check without touching generated
    /// Rust files.
    ///
    /// The caller must supply the snapshot captured during this invocation's
    /// source-load phase. The generated and terminal manifests are validated
    /// and cross-checked, but their recorded Rust files are deliberately not
    /// opened: a verified executable does not consume those intermediates.
    pub fn load_if_source_revision_matches(
        output_dir: &Path,
        identity: &GeneratedRustIdentity,
        current_inputs: &InputSnapshot,
    ) -> Option<Self> {
        let content = std::fs::read(output_dir.join(CACHE_MANIFEST_FILENAME)).ok()?;
        let mut manifest: Self = serde_json::from_slice(&content).ok()?;
        if manifest.version != CACHE_MANIFEST_VERSION
            || manifest.generated_identity != identity.fingerprint()
            || &manifest.inputs != current_inputs
        {
            return None;
        }

        let output_manifest = GeneratedOutputManifest::load(output_dir)?;
        let output_dependency = output_manifest.runtime_dependency().ok()?;
        let cached_dependency = manifest.runtime_dependency().ok()?;
        if output_dependency != cached_dependency {
            return None;
        }
        if output_manifest.len() != manifest.generated_files.len() {
            return None;
        }
        for (filename, content_hash) in &manifest.generated_files {
            if !output_manifest.matches(filename, content_hash) {
                return None;
            }
        }

        manifest.terminal = TerminalState::load_if_valid(
            output_dir,
            &manifest.generated_identity,
            &cached_dependency,
        );
        let now = unix_time_ms();
        let persist_interval = CACHE_ACCESS_PERSIST_INTERVAL
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let should_persist_access =
            now.saturating_sub(manifest.last_used_unix_ms) >= persist_interval;
        manifest.last_used_unix_ms = now;
        if should_persist_access {
            // Access accounting is pruning metadata, never cache admission.
            // A read-only or transiently unavailable cache must not turn an
            // otherwise exact executable hit into compilation work.
            drop(manifest.save(output_dir));
        }
        Some(manifest)
    }

    /// Load reusable generated Rust after validating every recorded source
    /// artifact. Source-emitting commands and terminal misses use this stricter
    /// path before reading or relinking generated files.
    #[cfg(test)]
    pub fn load_if_generated_valid(
        output_dir: &Path,
        identity: &GeneratedRustIdentity,
        current_inputs: &InputSnapshot,
    ) -> Option<Self> {
        let manifest = Self::load_if_source_revision_matches(output_dir, identity, current_inputs)?;
        manifest
            .generated_files_are_current(output_dir)
            .then_some(manifest)
    }

    pub fn runtime_dependency(
        &self,
    ) -> Result<gors_runtime_abi::RuntimeDependency, RuntimeDescriptorError> {
        self.runtime_dependency.reconstruct()
    }

    #[cfg(test)]
    pub fn runtime(&self) -> Option<&RuntimeLinkDescriptor> {
        self.terminal.as_ref().map(|terminal| &terminal.runtime)
    }

    pub fn refresh_runtime(
        &mut self,
        runtime: &RuntimeLinkDescriptor,
        artifact_path: &Path,
        toolchain: &crate::rustc::TerminalToolchain,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !artifact_path.is_absolute() {
            return Err(std::io::Error::other(format!(
                "terminal runtime artifact path is not absolute: {}",
                artifact_path.display()
            ))
            .into());
        }
        let artifact_path = artifact_path.to_str().ok_or_else(|| {
            std::io::Error::other(format!(
                "terminal runtime artifact path is not valid UTF-8: {}",
                artifact_path.display()
            ))
        })?;
        if !toolchain.is_canonical() || toolchain.target() != runtime.target_triple() {
            return Err(std::io::Error::other(
                "terminal toolchain is not canonical for the selected runtime target",
            )
            .into());
        }
        match &mut self.terminal {
            Some(terminal) => {
                terminal.runtime = runtime.clone();
                terminal.artifact_path = artifact_path.to_string();
                terminal.toolchain = toolchain.clone();
            }
            None => {
                self.terminal = Some(TerminalState {
                    version: TERMINAL_MANIFEST_VERSION,
                    generated_identity: self.generated_identity.clone(),
                    runtime: runtime.clone(),
                    artifact_path: artifact_path.to_string(),
                    toolchain: toolchain.clone(),
                    executables: BTreeMap::new(),
                });
            }
        }
        self.last_used_unix_ms = unix_time_ms();
        Ok(())
    }

    pub fn selected_runtime(&self) -> Option<&RuntimeLinkDescriptor> {
        self.terminal.as_ref().map(|terminal| &terminal.runtime)
    }

    pub fn selected_artifact_path(&self) -> Option<&Path> {
        self.terminal
            .as_ref()
            .map(|terminal| Path::new(&terminal.artifact_path))
    }

    pub fn selected_terminal_toolchain(&self) -> Option<&crate::rustc::TerminalToolchain> {
        self.terminal.as_ref().map(|terminal| &terminal.toolchain)
    }

    pub fn admit_executable(
        &self,
        profile: crate::rustc::RustcProfile,
        expected_path: &Path,
        action: &crate::rustc::RustcAction,
    ) -> Option<crate::rustc::ExecutableProduct> {
        let Ok(normalized_expected_path) = normalized_path(expected_path) else {
            return None;
        };
        let artifact = self
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.executables.get(profile.label()))?;
        if artifact.path != normalized_expected_path
            || artifact.rustc_action_identity != action.identity().to_string()
        {
            return None;
        }
        let product = crate::rustc::ExecutableProduct::admit(expected_path).ok()?;
        (product.content_hash() == artifact.content_hash
            && product.size_bytes() == artifact.size_bytes
            && product.mode() == artifact.mode)
            .then_some(product)
    }

    pub fn generated_files_are_current(&self, output_dir: &Path) -> bool {
        generated_files_are_current(output_dir, &self.generated_files)
    }

    pub fn generated_files(&self) -> &BTreeMap<String, String> {
        &self.generated_files
    }

    pub fn set_executable(
        &mut self,
        profile: crate::rustc::RustcProfile,
        executable: &crate::rustc::ExecutableProduct,
        action: &crate::rustc::RustcAction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let terminal = self
            .terminal
            .as_mut()
            .ok_or("cannot record an executable before selecting a runtime")?;
        terminal.executables.insert(
            profile.label().to_string(),
            ExecutableArtifact {
                path: normalized_path(executable.path())?,
                content_hash: executable.content_hash().to_string(),
                size_bytes: executable.size_bytes(),
                mode: executable.mode(),
                rustc_action_identity: action.identity().to_string(),
            },
        );
        self.last_used_unix_ms = unix_time_ms();
        Ok(())
    }

    /// Reuse a presentation-only source map without changing generated-Rust
    /// identity. Returns false when no valid map bytes are available.
    pub fn reuse_sourcemap(&mut self, destination: &Path) -> Result<bool, std::io::Error> {
        let Some(current) = self.sourcemap.as_ref() else {
            return Ok(false);
        };
        if !current.is_current() {
            return Ok(false);
        }
        let destination = normalized_path(destination)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if current.path == destination {
            return Ok(true);
        }
        let bytes = std::fs::read(&current.path)?;
        let parent = Path::new(&destination)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write as _;
        temporary.write_all(&bytes)?;
        temporary.as_file_mut().sync_all()?;
        temporary
            .persist(&destination)
            .map_err(|error| error.error)?;
        self.sourcemap = Some(
            FileArtifact::capture(Path::new(&destination))
                .map_err(|error| std::io::Error::other(error.to_string()))?,
        );
        self.last_used_unix_ms = unix_time_ms();
        Ok(true)
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

    pub fn save_terminal(&self, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let Some(terminal) = &self.terminal else {
            return Ok(());
        };
        std::fs::create_dir_all(output_dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(output_dir)?;
        serde_json::to_writer_pretty(temp.as_file_mut(), terminal)?;
        use std::io::Write as _;
        temp.as_file_mut().write_all(b"\n")?;
        temp.as_file_mut().sync_all()?;
        temp.persist(output_dir.join(TERMINAL_MANIFEST_FILENAME))
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        std::fs::File::open(output_dir)?.sync_all()?;
        Ok(())
    }

    #[cfg(test)]
    fn with_last_used(mut self, last_used_unix_ms: u64) -> Self {
        self.last_used_unix_ms = last_used_unix_ms;
        self
    }
}

impl TerminalState {
    fn load_if_valid(
        output_dir: &Path,
        generated_identity: &str,
        generated_dependency: &gors_runtime_abi::RuntimeDependency,
    ) -> Option<Self> {
        let content = std::fs::read(output_dir.join(TERMINAL_MANIFEST_FILENAME)).ok()?;
        let terminal: Self = serde_json::from_slice(&content).ok()?;
        let selected_dependency = terminal.runtime.reconstruct_dependency().ok()?;
        if terminal.version != TERMINAL_MANIFEST_VERSION
            || terminal.generated_identity != generated_identity
            || &selected_dependency != generated_dependency
            || !terminal.runtime.is_canonical()
            || !Path::new(&terminal.artifact_path).is_absolute()
            || !terminal.toolchain.is_canonical()
            || terminal.toolchain.target() != terminal.runtime.target_triple()
            || !terminal.executables.iter().all(|(profile, executable)| {
                matches!(profile.as_str(), "development" | "production")
                    && executable.is_canonical()
            })
        {
            return None;
        }
        Some(terminal)
    }
}

impl ExecutableArtifact {
    fn is_canonical(&self) -> bool {
        Path::new(&self.path).is_absolute()
            && is_sha256(&self.content_hash)
            && self.size_bytes > 0
            && self.mode == crate::rustc::ExecutableProduct::canonical_mode()
            && is_sha256(&self.rustc_action_identity)
    }
}

pub fn generated_file_hashes(output: &gors::printer::GeneratedOutput) -> BTreeMap<String, String> {
    output
        .files
        .iter()
        .map(|(filename, source)| (filename.clone(), sha2_hash(source.as_bytes())))
        .collect()
}

/// Publish a newly selected terminal provider without changing the reusable
/// target-neutral generated-Rust product.
pub fn refresh_runtime_selection(
    output_dir: &Path,
    manifest: &mut CliCacheManifest,
    runtime: &RuntimeLinkDescriptor,
    artifact_path: &Path,
    toolchain: &crate::rustc::TerminalToolchain,
) -> Result<(), Box<dyn std::error::Error>> {
    manifest.refresh_runtime(runtime, artifact_path, toolchain)?;
    manifest.save_terminal(output_dir)
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
    for category in ["programs"] {
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
mod tests;
