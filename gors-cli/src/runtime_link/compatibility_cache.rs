//! Strict metadata-admitted cache for native rustc/runtime compatibility.
//!
//! A hit resolves the ABI-owned rustup selector once and compares filesystem
//! snapshots only. Rustc queries and the ABI's content-bearing rustlib
//! inventory run only after a per-selector/target OS lock is held.

use std::fmt::{Display, Formatter};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gors_runtime_abi::{
    Endianness, RustRlibCompatibility, RustRlibRecordError, TargetModel,
    canonical_target_libdir_record,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use self::probe::{CompatibilityProbe, ProbeError, SystemProbe};
use self::snapshot::{RustcSnapshot, SnapshotError, TreeSnapshot, rustc_snapshot, tree_snapshot};

mod probe;
mod snapshot;

const CACHE_SCHEMA: u32 = 2;
const CACHE_FILENAME: &str = "compatibility-v2.json";
const LOCK_FILENAME: &str = ".compatibility.lock";
const CACHE_KEY_DOMAIN: &[u8] = b"gors.native-rustc-compatibility-cache-key\0";
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn resolve(
    cache_root: &Path,
    selector: &str,
    target: &TargetModel,
    expected: &RustRlibCompatibility,
) -> Result<ResolvedRustcCompatibility, CompatibilityCacheError> {
    resolve_with_probe(cache_root, selector, target, expected, &SystemProbe)
}

pub(super) fn current_rustc_snapshot_identity(
    rustc_path: &Path,
) -> Result<String, CompatibilityCacheError> {
    let snapshot = rustc_snapshot(rustc_path).map_err(CompatibilityCacheError::Snapshot)?;
    snapshot_identity(&snapshot)
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedRustcCompatibility {
    compatibility: RustRlibCompatibility,
    rustc_path: PathBuf,
    rustc_snapshot_identity: String,
}

impl ResolvedRustcCompatibility {
    pub(super) const fn compatibility(&self) -> &RustRlibCompatibility {
        &self.compatibility
    }

    pub(super) fn rustc_path(&self) -> &Path {
        &self.rustc_path
    }

    pub(super) fn rustc_snapshot_identity(&self) -> &str {
        &self.rustc_snapshot_identity
    }
}

fn resolve_with_probe(
    cache_root: &Path,
    selector: &str,
    target: &TargetModel,
    expected: &RustRlibCompatibility,
    probe: &dyn CompatibilityProbe,
) -> Result<ResolvedRustcCompatibility, CompatibilityCacheError> {
    if expected.target() != target {
        return Err(CompatibilityCacheError::ExpectedTargetMismatch);
    }
    let entry_directory = cache_entry_directory(cache_root, selector, target);
    std::fs::create_dir_all(&entry_directory).map_err(|source| {
        CompatibilityCacheError::io(
            "create toolchain compatibility cache directory",
            &entry_directory,
            source,
        )
    })?;
    let rustc_path = probe
        .resolve_rustc(selector)
        .map_err(CompatibilityCacheError::Probe)?;
    let initial_rustc_snapshot = rustc_snapshot(&rustc_path).ok();
    let cache_path = entry_directory.join(CACHE_FILENAME);
    if let Some(initial_rustc_snapshot) = &initial_rustc_snapshot {
        if let Some(compatibility) = admit_cached(
            &cache_path,
            selector,
            target,
            expected,
            &rustc_path,
            initial_rustc_snapshot,
        ) {
            return Ok(compatibility);
        }
    }

    let lock_path = entry_directory.join(LOCK_FILENAME);
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| {
            CompatibilityCacheError::io("open toolchain compatibility lock", &lock_path, source)
        })?;
    lock_file.lock().map_err(|source| {
        CompatibilityCacheError::io("lock toolchain compatibility cache", &lock_path, source)
    })?;

    let locked_rustc_snapshot =
        rustc_snapshot(&rustc_path).map_err(CompatibilityCacheError::Snapshot)?;
    if let Some(compatibility) = admit_cached(
        &cache_path,
        selector,
        target,
        expected,
        &rustc_path,
        &locked_rustc_snapshot,
    ) {
        return Ok(compatibility);
    }

    let probed = fully_probe(
        selector,
        target,
        expected,
        probe,
        &rustc_path,
        locked_rustc_snapshot,
    )?;
    publish(&cache_path, &probed.document)?;
    Ok(probed.resolved)
}

struct FullProbe {
    resolved: ResolvedRustcCompatibility,
    document: CacheDocument,
}

fn fully_probe(
    selector: &str,
    target: &TargetModel,
    expected: &RustRlibCompatibility,
    probe: &dyn CompatibilityProbe,
    rustc_path: &Path,
    rustc_before: RustcSnapshot,
) -> Result<FullProbe, CompatibilityCacheError> {
    let rustc_verbose_version = probe
        .rustc_verbose_version(rustc_path)
        .map_err(CompatibilityCacheError::Probe)?;
    let reported_target_libdir = probe
        .target_libdir(rustc_path, target.triple())
        .map_err(CompatibilityCacheError::Probe)?;
    let target_libdir = std::fs::canonicalize(&reported_target_libdir).map_err(|source| {
        CompatibilityCacheError::io(
            "canonicalize probed target-libdir",
            &reported_target_libdir,
            source,
        )
    })?;
    let target_before = tree_snapshot(&target_libdir).map_err(CompatibilityCacheError::Snapshot)?;
    let target_libdir_record = canonical_target_libdir_record(&target_libdir)
        .map_err(CompatibilityCacheError::TargetInventory)?;
    let target_after = tree_snapshot(&target_libdir).map_err(CompatibilityCacheError::Snapshot)?;
    let rustc_after = rustc_snapshot(rustc_path).map_err(CompatibilityCacheError::Snapshot)?;
    if rustc_before != rustc_after || target_before != target_after {
        return Err(CompatibilityCacheError::UnstableProbe);
    }
    let compatibility =
        RustRlibCompatibility::new(&rustc_verbose_version, target_libdir_record, target.clone())
            .map_err(CompatibilityCacheError::InvalidCompatibility)?;
    if compatibility.identity() != expected.identity()
        || compatibility.canonical_bytes() != expected.canonical_bytes()
    {
        return Err(CompatibilityCacheError::ProviderMismatch {
            live: compatibility.identity().to_string(),
            provider: expected.identity().to_string(),
        });
    }
    let rustc_snapshot_identity = snapshot_identity(&rustc_after)?;
    let payload = CachePayload {
        selector: selector.to_owned(),
        target: CachedTarget::from(target),
        rustc_verbose_version: String::from_utf8(rustc_verbose_version)
            .map_err(|_| CompatibilityCacheError::InvalidRustcUtf8)?,
        rustc_release_record: hex(compatibility.rustc_release_record()),
        target_libdir: utf8_absolute_path(&target_libdir)?,
        target_libdir_record: hex(compatibility.target_libdir_record()),
        compatibility_record: hex(compatibility.canonical_bytes()),
        compatibility_identity: compatibility.identity().to_string(),
        rustc_snapshot: rustc_after,
        rustc_snapshot_identity: rustc_snapshot_identity.clone(),
        target_libdir_snapshot: target_after,
    };
    let document = CacheDocument::new(payload)?;
    Ok(FullProbe {
        resolved: ResolvedRustcCompatibility {
            compatibility,
            rustc_path: rustc_path.to_path_buf(),
            rustc_snapshot_identity,
        },
        document,
    })
}

fn admit_cached(
    cache_path: &Path,
    selector: &str,
    target: &TargetModel,
    expected: &RustRlibCompatibility,
    rustc_path: &Path,
    current_rustc_snapshot: &RustcSnapshot,
) -> Option<ResolvedRustcCompatibility> {
    // Cache decoding is intentionally fail-closed. Every failure becomes a
    // locked full probe; no partially decoded field is used as compatibility
    // evidence.
    let bytes = std::fs::read(cache_path).ok()?;
    let document: CacheDocument = serde_json::from_slice(&bytes).ok()?;
    if !document.verifies_checksum().ok()? {
        return None;
    }
    let payload = &document.payload;
    if payload.selector != selector
        || !payload.target.matches(target)
        || payload.rustc_snapshot != *current_rustc_snapshot
        || payload.rustc_snapshot.path() != rustc_path
        || !Path::new(&payload.target_libdir).is_absolute()
    {
        return None;
    }
    let current_target_snapshot = tree_snapshot(Path::new(&payload.target_libdir)).ok()?;
    if current_target_snapshot != payload.target_libdir_snapshot {
        return None;
    }
    let target_libdir_record = decode_hex(&payload.target_libdir_record)?;
    let compatibility = RustRlibCompatibility::new(
        payload.rustc_verbose_version.as_bytes(),
        target_libdir_record,
        target.clone(),
    )
    .ok()?;
    let current_rustc_snapshot_identity = snapshot_identity(current_rustc_snapshot).ok()?;
    if payload.rustc_release_record != hex(compatibility.rustc_release_record())
        || payload.target_libdir_record != hex(compatibility.target_libdir_record())
        || payload.compatibility_record != hex(compatibility.canonical_bytes())
        || payload.compatibility_identity != compatibility.identity().to_string()
        || payload.rustc_snapshot_identity != current_rustc_snapshot_identity
        || compatibility.identity() != expected.identity()
        || compatibility.canonical_bytes() != expected.canonical_bytes()
    {
        return None;
    }
    Some(ResolvedRustcCompatibility {
        compatibility,
        rustc_path: rustc_path.to_path_buf(),
        rustc_snapshot_identity: current_rustc_snapshot_identity,
    })
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheDocument {
    schema_version: u32,
    checksum: String,
    payload: CachePayload,
}

impl CacheDocument {
    fn new(payload: CachePayload) -> Result<Self, CompatibilityCacheError> {
        let checksum = payload_checksum(&payload)?;
        Ok(Self {
            schema_version: CACHE_SCHEMA,
            checksum,
            payload,
        })
    }

    fn verifies_checksum(&self) -> Result<bool, serde_json::Error> {
        Ok(self.schema_version == CACHE_SCHEMA
            && is_sha256(&self.checksum)
            && self.checksum == payload_checksum_json(&self.payload)?)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CachePayload {
    selector: String,
    target: CachedTarget,
    rustc_verbose_version: String,
    rustc_release_record: String,
    target_libdir: String,
    target_libdir_record: String,
    compatibility_record: String,
    compatibility_identity: String,
    rustc_snapshot: RustcSnapshot,
    rustc_snapshot_identity: String,
    target_libdir_snapshot: TreeSnapshot,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CachedTarget {
    triple: String,
    pointer_width: u16,
    endianness: String,
}

impl From<&TargetModel> for CachedTarget {
    fn from(target: &TargetModel) -> Self {
        Self {
            triple: target.triple().to_owned(),
            pointer_width: target.pointer_width().bits(),
            endianness: endianness_name(target.endianness()).to_owned(),
        }
    }
}

impl CachedTarget {
    fn matches(&self, target: &TargetModel) -> bool {
        self.triple == target.triple()
            && self.pointer_width == target.pointer_width().bits()
            && self.endianness == endianness_name(target.endianness())
    }
}

fn cache_entry_directory(cache_root: &Path, selector: &str, target: &TargetModel) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_KEY_DOMAIN);
    update_len_prefixed(&mut hasher, selector.as_bytes());
    update_len_prefixed(&mut hasher, target.triple().as_bytes());
    hasher.update(target.pointer_width().bits().to_be_bytes());
    hasher.update([match target.endianness() {
        Endianness::Little => 1,
        Endianness::Big => 2,
    }]);
    cache_root.join(hex(hasher.finalize()))
}

fn update_len_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn endianness_name(endianness: Endianness) -> &'static str {
    match endianness {
        Endianness::Little => "little",
        Endianness::Big => "big",
    }
}

fn payload_checksum(payload: &CachePayload) -> Result<String, CompatibilityCacheError> {
    payload_checksum_json(payload).map_err(CompatibilityCacheError::Serialize)
}

fn payload_checksum_json(payload: &CachePayload) -> Result<String, serde_json::Error> {
    serde_json::to_vec(payload).map(|bytes| hex(Sha256::digest(bytes)))
}

fn snapshot_identity(snapshot: &RustcSnapshot) -> Result<String, CompatibilityCacheError> {
    snapshot
        .identity()
        .map(hex)
        .map_err(CompatibilityCacheError::Serialize)
}

fn publish(path: &Path, document: &CacheDocument) -> Result<(), CompatibilityCacheError> {
    let directory = path
        .parent()
        .ok_or_else(|| CompatibilityCacheError::MissingCacheParent(path.to_path_buf()))?;
    let content =
        serde_json::to_vec_pretty(document).map_err(CompatibilityCacheError::Serialize)?;
    let (temporary_path, mut temporary_file) = create_temporary_file(directory)?;
    let write_result = (|| -> Result<(), std::io::Error> {
        temporary_file.write_all(&content)?;
        temporary_file.write_all(b"\n")?;
        temporary_file.sync_all()
    })();
    if let Err(source) = write_result {
        drop(temporary_file);
        drop(std::fs::remove_file(&temporary_path));
        return Err(CompatibilityCacheError::io(
            "write toolchain compatibility cache",
            &temporary_path,
            source,
        ));
    }
    drop(temporary_file);
    #[cfg(windows)]
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            drop(std::fs::remove_file(&temporary_path));
            return Err(CompatibilityCacheError::io(
                "replace toolchain compatibility cache",
                path,
                source,
            ));
        }
    }
    if let Err(source) = std::fs::rename(&temporary_path, path) {
        drop(std::fs::remove_file(&temporary_path));
        return Err(CompatibilityCacheError::io(
            "publish toolchain compatibility cache",
            path,
            source,
        ));
    }
    sync_directory(directory)?;
    Ok(())
}

