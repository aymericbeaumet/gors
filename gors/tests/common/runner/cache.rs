use super::program_name;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub(super) const RUST_EDITION: &str = gors_runtime_abi::RUST_RUNTIME_EDITION;
const INTEGRATION_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const INTEGRATION_CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const INCOMPLETE_CACHE_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60);
const CACHE_MARKER_SCHEMA: u32 = 1;
const CACHE_MARKER_FILENAME: &str = ".rustc-ok";
const CACHE_ENTRY_LOCK_FILENAME: &str = ".entry.lock";
static PENDING_BINARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct ResolvedRuntime {
    pub(super) path: PathBuf,
    pub(super) link_plan: gors_runtime_abi::LinkPlanIdentity,
}

pub(super) struct FixtureCacheEntry {
    path: PathBuf,
    identity: String,
    _lock: fs::File,
}

impl FixtureCacheEntry {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn acquire(path: PathBuf, identity: String) -> Result<Self, String> {
        loop {
            fs::create_dir_all(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let lock_path = path.join(CACHE_ENTRY_LOCK_FILENAME);
            let lock = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(|error| format!("{}: {error}", lock_path.display()))?;
            lock.lock()
                .map_err(|error| format!("{}: {error}", lock_path.display()))?;

            // A pruning process can remove an old entry after this process
            // opens its lock but before it acquires it. Retry with the new
            // directory and lock inode rather than publishing through an
            // unlinked directory.
            if path.is_dir() && lock_path_still_names_file(&lock, &lock_path)? {
                return Ok(Self {
                    path,
                    identity,
                    _lock: lock,
                });
            }
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeDependencyMarker {
    schema_version: u32,
    contract_identity: String,
    operation_ids: Vec<u16>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheMarker {
    schema_version: u32,
    fixture_cache_identity: String,
    runtime_dependency: RuntimeDependencyMarker,
    link_plan_identity: String,
    binary_sha256: String,
}

impl CacheMarker {
    fn new(
        entry: &FixtureCacheEntry,
        dependency: &gors_runtime_abi::RuntimeDependency,
        link_plan: gors_runtime_abi::LinkPlanIdentity,
        binary_sha256: String,
    ) -> Self {
        Self {
            schema_version: CACHE_MARKER_SCHEMA,
            fixture_cache_identity: entry.identity.clone(),
            runtime_dependency: RuntimeDependencyMarker {
                schema_version: gors_runtime_abi::CURRENT_RUNTIME_DEPENDENCY_SCHEMA,
                contract_identity: dependency.contract().to_string(),
                operation_ids: dependency.requirement().operation_ids().collect(),
            },
            link_plan_identity: link_plan.to_string(),
            binary_sha256,
        }
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let mut bytes = serde_json::to_vec(self)
            .map_err(|error| format!("cannot encode integration cache marker: {error}"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn decode_canonical(bytes: &[u8]) -> Result<Self, String> {
        let marker: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid integration cache marker: {error}"))?;
        if marker.canonical_bytes()? != bytes {
            return Err("integration cache marker is not canonically encoded".to_string());
        }
        Ok(marker)
    }

    fn current_dependency(
        &self,
        expected_fixture_identity: &str,
    ) -> Result<gors_runtime_abi::RuntimeDependency, String> {
        if self.schema_version != CACHE_MARKER_SCHEMA {
            return Err(format!(
                "integration cache marker schema {} is unsupported",
                self.schema_version
            ));
        }
        if self.fixture_cache_identity != expected_fixture_identity
            || !is_lower_hex_sha256(&self.fixture_cache_identity)
        {
            return Err("integration cache marker fixture identity is stale".to_string());
        }
        if self.runtime_dependency.schema_version
            != gors_runtime_abi::CURRENT_RUNTIME_DEPENDENCY_SCHEMA
        {
            return Err("integration cache runtime dependency schema is stale".to_string());
        }

        let manifest = gors_runtime_abi::RuntimeAbiManifest::current();
        if self.runtime_dependency.contract_identity != manifest.identity().to_string()
            || !is_lower_hex_sha256(&self.runtime_dependency.contract_identity)
        {
            return Err("integration cache runtime contract is stale".to_string());
        }
        if !strictly_ascending(&self.runtime_dependency.operation_ids) {
            return Err(
                "integration cache runtime operation IDs are not strictly ascending".to_string(),
            );
        }
        let requirement = gors_runtime_abi::RuntimeRequirement::from_operation_ids(
            self.runtime_dependency.operation_ids.iter().copied(),
        )
        .map_err(|error| format!("integration cache uses {error}"))?;
        if !is_lower_hex_sha256(&self.link_plan_identity)
            || !is_lower_hex_sha256(&self.binary_sha256)
        {
            return Err("integration cache marker contains a malformed identity".to_string());
        }
        gors_runtime_abi::RuntimeDependency::new(&manifest, requirement)
            .map_err(|error| format!("integration cache runtime dependency is invalid: {error}"))
    }
}

pub(super) fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    for (filename, source) in &output.files {
        let path = output_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn locked_fixture_cache_entry(
    fixture_root: &Path,
    fixture_dir: &Path,
) -> Result<FixtureCacheEntry, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"gors-integration-fixture-v3");
    hasher.update(b"\0");
    hasher.update(program_name(fixture_root, fixture_dir).as_bytes());
    hasher.update(b"\0");
    hasher.update(test_binary_fingerprint().as_bytes());
    hasher.update(b"\0");
    hasher.update(rustc_fingerprint().as_bytes());
    hasher.update(b"\0");
    hasher.update(gors::STDLIB_VERSION.as_bytes());
    let provider = gors::artifact::embedded_runtime_artifact();
    hasher.update(provider.manifest().identity().as_bytes());
    hasher.update(provider.manifest().compatibility().as_bytes());
    hasher.update(
        format!(
            "\0rustc-toolchain:{},rustc-flags:edition{RUST_EDITION},deny-unused,overflow-checks-off",
            gors_runtime_abi::NATIVE_RUNTIME_RUST_TOOLCHAIN
        )
        .as_bytes(),
    );
    hasher.update(b"\0");
    let mut input_files = Vec::new();
    collect_regular_files_recursive(fixture_dir, &mut input_files)?;
    input_files.sort();
    for path in input_files {
        let relative = path
            .strip_prefix(fixture_dir)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?);
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    let identity = hex_hash(&digest);
    let dir = integration_cache_root().join(&identity);
    FixtureCacheEntry::acquire(dir, identity)
}

fn integration_cache_root() -> PathBuf {
    workspace_root()
        .join("target")
        .join("gors-integration-run")
        .join("v4")
}

pub(super) fn admit_cached_binary(
    entry: &FixtureCacheEntry,
    binary_path: &Path,
) -> Result<bool, String> {
    if !is_regular_file(binary_path) {
        return Ok(false);
    }
    let marker_path = entry.path.join(CACHE_MARKER_FILENAME);
    let marker_bytes = match fs::read(&marker_path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(false),
    };
    let marker = match CacheMarker::decode_canonical(&marker_bytes).and_then(|marker| {
        marker
            .current_dependency(&entry.identity)
            .map(|dependency| (marker, dependency))
    }) {
        Ok(marker) => marker,
        Err(_) => return Ok(false),
    };
    let (marker, dependency) = marker;
    let runtime = resolve_runtime(dependency)?;
    if marker.link_plan_identity != runtime.link_plan.to_string() {
        return Ok(false);
    }
    let binary_sha256 = match sha256_file(binary_path) {
        Ok(identity) => identity,
        Err(_) => return Ok(false),
    };
    if marker.binary_sha256 != binary_sha256 {
        return Ok(false);
    }

    // Refresh recency through the same crash-safe publication path used for a
    // new marker. An interrupted refresh leaves either complete generation.
    write_marker_atomic(&marker_path, &marker.canonical_bytes()?)?;
    Ok(true)
}

pub(super) fn reserve_pending_binary(entry: &FixtureCacheEntry) -> Result<PathBuf, String> {
    loop {
        let sequence = PENDING_BINARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = entry
            .path
            .join(format!(".main-{}-{sequence}.pending", std::process::id()));
        match fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
}

pub(super) fn discard_pending_binary(path: &Path) {
    drop(fs::remove_file(path));
}

pub(super) fn publish_compiled_binary(
    entry: &FixtureCacheEntry,
    pending_path: &Path,
    binary_path: &Path,
    dependency: &gors_runtime_abi::RuntimeDependency,
    link_plan: gors_runtime_abi::LinkPlanIdentity,
) -> Result<(), String> {
    if pending_path.parent() != Some(entry.path()) || binary_path.parent() != Some(entry.path()) {
        return Err("integration cache publication escaped its locked entry".to_string());
    }

    let pending = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pending_path)
        .map_err(|error| format!("{}: {error}", pending_path.display()))?;
    pending
        .sync_all()
        .map_err(|error| format!("{}: {error}", pending_path.display()))?;
    drop(pending);

    if binary_path.exists()
        && let Err(error) = fs::remove_file(binary_path)
    {
        discard_pending_binary(pending_path);
        return Err(format!(
            "cannot replace stale integration binary {}: {error}",
            binary_path.display()
        ));
    }
    if let Err(error) = fs::rename(pending_path, binary_path) {
        discard_pending_binary(pending_path);
        return Err(format!(
            "cannot atomically publish {}: {error}",
            binary_path.display()
        ));
    }
    sync_directory(entry.path())?;
    let binary_sha256 =
        sha256_file(binary_path).map_err(|error| format!("{}: {error}", binary_path.display()))?;

    let marker = CacheMarker::new(entry, dependency, link_plan, binary_sha256);
    let marker_path = entry.path.join(CACHE_MARKER_FILENAME);
    write_marker_atomic(&marker_path, &marker.canonical_bytes()?)
}

pub(super) fn resolve_runtime(
    dependency: gors_runtime_abi::RuntimeDependency,
) -> Result<ResolvedRuntime, String> {
    use gors_runtime_abi::{RuntimeArtifactFormat, RuntimeLinkRequest};

    let provider = gors::artifact::embedded_runtime_artifact();
    let live_compatibility = integration_rust_compatibility()?;
    if live_compatibility.canonical_bytes() != provider.compatibility().canonical_bytes()
        || live_compatibility.identity() != provider.manifest().compatibility()
    {
        return Err(format!(
            "pinned integration rustc is incompatible with the embedded runtime provider: live identity {}, provider identity {}",
            live_compatibility.identity(),
            provider.manifest().compatibility()
        ));
    }
    let request = RuntimeLinkRequest::new(
        dependency,
        provider.manifest().target().clone(),
        RuntimeArtifactFormat::RustRlibV1,
        live_compatibility.identity(),
    );
    let plan = provider
        .manifest()
        .select(request)
        .map_err(|error| format!("integration rustc cannot link the runtime provider: {error}"))?;
    let path = provider
        .materialize(
            &workspace_root()
                .join("target")
                .join("gors-runtime-artifacts"),
        )
        .map_err(|error| format!("cannot materialize integration runtime provider: {error}"))?;
    Ok(ResolvedRuntime {
        path,
        link_plan: plan.identity(),
    })
}

fn integration_rust_compatibility()
-> Result<&'static gors_runtime_abi::RustRlibCompatibility, String> {
    static COMPATIBILITY: OnceLock<Result<gors_runtime_abi::RustRlibCompatibility, String>> =
        OnceLock::new();
    COMPATIBILITY
        .get_or_init(|| {
            let target = gors::artifact::embedded_runtime_artifact()
                .manifest()
                .target()
                .clone();
            let target_libdir = rustc_target_libdir(target.triple())?;
            let target_libdir_record = gors_runtime_abi::canonical_target_libdir_record(
                &target_libdir,
            )
            .map_err(|error| {
                format!(
                    "cannot inventory pinned rustc target libdir {}: {error}",
                    target_libdir.display()
                )
            })?;
            gors_runtime_abi::RustRlibCompatibility::new(
                rustc_verbose_version()?,
                target_libdir_record,
                target,
            )
            .map_err(|error| format!("cannot identify integration rustc: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn rustc_target_libdir(target: &str) -> Result<PathBuf, String> {
    let output = Command::new("rustup")
        .args([
            "run",
            gors_runtime_abi::NATIVE_RUNTIME_RUST_TOOLCHAIN,
            "rustc",
            "--target",
            target,
            "--print",
            "target-libdir",
        ])
        .output()
        .map_err(|error| format!("cannot inspect pinned rustc target libdir: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pinned rustc target-libdir query failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let path = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("pinned rustc target-libdir is not UTF-8: {error}"))?
        .trim();
    if path.is_empty() {
        return Err(format!(
            "pinned rustc returned an empty target-libdir for {target}"
        ));
    }
    Ok(PathBuf::from(path))
}

pub(super) fn prune_integration_cache() -> Result<(), String> {
    let root = integration_cache_root();
    if !root.exists() {
        return Ok(());
    }
    let now = SystemTime::now();
    let mut successful = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| format!("{}: {error}", root.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(entry_lock) = try_lock_cache_entry(&path)? else {
            continue;
        };
        let marker = path.join(CACHE_MARKER_FILENAME);
        if !marker.exists() {
            let age = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or(Duration::ZERO);
            if age >= INCOMPLETE_CACHE_GRACE_PERIOD {
                let _ = fs::remove_dir_all(path);
            }
            continue;
        }
        let modified = marker
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age >= INTEGRATION_CACHE_MAX_AGE {
            let _ = fs::remove_dir_all(path);
            continue;
        }
        successful.push((modified, directory_size(&path)?, path));
        drop(entry_lock);
    }

    successful.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut retained_bytes = 0_u64;
    for (observed_modified, size, path) in successful {
        if retained_bytes.saturating_add(size) <= INTEGRATION_CACHE_MAX_BYTES {
            retained_bytes = retained_bytes.saturating_add(size);
            continue;
        }
        let Some(_entry_lock) = try_lock_cache_entry(&path)? else {
            continue;
        };
        let marker = path.join(CACHE_MARKER_FILENAME);
        if marker
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified == observed_modified)
        {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(())
}

fn try_lock_cache_entry(path: &Path) -> Result<Option<fs::File>, String> {
    if !path.is_dir() {
        return Ok(None);
    }
    let lock_path = path.join(CACHE_ENTRY_LOCK_FILENAME);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path);
    let lock = match lock {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", lock_path.display())),
    };
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => {
            Err(format!("cannot lock {}: {error}", lock_path.display()))
        }
    }
}

fn write_marker_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no cache directory", path.display()))?;
    let temporary = loop {
        let sequence = PENDING_BINARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".rustc-ok-{}-{sequence}.pending",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                    drop(file);
                    drop(fs::remove_file(&temporary));
                    return Err(format!("{}: {error}", temporary.display()));
                }
                drop(file);
                break temporary;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", temporary.display())),
        }
    };
    if path.exists()
        && let Err(error) = fs::remove_file(path)
    {
        drop(fs::remove_file(&temporary));
        return Err(format!(
            "cannot replace integration cache marker {}: {error}",
            path.display()
        ));
    }
    if let Err(error) = fs::rename(&temporary, path) {
        drop(fs::remove_file(&temporary));
        return Err(format!(
            "cannot atomically publish {}: {error}",
            path.display()
        ));
    }
    sync_directory(parent)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn lock_path_still_names_file(lock: &fs::File, path: &Path) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt as _;

    let open = lock
        .metadata()
        .map_err(|error| format!("cannot inspect open cache lock {}: {error}", path.display()))?;
    let named = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "cannot inspect cache lock {}: {error}",
                path.display()
            ));
        }
    };
    Ok(open.dev() == named.dev() && open.ino() == named.ino())
}

#[cfg(not(unix))]
fn lock_path_still_names_file(_lock: &fs::File, path: &Path) -> Result<bool, String> {
    Ok(path.is_file())
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    sha256_reader(fs::File::open(path)?)
}

fn sha256_reader(mut reader: impl Read) -> Result<String, std::io::Error> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let bytes = buffer
            .get(..read)
            .ok_or_else(|| std::io::Error::other("cache hash read exceeded its buffer"))?;
        hasher.update(bytes);
    }
    Ok(hex_hash(&hasher.finalize()))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn strictly_ascending(values: &[u16]) -> bool {
    values
        .windows(2)
        .all(|pair| matches!(pair, [left, right] if left < right))
}

fn directory_size(dir: &Path) -> Result<u64, String> {
    let mut size = 0_u64;
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            size = size.saturating_add(directory_size(&path)?);
        } else {
            size = size.saturating_add(
                entry
                    .metadata()
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .len(),
            );
        }
    }
    Ok(size)
}

fn collect_regular_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files_recursive(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn test_binary_fingerprint() -> &'static str {
    static TEST_BINARY_FINGERPRINT: OnceLock<String> = OnceLock::new();
    TEST_BINARY_FINGERPRINT.get_or_init(|| {
        std::env::current_exe()
            .and_then(fs::read)
            .map(|binary| {
                let mut hasher = Sha256::new();
                hasher.update(binary);
                hex_hash(&hasher.finalize())
            })
            .unwrap_or_else(|error| format!("test-binary-fingerprint-error:{error}"))
    })
}

fn rustc_fingerprint() -> &'static str {
    static RUSTC_FINGERPRINT: OnceLock<String> = OnceLock::new();
    RUSTC_FINGERPRINT.get_or_init(|| {
        Command::new("rustup")
            .args([
                "run",
                gors_runtime_abi::NATIVE_RUNTIME_RUST_TOOLCHAIN,
                "rustc",
                "-vV",
            ])
            .output()
            .map(|output| {
                let mut hasher = Sha256::new();
                hasher.update(&output.stdout);
                hasher.update(&output.stderr);
                hasher.update([u8::from(output.status.success())]);
                let digest = hasher.finalize();
                hex_hash(&digest)
            })
            .unwrap_or_else(|error| format!("rustc-fingerprint-error:{error}"))
    })
}

fn rustc_verbose_version() -> Result<&'static [u8], String> {
    static RUSTC_VERBOSE_VERSION: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();
    RUSTC_VERBOSE_VERSION
        .get_or_init(|| {
            let output = Command::new("rustup")
                .args([
                    "run",
                    gors_runtime_abi::NATIVE_RUNTIME_RUST_TOOLCHAIN,
                    "rustc",
                    "-vV",
                ])
                .output()
                .map_err(|error| format!("cannot inspect pinned rustc: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "pinned rustc -vV failed with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            if output.stdout.is_empty() {
                return Err("pinned rustc -vV returned an empty producer record".to_string());
            }
            Ok(output.stdout)
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

fn hex_hash(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
}

#[cfg(test)]
mod tests;