fn create_temporary_file(
    directory: &Path,
) -> Result<(PathBuf, std::fs::File), CompatibilityCacheError> {
    for _ in 0..64 {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".{CACHE_FILENAME}.tmp-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(CompatibilityCacheError::io(
                    "create temporary toolchain compatibility cache",
                    &path,
                    source,
                ));
            }
        }
    }
    let path = directory.join(format!(".{CACHE_FILENAME}.tmp"));
    Err(CompatibilityCacheError::io(
        "create temporary toolchain compatibility cache",
        &path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename sequence exhausted",
        ),
    ))
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), CompatibilityCacheError> {
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|source| {
            CompatibilityCacheError::io(
                "sync toolchain compatibility cache directory",
                directory,
                source,
            )
        })
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), CompatibilityCacheError> {
    Ok(())
}

fn utf8_absolute_path(path: &Path) -> Result<String, CompatibilityCacheError> {
    if !path.is_absolute() {
        return Err(CompatibilityCacheError::NonAbsoluteProbePath(
            path.to_path_buf(),
        ));
    }
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CompatibilityCacheError::NonUtf8ProbePath(path.to_path_buf()))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(hex_digit(byte >> 4));
        output.push(hex_digit(byte & 0x0f));
    }
    output
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        _ => char::from(b'a' + nibble - 10),
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| match pair {
            [high, low] => Some((hex_nibble(*high)? << 4) | hex_nibble(*low)?),
            _ => None,
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug)]
pub(super) enum CompatibilityCacheError {
    Probe(ProbeError),
    Snapshot(SnapshotError),
    TargetInventory(gors_runtime_abi::RustTargetLibdirError),
    InvalidCompatibility(RustRlibRecordError),
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Serialize(serde_json::Error),
    ExpectedTargetMismatch,
    ProviderMismatch {
        live: String,
        provider: String,
    },
    InvalidRustcUtf8,
    UnstableProbe,
    NonAbsoluteProbePath(PathBuf),
    NonUtf8ProbePath(PathBuf),
    MissingCacheParent(PathBuf),
}

impl CompatibilityCacheError {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl Display for CompatibilityCacheError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Probe(error) => write!(formatter, "native rustc probe failed: {error}"),
            Self::Snapshot(error) => write!(formatter, "native rustc snapshot failed: {error}"),
            Self::TargetInventory(error) => {
                write!(formatter, "native target rustlib inventory failed: {error}")
            }
            Self::InvalidCompatibility(error) => {
                write!(formatter, "native rustc compatibility is invalid: {error}")
            }
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "failed to {action} {}: {source}", path.display()),
            Self::Serialize(error) => {
                write!(
                    formatter,
                    "failed to encode rustc compatibility cache: {error}"
                )
            }
            Self::ExpectedTargetMismatch => formatter.write_str(
                "embedded runtime compatibility target differs from its artifact target",
            ),
            Self::ProviderMismatch { live, provider } => write!(
                formatter,
                "live native rustc compatibility {live} does not match embedded provider {provider}"
            ),
            Self::InvalidRustcUtf8 => {
                formatter.write_str("resolved rustc -vV output is not valid UTF-8")
            }
            Self::UnstableProbe => formatter.write_str(
                "native rustc or target-libdir changed while its compatibility was probed",
            ),
            Self::NonAbsoluteProbePath(path) => write!(
                formatter,
                "native rustc probe returned a non-absolute path: {}",
                path.display()
            ),
            Self::NonUtf8ProbePath(path) => write!(
                formatter,
                "native rustc probe returned a non-UTF-8 path: {}",
                path.display()
            ),
            Self::MissingCacheParent(path) => write!(
                formatter,
                "toolchain compatibility cache path has no parent: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CompatibilityCacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Probe(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::TargetInventory(error) => Some(error),
            Self::InvalidCompatibility(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Serialize(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
