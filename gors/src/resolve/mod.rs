//! Go package resolver.
//!
//! This module resolves import paths to Go source packages, currently backed by
//! build-time generated metadata from the embedded Go SDK.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
use rayon::prelude::*;

use crate::profile::ProfileTimer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod runtime_primitives;
mod structural_helpers;

#[derive(Clone, Copy)]
struct EmbeddedGoFile {
    filename: &'static str,
    content: &'static str,
}

#[derive(Clone, Copy)]
struct EmbeddedGoPackage {
    import_path: &'static str,
    files: &'static [EmbeddedGoFile],
    direct_imports: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/go_stdlib.rs"));

type PackageFiles = Vec<(&'static str, &'static str)>;
type TypeEnv = crate::compiler::typeinfer::TypeEnv;
type TypeEnvCell = Arc<OnceLock<Option<(String, TypeEnv)>>>;
type PackageFilesCache = HashMap<String, Arc<OnceLock<Option<Arc<PackageFiles>>>>>;
type TypeEnvCache = HashMap<String, TypeEnvCell>;
type TransitiveImportsCache = HashMap<String, Arc<OnceLock<Vec<String>>>>;
type ResolvedModuleCache = HashMap<String, Arc<ResolvedModuleSlot>>;

struct ResolvedModuleSlot {
    entry: OnceLock<ResolvedModuleEntry>,
    last_used: AtomicU64,
}

impl ResolvedModuleSlot {
    fn new() -> Self {
        Self {
            entry: OnceLock::new(),
            last_used: AtomicU64::new(next_resolved_cache_tick()),
        }
    }

    fn touch(&self) {
        self.last_used
            .store(next_resolved_cache_tick(), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
enum ResolvedModuleEntry {
    Missing {
        imports: Vec<String>,
    },
    Source {
        source: String,
        imports: Vec<String>,
    },
    Uncacheable,
}

impl ResolvedModuleEntry {
    fn imports(&self) -> Option<&[String]> {
        match self {
            Self::Missing { imports } | Self::Source { imports, .. } => Some(imports),
            Self::Uncacheable => None,
        }
    }

    fn estimated_bytes(&self) -> usize {
        let imports = self
            .imports()
            .map_or(0, |imports| imports.iter().map(String::len).sum());
        match self {
            Self::Source { source, .. } => source.len().saturating_add(imports),
            Self::Missing { .. } => imports,
            Self::Uncacheable => 0,
        }
    }
}

struct ResolvedModuleOutput {
    module: Option<syn::ItemMod>,
    imports: Vec<String>,
}

const RESOLVED_CACHE_SCHEMA: u32 = 4;
#[cfg(target_family = "wasm")]
const MAX_IMPORTED_RESOLVED_CACHE_BYTES: usize = 64 * 1024 * 1024;
#[cfg(not(target_family = "wasm"))]
const MAX_IMPORTED_RESOLVED_CACHE_BYTES: usize = 256 * 1024 * 1024;
#[cfg(target_family = "wasm")]
const MAX_IMPORTED_RESOLVED_CACHE_ENTRIES: usize = 1_024;
#[cfg(not(target_family = "wasm"))]
const MAX_IMPORTED_RESOLVED_CACHE_ENTRIES: usize = 4_096;
#[cfg(target_family = "wasm")]
const MAX_IN_MEMORY_RESOLVED_CACHE_BYTES: usize = 64 * 1024 * 1024;
#[cfg(not(target_family = "wasm"))]
const MAX_IN_MEMORY_RESOLVED_CACHE_BYTES: usize = 256 * 1024 * 1024;
#[cfg(target_family = "wasm")]
const MAX_IN_MEMORY_RESOLVED_CACHE_ENTRIES: usize = 256;
#[cfg(not(target_family = "wasm"))]
const MAX_IN_MEMORY_RESOLVED_CACHE_ENTRIES: usize = 1_024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedCacheArchive {
    schema: u32,
    go_version: String,
    stdlib_version: String,
    #[serde(rename = "compilerFingerprint")]
    resolver_fingerprint: String,
    entries: Vec<ResolvedCacheRecord>,
    type_envs: Vec<ResolvedTypeEnvRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedCacheRecord {
    import_path: String,
    roots: Option<Vec<String>>,
    source: Option<String>,
    imports: Vec<String>,
    integrity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedTypeEnvRecord {
    import_path: String,
    package_name: String,
    env: serde_json::Value,
    integrity: String,
}

#[derive(Debug)]
struct PreparedResolvedCacheArchive {
    entries: Vec<(String, ResolvedModuleEntry)>,
    type_envs: Vec<(String, String, TypeEnv)>,
}

/// Result of importing a persistent resolver cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCacheImportStats {
    /// Records accepted into previously empty in-memory cache slots.
    pub imported: usize,
    /// Valid records whose cache slots were already initialized.
    pub already_present: usize,
    /// Type environments accepted into previously empty in-memory cache slots.
    pub type_envs_imported: usize,
    /// Valid type environments whose cache slots were already initialized.
    pub type_envs_already_present: usize,
}

/// Current bounded in-memory generated-module cache usage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModuleCacheStats {
    /// Cache slots, including a slot currently being initialized.
    pub entries: usize,
    /// Fully initialized cache slots.
    pub initialized: usize,
    /// Estimated generated-source and dependency metadata bytes.
    pub bytes: usize,
    /// Initialized slots evicted since process start.
    pub evictions: u64,
}

static PACKAGE_FILES: OnceLock<RwLock<PackageFilesCache>> = OnceLock::new();
static TYPE_ENVS: OnceLock<RwLock<TypeEnvCache>> = OnceLock::new();
static TRANSITIVE_IMPORTS: OnceLock<RwLock<TransitiveImportsCache>> = OnceLock::new();
static RESOLVED_MODULES: OnceLock<RwLock<ResolvedModuleCache>> = OnceLock::new();
static RESOLVED_CACHE_CLOCK: AtomicU64 = AtomicU64::new(1);
static RESOLVED_CACHE_EVICTIONS: AtomicU64 = AtomicU64::new(0);

fn package_file_cache() -> &'static RwLock<PackageFilesCache> {
    PACKAGE_FILES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn load_package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    let package = embedded_package(import_path)?;
    if package.files.is_empty() {
        return None;
    }
    Some(Arc::new(
        package
            .files
            .iter()
            .map(|file| (file.filename, file.content))
            .collect(),
    ))
}

fn type_envs() -> &'static RwLock<TypeEnvCache> {
    TYPE_ENVS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn transitive_imports() -> &'static RwLock<TransitiveImportsCache> {
    TRANSITIVE_IMPORTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn resolved_modules() -> &'static RwLock<ResolvedModuleCache> {
    RESOLVED_MODULES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn next_resolved_cache_tick() -> u64 {
    RESOLVED_CACHE_CLOCK.fetch_add(1, Ordering::Relaxed)
}

fn embedded_package(import_path: &str) -> Option<&'static EmbeddedGoPackage> {
    EMBEDDED_PACKAGES
        .binary_search_by(|package| package.import_path.cmp(import_path))
        .ok()
        .and_then(|idx| EMBEDDED_PACKAGES.get(idx))
}

pub fn is_known(import_path: &str) -> bool {
    package_exists(import_path)
}

pub fn package_exists(import_path: &str) -> bool {
    embedded_package(import_path).is_some()
}

pub fn package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    if !package_exists(import_path) {
        return None;
    }

    let Some(cell) = package_file_cell(import_path) else {
        return load_package_files(import_path);
    };
    cell.get_or_init(|| load_package_files(import_path)).clone()
}

fn package_file_cell(import_path: &str) -> Option<Arc<OnceLock<Option<Arc<PackageFiles>>>>> {
    if let Ok(cache) = package_file_cache().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = package_file_cache().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

pub fn list_packages() -> Vec<String> {
    EMBEDDED_PACKAGES
        .iter()
        .map(|package| package.import_path.to_string())
        .collect()
}

pub fn module_name(import_path: &str) -> String {
    let mut out = String::new();
    for ch in import_path.chars() {
        match ch {
            '/' => out.push_str("__"),
            ch if ch.is_ascii_alphanumeric() || ch == '_' => out.push(ch),
            _ => out.push('_'),
        }
    }

    if out.is_empty() {
        out.push('_');
    }

    if out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }

    if is_rust_keyword(&out) {
        out.push('_');
    }

    out
}

pub fn resolve(import_path: &str) -> Option<syn::ItemMod> {
    resolve_cached(
        import_path,
        None,
        crate::compiler::CompileOptions::default(),
    )
}

pub fn resolve_with_roots(import_path: &str, roots: &HashSet<String>) -> Option<syn::ItemMod> {
    resolve_with_roots_and_options(
        import_path,
        roots,
        crate::compiler::CompileOptions::default(),
    )
}

/// Resolve the reachable portion of a package using the requested compiler
/// task concurrency.
///
/// Package-wide analysis remains deterministic and sequential. When it is
/// safe, independent source files may lower concurrently; generated Rust is
/// formatted in the worker and reparsed by the coordinator so non-`Send`
/// `syn` nodes never cross a thread boundary.
pub fn resolve_with_roots_and_options(
    import_path: &str,
    roots: &HashSet<String>,
    options: crate::compiler::CompileOptions,
) -> Option<syn::ItemMod> {
    if roots.is_empty() {
        return None;
    }
    resolve_cached(import_path, Some(roots), options)
}

fn resolve_cached(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    options: crate::compiler::CompileOptions,
) -> Option<syn::ItemMod> {
    if crate::compiler::has_external_interface_implementors() {
        return resolve_uncached(
            import_path,
            roots,
            crate::compiler::CompileOptions::default(),
        )
        .module;
    }

    let cache_key = resolve_cache_key(import_path, roots);
    let Some(cell) = resolved_module_cell(import_path, roots, &cache_key) else {
        return resolve_uncached(import_path, roots, options).module;
    };

    let entry = cell.entry.get_or_init(|| {
        let resolved = resolve_uncached(import_path, roots, options);
        let imports = resolved.imports;
        let Some(module) = resolved.module else {
            return ResolvedModuleEntry::Missing { imports };
        };
        let source = module_content_cache_source(&module);
        if parse_cached_module(import_path, &source).is_some() {
            ResolvedModuleEntry::Source { source, imports }
        } else {
            ResolvedModuleEntry::Uncacheable
        }
    });
    cell.touch();
    let resolved = match entry {
        ResolvedModuleEntry::Missing { .. } => None,
        ResolvedModuleEntry::Source { source, .. } => parse_cached_module(import_path, source),
        ResolvedModuleEntry::Uncacheable => resolve_uncached(import_path, roots, options).module,
    };
    trim_resolved_module_cache();
    resolved
}

fn resolved_module_cell(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    cache_key: &str,
) -> Option<Arc<ResolvedModuleSlot>> {
    if let Ok(cache) = resolved_modules().read()
        && let Some((_, cell)) =
            reusable_resolved_module_slot(&cache, import_path, roots, cache_key)
    {
        cell.touch();
        return Some(cell.clone());
    }

    let Ok(mut cache) = resolved_modules().write() else {
        return None;
    };
    if let Some((_, cell)) = reusable_resolved_module_slot(&cache, import_path, roots, cache_key) {
        cell.touch();
        return Some(cell.clone());
    }
    Some(
        cache
            .entry(cache_key.to_string())
            .or_insert_with(|| Arc::new(ResolvedModuleSlot::new()))
            .clone(),
    )
}

fn reusable_resolved_module_slot<'a>(
    cache: &'a ResolvedModuleCache,
    import_path: &str,
    roots: Option<&HashSet<String>>,
    cache_key: &str,
) -> Option<(&'a str, &'a Arc<ResolvedModuleSlot>)> {
    // An exact slot owns its initialization, including when another worker is
    // currently filling it. Otherwise, an initialized module compiled for a
    // superset of the requested roots is safe to reuse: source reachability is
    // monotonic and compiler-side DCE still prunes from the actual roots. Keep
    // the smallest rooted superset to minimize downstream work, use the cache
    // key as a deterministic tie-breaker. An unfiltered (`None`) compilation
    // is a distinct lowering mode rather than a root set, so it is not reused.
    if let Some((stored_key, slot)) = cache.get_key_value(cache_key) {
        return Some((stored_key.as_str(), slot));
    }

    let requested_roots = roots?;
    cache
        .iter()
        .filter_map(|(candidate_key, slot)| {
            let entry = slot.entry.get()?;
            if matches!(entry, ResolvedModuleEntry::Uncacheable) {
                return None;
            }
            let (candidate_import_path, candidate_roots) = parse_resolve_cache_key(candidate_key)?;
            if candidate_import_path != import_path {
                return None;
            }
            let rank = match candidate_roots {
                Some(candidate_roots)
                    if requested_roots
                        .iter()
                        .all(|root| candidate_roots.binary_search(root).is_ok()) =>
                {
                    candidate_roots.len()
                }
                Some(_) | None => return None,
            };
            Some((rank, candidate_key.as_str(), slot))
        })
        .min_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)))
        .map(|(_, candidate_key, slot)| (candidate_key, slot))
}

fn trim_resolved_module_cache_map(
    cache: &mut ResolvedModuleCache,
    max_entries: usize,
    max_bytes: usize,
) -> usize {
    let mut bytes = cache
        .iter()
        .filter_map(|(cache_key, slot)| {
            slot.entry
                .get()
                .map(|entry| cache_key.len().saturating_add(entry.estimated_bytes()))
        })
        .sum::<usize>();
    if cache.len() <= max_entries && bytes <= max_bytes {
        return 0;
    }

    let mut candidates = cache
        .iter()
        .filter_map(|(cache_key, slot)| {
            slot.entry.get().map(|entry| {
                (
                    slot.last_used.load(Ordering::Relaxed),
                    cache_key.clone(),
                    cache_key.len().saturating_add(entry.estimated_bytes()),
                )
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));

    let mut evicted = 0;
    for (_, cache_key, entry_bytes) in candidates {
        if cache.len() <= max_entries && bytes <= max_bytes {
            break;
        }
        if cache.remove(&cache_key).is_some() {
            bytes = bytes.saturating_sub(entry_bytes);
            evicted += 1;
        }
    }
    evicted
}

fn trim_resolved_module_cache() {
    let Ok(mut cache) = resolved_modules().write() else {
        return;
    };
    let evicted = trim_resolved_module_cache_map(
        &mut cache,
        MAX_IN_MEMORY_RESOLVED_CACHE_ENTRIES,
        MAX_IN_MEMORY_RESOLVED_CACHE_BYTES,
    );
    RESOLVED_CACHE_EVICTIONS.fetch_add(evicted as u64, Ordering::Relaxed);
}

/// Return bounded generated-module cache telemetry without initializing entries.
pub fn resolved_module_cache_stats() -> ResolvedModuleCacheStats {
    let Ok(cache) = resolved_modules().read() else {
        return ResolvedModuleCacheStats {
            evictions: RESOLVED_CACHE_EVICTIONS.load(Ordering::Relaxed),
            ..ResolvedModuleCacheStats::default()
        };
    };
    let mut initialized = 0;
    let mut bytes = 0usize;
    for (cache_key, slot) in cache.iter() {
        bytes = bytes.saturating_add(cache_key.len());
        let Some(entry) = slot.entry.get() else {
            continue;
        };
        initialized += 1;
        bytes = bytes.saturating_add(entry.estimated_bytes());
    }
    ResolvedModuleCacheStats {
        entries: cache.len(),
        initialized,
        bytes,
        evictions: RESOLVED_CACHE_EVICTIONS.load(Ordering::Relaxed),
    }
}

pub(crate) fn has_initialized_resolved_module(import_path: &str, roots: &HashSet<String>) -> bool {
    let cache_key = resolve_cache_key(import_path, Some(roots));
    let Ok(cache) = resolved_modules().read() else {
        return false;
    };
    reusable_resolved_module_slot(&cache, import_path, Some(roots), &cache_key)
        .and_then(|(_, slot)| slot.entry.get())
        .is_some_and(|entry| !matches!(entry, ResolvedModuleEntry::Uncacheable))
}

/// Serialize reusable, mechanically generated stdlib modules resolved by this
/// process. The archive contains generated Rust source, never handwritten
/// replacements for Go packages.
pub fn export_resolved_module_cache() -> Result<Vec<u8>, String> {
    let mut entries = {
        let cache = resolved_modules()
            .read()
            .map_err(|_| "resolved module cache lock is poisoned".to_string())?;
        let mut entries = Vec::new();
        for (cache_key, slot) in cache.iter() {
            let Some(entry) = slot.entry.get() else {
                continue;
            };
            let Some((import_path, roots)) = parse_resolve_cache_key(cache_key) else {
                continue;
            };
            let (source, mut imports) = match entry {
                ResolvedModuleEntry::Missing { imports } => (None, imports.clone()),
                ResolvedModuleEntry::Source { source, imports } => {
                    (Some(source.clone()), imports.clone())
                }
                ResolvedModuleEntry::Uncacheable => continue,
            };
            imports.sort();
            imports.dedup();
            let mut record = ResolvedCacheRecord {
                import_path,
                roots,
                source,
                imports,
                integrity: String::new(),
            };
            record.integrity = resolved_record_integrity(&record)?;
            entries.push(record);
        }
        drop(cache);
        entries
    };
    entries.sort_by(|left, right| {
        (&left.import_path, &left.roots).cmp(&(&right.import_path, &right.roots))
    });
    let mut type_env_records = {
        let type_envs = type_envs()
            .read()
            .map_err(|_| "type environment cache lock is poisoned".to_string())?;
        let mut records = Vec::new();
        for (import_path, cell) in type_envs.iter() {
            let Some((package_name, env)) = cell.get().and_then(Option::as_ref) else {
                continue;
            };
            let env = canonical_type_env_value(env)?;
            let integrity = type_env_record_integrity(import_path, package_name, &env)?;
            records.push(ResolvedTypeEnvRecord {
                import_path: import_path.clone(),
                package_name: package_name.clone(),
                env,
                integrity,
            });
        }
        drop(type_envs);
        records
    };
    type_env_records.sort_by(|left, right| left.import_path.cmp(&right.import_path));
    serde_json::to_vec(&ResolvedCacheArchive {
        schema: RESOLVED_CACHE_SCHEMA,
        go_version: crate::GO_VERSION.to_string(),
        stdlib_version: crate::STDLIB_VERSION.to_string(),
        resolver_fingerprint: crate::RESOLVER_CACHE_FINGERPRINT.to_string(),
        entries,
        type_envs: type_env_records,
    })
    .map_err(|error| format!("failed to encode resolved module cache: {error}"))
}

/// Import a cache previously returned by [`export_resolved_module_cache`].
///
/// Archives are rejected unless their schema, Go SDK, stdlib, and resolver ABI
/// fingerprints match exactly.
pub fn import_resolved_module_cache(bytes: &[u8]) -> Result<ResolvedCacheImportStats, String> {
    if bytes.len() > MAX_IMPORTED_RESOLVED_CACHE_BYTES {
        return Err(format!(
            "resolved module cache is too large: {} bytes exceeds {}",
            bytes.len(),
            MAX_IMPORTED_RESOLVED_CACHE_BYTES
        ));
    }
    let archive: ResolvedCacheArchive = serde_json::from_slice(bytes)
        .map_err(|error| format!("failed to decode resolved module cache: {error}"))?;
    let prepared = prepare_resolved_cache_archive(archive)?;

    // Acquire every fallible global lock before publishing any validated
    // semantic data. Once validation has completed, individual OnceLock writes
    // cannot fail: an initialized slot simply counts as already present.
    let mut modules = resolved_modules()
        .write()
        .map_err(|_| "resolved module cache lock is poisoned".to_string())?;
    let mut envs = type_envs()
        .write()
        .map_err(|_| "type environment cache lock is poisoned".to_string())?;
    let mut stats = ResolvedCacheImportStats::default();
    for (cache_key, entry) in prepared.entries {
        let slot = modules
            .entry(cache_key)
            .or_insert_with(|| Arc::new(ResolvedModuleSlot::new()));
        if slot.entry.set(entry).is_ok() {
            stats.imported += 1;
        } else {
            stats.already_present += 1;
        }
        slot.touch();
    }
    for (import_path, package_name, env) in prepared.type_envs {
        let cell = envs
            .entry(import_path)
            .or_insert_with(|| Arc::new(OnceLock::new()));
        if cell.set(Some((package_name, env))).is_ok() {
            stats.type_envs_imported += 1;
        } else {
            stats.type_envs_already_present += 1;
        }
    }
    drop(envs);
    drop(modules);
    trim_resolved_module_cache();
    Ok(stats)
}

fn prepare_resolved_cache_archive(
    archive: ResolvedCacheArchive,
) -> Result<PreparedResolvedCacheArchive, String> {
    validate_resolved_cache_archive(&archive)?;
    if archive.entries.len() + archive.type_envs.len() > MAX_IMPORTED_RESOLVED_CACHE_ENTRIES {
        return Err(format!(
            "resolved module cache has too many entries: {} exceeds {}",
            archive.entries.len() + archive.type_envs.len(),
            MAX_IMPORTED_RESOLVED_CACHE_ENTRIES
        ));
    }

    let mut seen_entries = BTreeSet::new();
    let mut prepared_entries = Vec::with_capacity(archive.entries.len());
    for record in archive.entries {
        if !is_known(&record.import_path) {
            return Err(format!(
                "resolved module cache contains unknown package {}",
                record.import_path
            ));
        }
        if record.import_path.contains('\0') {
            return Err("resolved module cache contains an invalid package path".to_string());
        }
        if let Some(roots) = &record.roots {
            if roots.is_empty() {
                return Err(format!(
                    "resolved module cache contains an empty root set for {}",
                    record.import_path
                ));
            }
            validate_sorted_unique_strings(roots, "roots", &record.import_path)?;
            if roots
                .iter()
                .any(|root| root.is_empty() || root.contains([',', '\0']))
            {
                return Err(format!(
                    "resolved module cache contains an invalid root for {}",
                    record.import_path
                ));
            }
        }
        validate_sorted_unique_strings(&record.imports, "imports", &record.import_path)?;
        for dependency in &record.imports {
            if !is_known(dependency) {
                return Err(format!(
                    "resolved module cache contains unknown dependency {dependency} for {}",
                    record.import_path
                ));
            }
        }
        if record.source.is_none() && !record.imports.is_empty() {
            return Err(format!(
                "resolved module cache contains dependencies for missing package {}",
                record.import_path
            ));
        }
        if resolved_record_integrity(&record)? != record.integrity {
            return Err(format!(
                "resolved module cache integrity mismatch for {}",
                record.import_path
            ));
        }
        if let Some(source) = record.source.as_deref() {
            syn::parse_str::<syn::File>(source).map_err(|error| {
                format!(
                    "resolved module cache contains invalid Rust for {}: {error}",
                    record.import_path
                )
            })?;
        }
        let roots = record
            .roots
            .as_ref()
            .map(|roots| roots.iter().cloned().collect::<HashSet<_>>());
        let cache_key = resolve_cache_key(&record.import_path, roots.as_ref());
        if !seen_entries.insert(cache_key.clone()) {
            return Err(format!(
                "resolved module cache contains duplicate entry for {}",
                record.import_path
            ));
        }
        let entry = match record.source {
            None => ResolvedModuleEntry::Missing {
                imports: record.imports,
            },
            Some(source) => ResolvedModuleEntry::Source {
                source,
                imports: record.imports,
            },
        };
        prepared_entries.push((cache_key, entry));
    }

    let mut seen_type_envs = BTreeSet::new();
    let mut prepared_type_envs = Vec::with_capacity(archive.type_envs.len());
    for record in archive.type_envs {
        if !is_known(&record.import_path) {
            return Err(format!(
                "resolved module cache contains unknown type environment {}",
                record.import_path
            ));
        }
        if !seen_type_envs.insert(record.import_path.clone()) {
            return Err(format!(
                "resolved module cache contains duplicate type environment {}",
                record.import_path
            ));
        }
        let actual_package_name = embedded_package_name(&record.import_path)?;
        if record.package_name != actual_package_name {
            return Err(format!(
                "resolved module cache package name mismatch for {}: expected {}, got {}",
                record.import_path, actual_package_name, record.package_name
            ));
        }
        let mut canonical_env = record.env.clone();
        canonicalize_type_env_value(&mut canonical_env)?;
        if type_env_record_integrity(&record.import_path, &record.package_name, &canonical_env)?
            != record.integrity
        {
            return Err(format!(
                "resolved module cache type environment integrity mismatch for {}",
                record.import_path
            ));
        }
        let env: TypeEnv = serde_json::from_value(canonical_env.clone()).map_err(|error| {
            format!(
                "resolved module cache contains an invalid type environment for {}: {error}",
                record.import_path
            )
        })?;
        if canonical_type_env_value(&env)? != canonical_env {
            return Err(format!(
                "resolved module cache contains a non-canonical type environment for {}",
                record.import_path
            ));
        }
        prepared_type_envs.push((record.import_path, record.package_name, env));
    }

    Ok(PreparedResolvedCacheArchive {
        entries: prepared_entries,
        type_envs: prepared_type_envs,
    })
}

fn validate_sorted_unique_strings(
    values: &[String],
    field: &str,
    import_path: &str,
) -> Result<(), String> {
    if values
        .iter()
        .zip(values.iter().skip(1))
        .any(|(left, right)| left >= right)
    {
        return Err(format!(
            "resolved module cache contains unsorted or duplicate {field} for {import_path}"
        ));
    }
    Ok(())
}

fn resolved_record_integrity(record: &ResolvedCacheRecord) -> Result<String, String> {
    let value = serde_json::json!({
        "importPath": record.import_path,
        "roots": record.roots,
        "source": record.source,
        "imports": record.imports,
    });
    Ok(integrity_hash(
        b"gors-resolved-module-cache-record-v1\0",
        &serde_json::to_vec(&value)
            .map_err(|error| format!("failed to encode resolved module integrity: {error}"))?,
    ))
}

fn type_env_record_integrity(
    import_path: &str,
    package_name: &str,
    env: &serde_json::Value,
) -> Result<String, String> {
    let value = serde_json::json!({
        "importPath": import_path,
        "packageName": package_name,
        "env": env,
    });
    Ok(integrity_hash(
        b"gors-resolved-type-env-cache-record-v1\0",
        &serde_json::to_vec(&value)
            .map_err(|error| format!("failed to encode type environment integrity: {error}"))?,
    ))
}

fn integrity_hash(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_type_env_value(env: &TypeEnv) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(env)
        .map_err(|error| format!("failed to encode type environment: {error}"))?;
    canonicalize_type_env_value(&mut value)?;
    Ok(value)
}

fn canonicalize_type_env_value(value: &mut serde_json::Value) -> Result<(), String> {
    canonicalize_json_objects(value);
    let object = value
        .as_object_mut()
        .ok_or_else(|| "type environment wire value must be an object".to_string())?;
    for field in [
        "pointer_receiver_methods",
        "type_aliases",
        "instantiated_type_aliases",
        "string_consts",
        "top_level_vars",
        "consts",
    ] {
        if let Some(value) = object.get_mut(field) {
            sort_json_set(value, field)?;
        }
    }
    for field in [
        "owned_interface_params",
        "borrowed_slice_params",
        "struct_embedded_fields",
    ] {
        let Some(values) = object.get_mut(field) else {
            continue;
        };
        let values = values
            .as_object_mut()
            .ok_or_else(|| format!("type environment {field} must be an object"))?;
        for value in values.values_mut() {
            sort_json_set(value, field)?;
        }
    }
    Ok(())
}

fn canonicalize_json_objects(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            let mut fields = std::mem::take(object).into_iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, mut value) in fields {
                canonicalize_json_objects(&mut value);
                object.insert(key, value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                canonicalize_json_objects(value);
            }
        }
        _ => {}
    }
}

fn sort_json_set(value: &mut serde_json::Value, field: &str) -> Result<(), String> {
    let values = value
        .as_array_mut()
        .ok_or_else(|| format!("type environment {field} set must be an array"))?;
    let mut keyed = std::mem::take(values)
        .into_iter()
        .map(|value| {
            serde_json::to_vec(&value)
                .map(|key| (key, value))
                .map_err(|error| format!("failed to canonicalize type environment set: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    values.extend(keyed.into_iter().map(|(_, value)| value));
    Ok(())
}

fn embedded_package_name(import_path: &str) -> Result<String, String> {
    let package = embedded_package(import_path)
        .ok_or_else(|| format!("resolved module cache contains unknown package {import_path}"))?;
    let mut package_name = None;
    for file in package.files {
        let Ok(ast) = crate::parser::parse_file(file.filename, file.content) else {
            continue;
        };
        let name = ast.name.name.to_string();
        if package_name
            .as_ref()
            .is_some_and(|existing| existing != &name)
        {
            return Err(format!(
                "embedded Go SDK contains conflicting package names for {import_path}"
            ));
        }
        package_name = Some(name);
    }
    package_name.ok_or_else(|| {
        format!("embedded Go SDK has no parseable package declaration for {import_path}")
    })
}

fn validate_resolved_cache_archive(archive: &ResolvedCacheArchive) -> Result<(), String> {
    if archive.schema != RESOLVED_CACHE_SCHEMA {
        return Err(format!(
            "resolved module cache schema mismatch: expected {}, got {}",
            RESOLVED_CACHE_SCHEMA, archive.schema
        ));
    }
    if archive.go_version != crate::GO_VERSION {
        return Err(format!(
            "resolved module cache Go version mismatch: expected {}, got {}",
            crate::GO_VERSION,
            archive.go_version
        ));
    }
    if archive.stdlib_version != crate::STDLIB_VERSION {
        return Err(format!(
            "resolved module cache stdlib version mismatch: expected {}, got {}",
            crate::STDLIB_VERSION,
            archive.stdlib_version
        ));
    }
    if archive.resolver_fingerprint != crate::RESOLVER_CACHE_FINGERPRINT {
        return Err("resolved module cache resolver fingerprint mismatch".to_string());
    }
    Ok(())
}

fn parse_resolve_cache_key(cache_key: &str) -> Option<(String, Option<Vec<String>>)> {
    let Some((import_path, roots)) = cache_key.split_once('\0') else {
        return Some((cache_key.to_string(), None));
    };
    if import_path.is_empty() {
        return None;
    }
    let roots = if roots.is_empty() {
        Vec::new()
    } else {
        roots.split(',').map(str::to_string).collect()
    };
    Some((import_path.to_string(), Some(roots)))
}

fn parse_cached_module(import_path: &str, source: &str) -> Option<syn::ItemMod> {
    syn::parse_str::<syn::File>(source)
        .inspect_err(|error| {
            log_skip(format_args!(
                "[gors] skip resolved module cache for {import_path}: {error}"
            ));
        })
        .ok()
        .map(|file| item_mod_for(import_path, file.items))
}

fn module_content_cache_source(module: &syn::ItemMod) -> String {
    let items = module
        .content
        .as_ref()
        .map(|(_, items)| items.clone())
        .unwrap_or_default();
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items,
    })
}

fn resolve_cache_key(import_path: &str, roots: Option<&HashSet<String>>) -> String {
    let Some(roots) = roots else {
        return import_path.to_string();
    };
    let mut roots: Vec<_> = roots.iter().map(String::as_str).collect();
    roots.sort_unstable();
    format!("{import_path}\0{}", roots.join(","))
}

fn resolve_uncached(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    options: crate::compiler::CompileOptions,
) -> ResolvedModuleOutput {
    let total_timer = ProfileTimer::start(format!("resolve.{import_path}.total"));
    if let Some(module) = runtime_primitives::module(import_path, roots) {
        drop(total_timer);
        return ResolvedModuleOutput {
            module: Some(module),
            imports: Vec::new(),
        };
    }

    let Some(files) = package_files(import_path) else {
        return ResolvedModuleOutput {
            module: None,
            imports: Vec::new(),
        };
    };

    let parse_timer = ProfileTimer::start(format!("resolve.{import_path}.parse"));
    let mut parsed_files = Vec::new();
    for (filename, content) in files.iter() {
        let ast = match crate::parser::parse_file(filename, content) {
            Ok(ast) => ast,
            Err(e) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: parse error: {e}"
                ));
                continue;
            }
        };
        parsed_files.push((*filename, ast));
    }
    drop(parse_timer);

    let reachable_timer = ProfileTimer::start(format!("resolve.{import_path}.reachable"));
    let reachable_names = roots.map(|roots| {
        let parsed_file_refs = parsed_files.iter().map(|(_, ast)| ast).collect::<Vec<_>>();
        let imported_type_envs = scan_imported_type_envs(import_path, &parsed_file_refs);
        reachable_package_names_with_imports(&parsed_files, roots, &imported_type_envs)
    });
    drop(reachable_timer);
    if roots.is_some() && reachable_names.as_ref().is_none_or(HashSet::is_empty) {
        drop(total_timer);
        return ResolvedModuleOutput {
            module: None,
            imports: Vec::new(),
        };
    }

    let filter_timer = ProfileTimer::start(format!("resolve.{import_path}.filter"));
    let parsed_files: Vec<_> = parsed_files
        .into_iter()
        .filter_map(|(filename, ast)| {
            let filtered = match &reachable_names {
                Some(reachable) => filter_file_to_reachable(ast, reachable),
                None => ast,
            };
            file_has_compilable_decl(&filtered).then_some((filename, filtered))
        })
        .collect();
    drop(filter_timer);

    if parsed_files.is_empty() {
        drop(total_timer);
        return ResolvedModuleOutput {
            module: None,
            imports: Vec::new(),
        };
    }

    let type_env_timer = ProfileTimer::start(format!("resolve.{import_path}.type_env"));
    let mut package_type_env = TypeEnv::new();
    let parsed_file_refs = parsed_files.iter().map(|(_, ast)| ast).collect::<Vec<_>>();
    package_type_env.scan_files(&parsed_file_refs);
    runtime_primitives::supplement_type_env(import_path, &mut package_type_env);
    let imported_type_envs = scan_imported_type_envs(import_path, &parsed_file_refs);
    refresh_borrowed_slice_params_with_imports(
        &mut package_type_env,
        &parsed_file_refs,
        &imported_type_envs,
    );
    refresh_top_level_vars_with_imports(
        &mut package_type_env,
        &parsed_file_refs,
        &imported_type_envs,
    );
    let package_mutable_top_level_vars =
        crate::compiler::mutable_top_level_var_names_for_files_with_type_env(
            parsed_file_refs.iter().copied(),
            false,
            &package_type_env,
        );
    let view_method_seed =
        crate::compiler::borrowed_view_method_seed_for_files(&parsed_file_refs, &package_type_env);
    drop(type_env_timer);

    let import_timer = ProfileTimer::start(format!("resolve.{import_path}.imports"));
    let import_renames = package_import_renames(&parsed_files);
    let import_path_by_module = package_import_path_by_module(&parsed_files);
    drop(import_timer);
    let mut all_items: Vec<syn::Item> = Vec::new();
    let recovery_context = ResolvedRecoveryContext {
        import_path,
        package_type_env: &package_type_env,
        imported_type_envs: &imported_type_envs,
        import_renames: &import_renames,
        package_mutable_top_level_vars: &package_mutable_top_level_vars,
        view_method_seed: &view_method_seed,
    };

    let compile_timer = ProfileTimer::start(format!("resolve.{import_path}.compile"));
    let (compiled_files, _) =
        compile_resolved_files(parsed_files, &recovery_context, options.jobs());
    for compiled_file in compiled_files {
        let filename = compiled_file.filename;
        let compiled = match compiled_file.result {
            Ok(compiled) => compiled,
            Err(e) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: compile error: {e}"
                ));
                if let Some(content) = files
                    .iter()
                    .find_map(|(file, content)| (*file == filename).then_some(*content))
                {
                    all_items.extend(recover_resolved_file_items(
                        filename,
                        content,
                        reachable_names.as_ref(),
                        &recovery_context,
                    ));
                }
                continue;
            }
        };
        all_items.extend(compiled.items);
    }
    drop(compile_timer);

    let post_timer = ProfileTimer::start(format!("resolve.{import_path}.post"));
    runtime_primitives::supplement_items(import_path, roots, &mut all_items);
    crate::compiler::merge_package_init_items(&mut all_items);

    if all_items.is_empty() {
        drop(post_timer);
        drop(total_timer);
        return ResolvedModuleOutput {
            module: None,
            imports: Vec::new(),
        };
    }

    let mut merged_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: all_items,
    };
    crate::compiler::passes::pass_after_package_merge(&mut merged_file);
    crate::compiler::add_post_merge_interface_helpers(&mut merged_file);
    let mut all_items = merged_file.items;

    dedupe_use_items(&mut all_items);
    let used_imports = used_imports_from_items(&mut all_items, &import_path_by_module);
    let module_refs: HashSet<String> = used_imports.iter().map(|path| module_name(path)).collect();
    structural_helpers::inject(&mut all_items);
    let mut merged_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: all_items,
    };
    crate::compiler::passes::pass_after_structural_helpers(&mut merged_file);
    let mut all_items = merged_file.items;
    prefix_crate_paths(&mut all_items, &module_refs);

    let module = item_mod_for(import_path, all_items);
    drop(post_timer);
    drop(total_timer);
    ResolvedModuleOutput {
        module: Some(module),
        imports: used_imports,
    }
}

fn compile_resolved_file(
    ast: crate::ast::File<'_>,
    package_type_env: &TypeEnv,
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &BTreeMap<String, String>,
    package_mutable_top_level_vars: &HashSet<String>,
    view_method_seed: &crate::compiler::BorrowedViewMethodSeed,
) -> Result<syn::File, crate::compiler::CompilerError> {
    let mut type_env = package_type_env.clone();
    crate::compiler::merge_import_type_envs(
        &mut type_env,
        &ast,
        &BTreeMap::new(),
        imported_type_envs,
    );
    crate::compiler::compile_with_type_env_import_renames_mutable_vars_and_view_seed(
        ast,
        type_env,
        import_renames.clone(),
        Some(package_mutable_top_level_vars.clone()),
        Some(view_method_seed),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedFileCompileMode {
    Sequential,
    #[cfg(any(
        all(feature = "parallel", not(target_family = "wasm")),
        all(feature = "wasm-threads", target_family = "wasm")
    ))]
    Parallel,
}

struct CompiledResolvedFile<'a> {
    filename: &'a str,
    result: Result<syn::File, String>,
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
struct CompiledResolvedFileSource<'a> {
    filename: &'a str,
    result: Result<String, String>,
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
struct ParallelResolvedCompileContext<'a> {
    import_path: &'a str,
    package_type_env: &'a TypeEnv,
    imported_type_envs: &'a BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &'a BTreeMap<String, String>,
    package_mutable_top_level_vars: &'a HashSet<String>,
    view_method_seed: &'a crate::compiler::BorrowedViewMethodSeedSnapshot,
}

fn compile_resolved_files<'a>(
    parsed_files: Vec<(&'a str, crate::ast::File<'a>)>,
    context: &ResolvedRecoveryContext<'_>,
    jobs: usize,
) -> (Vec<CompiledResolvedFile<'a>>, ResolvedFileCompileMode) {
    let can_parallelize = jobs > 1
        && parsed_files.len() > 1
        && !crate::compiler::has_external_interface_implementors();

    #[cfg(all(feature = "parallel", not(target_family = "wasm")))]
    if can_parallelize && rayon::current_thread_index().is_none() {
        let thread_count = jobs.min(parsed_files.len());
        if let Ok(pool) = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .thread_name(|index| format!("gors-file-{index}"))
            .build()
        {
            let view_method_seed = context.view_method_seed.snapshot();
            let parallel_context = ParallelResolvedCompileContext {
                import_path: context.import_path,
                package_type_env: context.package_type_env,
                imported_type_envs: context.imported_type_envs,
                import_renames: context.import_renames,
                package_mutable_top_level_vars: context.package_mutable_top_level_vars,
                view_method_seed: &view_method_seed,
            };
            let compiled =
                pool.install(|| compile_resolved_files_to_sources(parsed_files, &parallel_context));
            return (
                reparse_compiled_resolved_files(compiled),
                ResolvedFileCompileMode::Parallel,
            );
        }
    }

    #[cfg(all(feature = "wasm-threads", target_family = "wasm"))]
    if can_parallelize {
        let view_method_seed = context.view_method_seed.snapshot();
        let parallel_context = ParallelResolvedCompileContext {
            import_path: context.import_path,
            package_type_env: context.package_type_env,
            imported_type_envs: context.imported_type_envs,
            import_renames: context.import_renames,
            package_mutable_top_level_vars: context.package_mutable_top_level_vars,
            view_method_seed: &view_method_seed,
        };
        let compiled = compile_resolved_files_to_sources(parsed_files, &parallel_context);
        return (
            reparse_compiled_resolved_files(compiled),
            ResolvedFileCompileMode::Parallel,
        );
    }

    let _ = can_parallelize;
    (
        parsed_files
            .into_iter()
            .map(|(filename, ast)| CompiledResolvedFile {
                filename,
                result: compile_one_resolved_file(filename, ast, context),
            })
            .collect(),
        ResolvedFileCompileMode::Sequential,
    )
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
fn compile_resolved_files_to_sources<'a>(
    parsed_files: Vec<(&'a str, crate::ast::File<'a>)>,
    context: &ParallelResolvedCompileContext<'_>,
) -> Vec<CompiledResolvedFileSource<'a>> {
    parsed_files
        .into_par_iter()
        .map_init(
            || context.view_method_seed.rehydrate(),
            |view_method_seed, (filename, ast)| CompiledResolvedFileSource {
                filename,
                result: match view_method_seed {
                    Ok(view_method_seed) => {
                        compile_one_parallel_resolved_file(filename, ast, context, view_method_seed)
                            .map(|file| prettyplease::unparse(&file))
                    }
                    Err(error) => Err(error.clone()),
                },
            },
        )
        .collect()
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
fn compile_one_parallel_resolved_file(
    filename: &str,
    ast: crate::ast::File<'_>,
    context: &ParallelResolvedCompileContext<'_>,
    view_method_seed: &crate::compiler::BorrowedViewMethodSeed,
) -> Result<syn::File, String> {
    let file_timer = ProfileTimer::start(format!(
        "resolve.{}.compile_file.{filename}",
        context.import_path
    ));
    let compiled = compile_resolved_file(
        ast,
        context.package_type_env,
        context.imported_type_envs,
        context.import_renames,
        context.package_mutable_top_level_vars,
        view_method_seed,
    )
    .map_err(|error| error.to_string());
    drop(file_timer);
    compiled
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
fn reparse_compiled_resolved_files(
    compiled: Vec<CompiledResolvedFileSource<'_>>,
) -> Vec<CompiledResolvedFile<'_>> {
    compiled
        .into_iter()
        .map(|compiled| CompiledResolvedFile {
            filename: compiled.filename,
            result: compiled.result.and_then(|source| {
                syn::parse_str(&source)
                    .map_err(|error| format!("generated Rust round-trip parse error: {error}"))
            }),
        })
        .collect()
}

fn compile_one_resolved_file(
    filename: &str,
    ast: crate::ast::File<'_>,
    context: &ResolvedRecoveryContext<'_>,
) -> Result<syn::File, String> {
    let file_timer = ProfileTimer::start(format!(
        "resolve.{}.compile_file.{filename}",
        context.import_path
    ));
    let compiled = compile_resolved_file(
        ast,
        context.package_type_env,
        context.imported_type_envs,
        context.import_renames,
        context.package_mutable_top_level_vars,
        context.view_method_seed,
    )
    .map_err(|error| error.to_string());
    drop(file_timer);
    compiled
}

fn scan_imported_type_envs(
    import_path: &str,
    files: &[&crate::ast::File<'_>],
) -> BTreeMap<String, crate::compiler::PackageFacts> {
    let mut imported_type_envs: BTreeMap<String, crate::compiler::PackageFacts> = BTreeMap::new();
    for ast in files {
        for import in ast.imports() {
            let imported_path = import.path.value.trim_matches('"');
            if imported_path == import_path {
                continue;
            }
            if let Some((package_name, env)) = scan_type_env(imported_path) {
                imported_type_envs.insert(
                    imported_path.to_string(),
                    crate::compiler::PackageFacts::new(package_name, env),
                );
            }
        }
    }
    imported_type_envs
}

fn refresh_borrowed_slice_params_with_imports(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    loop {
        let mut changed = false;
        for ast in files {
            let mut inference_env = package_type_env.clone();
            crate::compiler::merge_import_type_envs(
                &mut inference_env,
                ast,
                &BTreeMap::new(),
                imported_type_envs,
            );
            changed |=
                package_type_env.refresh_borrowed_slice_params_from_env(&[*ast], &inference_env);
        }
        if !changed {
            break;
        }
    }
}

fn refresh_top_level_vars_with_imports(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    for ast in files {
        let mut inference_env = package_type_env.clone();
        crate::compiler::merge_import_type_envs(
            &mut inference_env,
            ast,
            &BTreeMap::new(),
            imported_type_envs,
        );
        package_type_env.rescan_file_top_level_vars(ast, &inference_env);
    }
    merge_imported_receiver_facts_for_top_level_vars(package_type_env, files, imported_type_envs);
}

fn merge_imported_receiver_facts_for_top_level_vars(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    let receiver_names_by_local = imported_receiver_names_by_local_name(package_type_env);
    if receiver_names_by_local.is_empty() {
        return;
    }
    for ast in files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            let Some(package_facts) = imported_type_envs.get(import_path) else {
                continue;
            };
            let Some(local_name) = import_type_env_local_name(import, package_facts.package_name())
            else {
                continue;
            };
            let Some(receiver_names) = receiver_names_by_local.get(&local_name) else {
                continue;
            };
            package_type_env.merge_package_receiver_facts(
                &local_name,
                package_facts.type_env(),
                receiver_names,
            );
        }
    }
}

fn imported_receiver_names_by_local_name(
    package_type_env: &TypeEnv,
) -> BTreeMap<String, HashSet<String>> {
    let mut receiver_names = BTreeMap::new();
    for (_, go_type) in package_type_env.top_level_var_types_snapshot() {
        collect_imported_receiver_names_from_type(&go_type, &mut receiver_names);
    }
    receiver_names
}

fn collect_imported_receiver_names_from_type(
    go_type: &crate::compiler::typeinfer::GoType,
    receiver_names: &mut BTreeMap<String, HashSet<String>>,
) {
    use crate::compiler::typeinfer::GoType;

    match go_type {
        GoType::Named(name) | GoType::Interface(name) => {
            if let Some((package_name, receiver_name)) = name.split_once('.') {
                receiver_names
                    .entry(package_name.to_string())
                    .or_default()
                    .insert(receiver_name.to_string());
            }
        }
        GoType::Instantiated { name, args } => {
            if let Some((package_name, receiver_name)) = name.split_once('.') {
                receiver_names
                    .entry(package_name.to_string())
                    .or_default()
                    .insert(receiver_name.to_string());
            }
            for arg in args {
                collect_imported_receiver_names_from_type(arg, receiver_names);
            }
        }
        GoType::Pointer(inner) | GoType::Slice(inner) | GoType::Array(inner) => {
            collect_imported_receiver_names_from_type(inner, receiver_names);
        }
        GoType::Map(key, value) => {
            collect_imported_receiver_names_from_type(key, receiver_names);
            collect_imported_receiver_names_from_type(value, receiver_names);
        }
        GoType::Chan { elem, .. } => {
            collect_imported_receiver_names_from_type(elem, receiver_names);
        }
        GoType::Func {
            params, results, ..
        } => {
            for go_type in params.iter().chain(results.iter()) {
                collect_imported_receiver_names_from_type(go_type, receiver_names);
            }
        }
        GoType::Bool
        | GoType::Int
        | GoType::Int8
        | GoType::Int16
        | GoType::Int32
        | GoType::Int64
        | GoType::Uint
        | GoType::Uint8
        | GoType::Uint16
        | GoType::Uint32
        | GoType::Uint64
        | GoType::Uintptr
        | GoType::Float32
        | GoType::Float64
        | GoType::Complex64
        | GoType::Complex128
        | GoType::String
        | GoType::Any
        | GoType::Error
        | GoType::Unit
        | GoType::Unknown => {}
    }
}

fn import_type_env_local_name(
    import: &crate::ast::ImportSpec<'_>,
    package_name: &str,
) -> Option<String> {
    import
        .name
        .as_ref()
        .and_then(|name| match name.name {
            "." | "_" => None,
            other => Some(other.to_string()),
        })
        .or_else(|| Some(package_name.to_string()))
}

struct DeclRecoveryPlan {
    non_import_index: usize,
    label: String,
    split_specs: usize,
}

#[derive(Default)]
struct RecoverySelection {
    whole_decl_indices: BTreeSet<usize>,
    spec_indices: BTreeMap<usize, BTreeSet<usize>>,
}

struct ResolvedRecoveryContext<'a> {
    import_path: &'a str,
    package_type_env: &'a TypeEnv,
    imported_type_envs: &'a BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &'a BTreeMap<String, String>,
    package_mutable_top_level_vars: &'a HashSet<String>,
    view_method_seed: &'a crate::compiler::BorrowedViewMethodSeed,
}

fn recover_resolved_file_items<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    context: &ResolvedRecoveryContext<'_>,
) -> Vec<syn::Item> {
    let plans = recovery_plans_for_file(filename, content, reachable_names);
    let mut fallback_items = Vec::new();
    let mut selection = RecoverySelection::default();
    for plan in plans {
        let Some(shard) = parse_recovery_shard(
            filename,
            content,
            reachable_names,
            plan.non_import_index,
            None,
        ) else {
            continue;
        };
        match compile_resolved_file(
            shard,
            context.package_type_env,
            context.imported_type_envs,
            context.import_renames,
            context.package_mutable_top_level_vars,
            context.view_method_seed,
        ) {
            Ok(compiled) => {
                selection.whole_decl_indices.insert(plan.non_import_index);
                fallback_items.extend(compiled.items);
                continue;
            }
            Err(error) => {
                log_skip(format_args!(
                    "[gors] skip {}/{filename} {}: compile error: {error}",
                    context.import_path, plan.label
                ));
            }
        }

        for spec_index in 0..plan.split_specs {
            let Some(shard) = parse_recovery_shard(
                filename,
                content,
                reachable_names,
                plan.non_import_index,
                Some(spec_index),
            ) else {
                continue;
            };
            let label = spec_label_for_shard(&shard)
                .unwrap_or_else(|| format!("{} spec {}", plan.label, spec_index.saturating_add(1)));
            match compile_resolved_file(
                shard,
                context.package_type_env,
                context.imported_type_envs,
                context.import_renames,
                context.package_mutable_top_level_vars,
                context.view_method_seed,
            ) {
                Ok(compiled) => {
                    selection
                        .spec_indices
                        .entry(plan.non_import_index)
                        .or_default()
                        .insert(spec_index);
                    fallback_items.extend(compiled.items);
                }
                Err(error) => {
                    log_skip(format_args!(
                        "[gors] skip {}/{filename} {label}: compile error: {error}",
                        context.import_path
                    ));
                }
            }
        }
    }
    let Some(combined) = parse_recovery_selection(filename, content, reachable_names, &selection)
    else {
        return fallback_items;
    };
    match compile_resolved_file(
        combined,
        context.package_type_env,
        context.imported_type_envs,
        context.import_renames,
        context.package_mutable_top_level_vars,
        context.view_method_seed,
    ) {
        Ok(compiled) => compiled.items,
        Err(error) => {
            log_skip(format_args!(
                "[gors] skip {}/{filename} combined recovered declarations: compile error: {error}",
                context.import_path
            ));
            fallback_items
        }
    }
}

fn recovery_plans_for_file<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
) -> Vec<DeclRecoveryPlan> {
    let Some(file) = parse_filtered_file(filename, content, reachable_names) else {
        return Vec::new();
    };

    file.decls
        .iter()
        .filter(|decl| !is_import_decl(decl))
        .enumerate()
        .map(|(non_import_index, decl)| DeclRecoveryPlan {
            non_import_index,
            label: decl_label(decl),
            split_specs: splittable_spec_count(decl),
        })
        .collect()
}

fn parse_recovery_shard<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    target_non_import_index: usize,
    target_spec_index: Option<usize>,
) -> Option<crate::ast::File<'a>> {
    let file = parse_filtered_file(filename, content, reachable_names)?;
    file_with_recovery_shard(file, target_non_import_index, target_spec_index)
}

fn parse_recovery_selection<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    selection: &RecoverySelection,
) -> Option<crate::ast::File<'a>> {
    if selection.whole_decl_indices.is_empty() && selection.spec_indices.is_empty() {
        return None;
    }
    let file = parse_filtered_file(filename, content, reachable_names)?;
    file_with_recovery_selection(file, selection)
}

fn parse_filtered_file<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
) -> Option<crate::ast::File<'a>> {
    let ast = match crate::parser::parse_file(filename, content) {
        Ok(ast) => ast,
        Err(error) => {
            log_skip(format_args!("[gors] skip {filename}: parse error: {error}"));
            return None;
        }
    };
    let filtered = match reachable_names {
        Some(reachable) => filter_file_to_reachable(ast, reachable),
        None => ast,
    };
    file_has_compilable_decl(&filtered).then_some(filtered)
}

fn file_with_recovery_shard<'a>(
    mut file: crate::ast::File<'a>,
    target_non_import_index: usize,
    target_spec_index: Option<usize>,
) -> Option<crate::ast::File<'a>> {
    let mut decls = Vec::new();
    let mut selected = None;
    let mut non_import_index = 0;
    for decl in std::mem::take(&mut file.decls) {
        if is_import_decl(&decl) {
            decls.push(decl);
            continue;
        }
        if non_import_index == target_non_import_index {
            selected = match target_spec_index {
                Some(spec_index) => single_spec_decl(decl, spec_index),
                None => Some(decl),
            };
        }
        non_import_index += 1;
    }

    decls.push(selected?);
    file.decls = decls;
    Some(file)
}

fn file_with_recovery_selection<'a>(
    mut file: crate::ast::File<'a>,
    selection: &RecoverySelection,
) -> Option<crate::ast::File<'a>> {
    let mut decls = Vec::new();
    let mut non_import_index = 0;
    for decl in std::mem::take(&mut file.decls) {
        if is_import_decl(&decl) {
            decls.push(decl);
            continue;
        }
        if selection.whole_decl_indices.contains(&non_import_index) {
            decls.push(decl);
        } else if let Some(spec_indices) = selection.spec_indices.get(&non_import_index)
            && let Some(decl) = decl_with_recovery_specs(decl, spec_indices)
        {
            decls.push(decl);
        }
        non_import_index += 1;
    }

    file.decls = decls;
    file_has_compilable_decl(&file).then_some(file)
}

fn decl_with_recovery_specs<'a>(
    decl: crate::ast::Decl<'a>,
    spec_indices: &BTreeSet<usize>,
) -> Option<crate::ast::Decl<'a>> {
    let crate::ast::Decl::GenDecl(mut gen_decl) = decl else {
        return None;
    };
    if gen_decl.tok == crate::token::Token::IMPORT {
        return None;
    }
    gen_decl.specs = std::mem::take(&mut gen_decl.specs)
        .into_iter()
        .enumerate()
        .filter_map(|(idx, spec)| spec_indices.contains(&idx).then_some(spec))
        .collect();
    (!gen_decl.specs.is_empty()).then_some(crate::ast::Decl::GenDecl(gen_decl))
}

fn single_spec_decl<'a>(
    decl: crate::ast::Decl<'a>,
    target_spec_index: usize,
) -> Option<crate::ast::Decl<'a>> {
    let crate::ast::Decl::GenDecl(mut gen_decl) = decl else {
        return None;
    };
    if gen_decl.tok == crate::token::Token::IMPORT {
        return None;
    }
    let spec = std::mem::take(&mut gen_decl.specs)
        .into_iter()
        .nth(target_spec_index)?;
    gen_decl.specs = vec![spec];
    Some(crate::ast::Decl::GenDecl(gen_decl))
}

fn is_import_decl(decl: &crate::ast::Decl<'_>) -> bool {
    matches!(
        decl,
        crate::ast::Decl::GenDecl(gen_decl) if gen_decl.tok == crate::token::Token::IMPORT
    )
}

fn splittable_spec_count(decl: &crate::ast::Decl<'_>) -> usize {
    let crate::ast::Decl::GenDecl(gen_decl) = decl else {
        return 0;
    };
    if gen_decl.tok == crate::token::Token::IMPORT || gen_decl.specs.len() <= 1 {
        return 0;
    }
    gen_decl.specs.len()
}

fn decl_label(decl: &crate::ast::Decl<'_>) -> String {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(receiver) = receiver_type_name(func) {
                format!("method {receiver}.{}", func.name.name)
            } else {
                format!("func {}", func.name.name)
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            let token: &'static str = (&gen_decl.tok).into();
            let names = gen_decl
                .specs
                .iter()
                .flat_map(spec_names)
                .collect::<Vec<_>>()
                .join(", ");
            if names.is_empty() {
                format!("{token} declaration")
            } else {
                format!("{token} {names}")
            }
        }
    }
}

fn spec_label_for_shard(file: &crate::ast::File<'_>) -> Option<String> {
    file.decls
        .iter()
        .find(|decl| !is_import_decl(decl))
        .map(decl_label)
}

fn item_mod_for(import_path: &str, items: Vec<syn::Item>) -> syn::ItemMod {
    syn::ItemMod {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        unsafety: None,
        mod_token: <syn::Token![mod]>::default(),
        ident: syn::Ident::new(&module_name(import_path), proc_macro2::Span::mixed_site()),
        content: Some((syn::token::Brace::default(), items)),
        semi: None,
    }
}

fn dedupe_use_items(items: &mut Vec<syn::Item>) {
    let mut seen = Vec::<syn::ItemUse>::new();
    let mut deduped = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        let syn::Item::Use(item_use) = &item else {
            deduped.push(item);
            continue;
        };
        if seen
            .iter()
            .any(|existing| use_items_match(existing, item_use))
        {
            continue;
        }
        seen.push(item_use.clone());
        deduped.push(item);
    }
    *items = deduped;
}

fn use_items_match(left: &syn::ItemUse, right: &syn::ItemUse) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && visibilities_match(&left.vis, &right.vis)
        && use_trees_match(&left.tree, &right.tree)
}

fn visibilities_match(left: &syn::Visibility, right: &syn::Visibility) -> bool {
    match (left, right) {
        (syn::Visibility::Inherited, syn::Visibility::Inherited) => true,
        (syn::Visibility::Public(_), syn::Visibility::Public(_)) => true,
        (syn::Visibility::Restricted(left), syn::Visibility::Restricted(right)) => {
            left.in_token.is_some() == right.in_token.is_some()
                && paths_match(&left.path, &right.path)
        }
        _ => false,
    }
}

fn use_trees_match(left: &syn::UseTree, right: &syn::UseTree) -> bool {
    match (left, right) {
        (syn::UseTree::Path(left), syn::UseTree::Path(right)) => {
            left.ident == right.ident && use_trees_match(&left.tree, &right.tree)
        }
        (syn::UseTree::Name(left), syn::UseTree::Name(right)) => left.ident == right.ident,
        (syn::UseTree::Rename(left), syn::UseTree::Rename(right)) => {
            left.ident == right.ident && left.rename == right.rename
        }
        (syn::UseTree::Glob(_), syn::UseTree::Glob(_)) => true,
        (syn::UseTree::Group(left), syn::UseTree::Group(right)) => {
            left.items.len() == right.items.len()
                && left
                    .items
                    .iter()
                    .zip(right.items.iter())
                    .all(|(left, right)| use_trees_match(left, right))
        }
        _ => false,
    }
}

fn paths_match(left: &syn::Path, right: &syn::Path) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && left.segments.len() == right.segments.len()
        && left
            .segments
            .iter()
            .zip(right.segments.iter())
            .all(|(left, right)| {
                left.ident == right.ident
                    && matches!(
                        (&left.arguments, &right.arguments),
                        (syn::PathArguments::None, syn::PathArguments::None)
                    )
            })
}

fn log_skip(args: std::fmt::Arguments<'_>) {
    if std::env::var("GORS_STDLIB_TRACE").is_ok_and(|value| value == "1" || value == "true") {
        eprintln!("{args}");
    }
}

pub fn scan_type_env(import_path: &str) -> Option<(String, TypeEnv)> {
    let Some(cell) = type_env_cell(import_path) else {
        return scan_type_env_uncached(import_path);
    };
    cell.get_or_init(|| scan_type_env_uncached(import_path))
        .clone()
}

pub(crate) fn has_initialized_type_env(import_path: &str) -> bool {
    type_envs()
        .read()
        .ok()
        .and_then(|cache| cache.get(import_path).cloned())
        .is_some_and(|cell| cell.get().is_some())
}

fn type_env_cell(import_path: &str) -> Option<TypeEnvCell> {
    if let Ok(cache) = type_envs().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = type_envs().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

fn scan_type_env_uncached(import_path: &str) -> Option<(String, TypeEnv)> {
    let files = package_files(import_path)?;
    let mut env = TypeEnv::new();
    let mut package_name = None;
    let mut parsed_files = Vec::new();

    for (filename, content) in files.iter() {
        let Ok(ast) = crate::parser::parse_file(filename, content) else {
            continue;
        };
        if package_name.is_none() {
            package_name = Some(ast.name.name.to_string());
        }
        parsed_files.push(ast);
    }

    let parsed_file_refs = parsed_files.iter().collect::<Vec<_>>();
    env.scan_files(&parsed_file_refs);
    runtime_primitives::supplement_type_env(import_path, &mut env);
    let imported_type_envs = scan_imported_type_envs(import_path, &parsed_file_refs);
    refresh_borrowed_slice_params_with_imports(&mut env, &parsed_file_refs, &imported_type_envs);
    refresh_top_level_vars_with_imports(&mut env, &parsed_file_refs, &imported_type_envs);

    package_name.map(|name| (name, env))
}

pub fn collect_transitive_imports(import_path: &str) -> Vec<String> {
    let Some(cell) = transitive_imports_cell(import_path) else {
        return collect_transitive_imports_uncached(import_path);
    };
    cell.get_or_init(|| collect_transitive_imports_uncached(import_path))
        .clone()
}

fn transitive_imports_cell(import_path: &str) -> Option<Arc<OnceLock<Vec<String>>>> {
    if let Ok(cache) = transitive_imports().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = transitive_imports().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

pub fn collect_resolved_imports(import_path: &str, roots: &HashSet<String>) -> Vec<String> {
    let cache_key = resolve_cache_key(import_path, Some(roots));
    if let Ok(cache) = resolved_modules().read()
        && let Some((_, slot)) =
            reusable_resolved_module_slot(&cache, import_path, Some(roots), &cache_key)
        && let Some(imports) = slot.entry.get().and_then(ResolvedModuleEntry::imports)
    {
        return imports.to_vec();
    }
    collect_transitive_imports(import_path)
}

fn collect_transitive_imports_uncached(import_path: &str) -> Vec<String> {
    let Some(package) = embedded_package(import_path) else {
        return Vec::new();
    };
    package
        .direct_imports
        .iter()
        .copied()
        .filter(|path| *path != import_path && is_known(path))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
fn reachable_package_names(
    parsed_files: &[(&str, crate::ast::File<'_>)],
    roots: &HashSet<String>,
) -> HashSet<String> {
    reachable_package_names_with_imports(parsed_files, roots, &BTreeMap::new())
}

fn reachable_package_names_with_imports(
    parsed_files: &[(&str, crate::ast::File<'_>)],
    roots: &HashSet<String>,
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) -> HashSet<String> {
    let top_names = package_top_level_names(parsed_files);
    let mut env = TypeEnv::new();
    let file_refs = parsed_files
        .iter()
        .map(|(_, file)| file)
        .collect::<Vec<_>>();
    env.scan_files(&file_refs);
    refresh_top_level_vars_with_imports(&mut env, &file_refs, imported_type_envs);
    for (_, file) in parsed_files {
        crate::compiler::merge_import_type_envs(
            &mut env,
            file,
            &BTreeMap::new(),
            imported_type_envs,
        );
    }
    let interface_method_roots = interface_method_roots(roots, &top_names, &env);
    let decls = parsed_files
        .iter()
        .flat_map(|(_, file)| file.decls.iter())
        .collect::<Vec<_>>();
    let mut decls_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut receiver_methods: HashMap<String, Vec<String>> = HashMap::new();
    for (index, decl) in decls.iter().enumerate() {
        for name in decl_names(decl) {
            decls_by_name.entry(name).or_default().push(index);
        }
        if let crate::ast::Decl::FuncDecl(func) = decl
            && let (Some(receiver), Some(method)) =
                (receiver_type_name(func), receiver_method_name(func))
        {
            receiver_methods.entry(receiver).or_default().push(method);
        }
    }

    let mut reachable = HashSet::new();
    let mut reachable_queue = VecDeque::new();
    let mut value_reachable = HashSet::new();
    let mut value_queue = VecDeque::new();
    let mut initial_roots = roots
        .iter()
        .filter(|name| top_names.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    initial_roots.sort();
    for root in initial_roots {
        enqueue_reachable(&mut reachable, &mut reachable_queue, root.clone());
        if !root.contains("::") && value_reachable.insert(root.clone()) {
            value_queue.push_back(root);
        }
    }
    if top_names.contains("init") {
        enqueue_reachable(&mut reachable, &mut reachable_queue, "init".to_string());
    }

    let mut processed_decls = HashSet::new();
    while !reachable_queue.is_empty() || !value_queue.is_empty() {
        while let Some(concrete) = value_queue.pop_front() {
            if let Some(methods) = receiver_methods.get(&concrete) {
                for method in methods {
                    enqueue_reachable(&mut reachable, &mut reachable_queue, method.clone());
                }
            }
            for (interface_name, methods) in &interface_method_roots {
                if !env.named_type_implements_interface(&concrete, interface_name, true) {
                    continue;
                }
                for method in methods {
                    let method_root = format!("{concrete}::{method}");
                    if top_names.contains(&method_root) {
                        enqueue_reachable(&mut reachable, &mut reachable_queue, method_root);
                    }
                }
            }

            let mut field_refs = HashSet::new();
            let concrete_only = HashSet::from([concrete.clone()]);
            if let Some(indices) = decls_by_name.get(&concrete) {
                for index in indices {
                    let Some(decl) = decls.get(*index).copied() else {
                        continue;
                    };
                    value_field_refs_from_decl(decl, &concrete_only, &mut field_refs);
                }
            }
            enqueue_value_refs(
                field_refs,
                &top_names,
                &mut reachable,
                &mut reachable_queue,
                &mut value_reachable,
                &mut value_queue,
            );
        }

        let Some(name) = reachable_queue.pop_front() else {
            continue;
        };
        let Some(indices) = decls_by_name.get(&name) else {
            continue;
        };
        for index in indices {
            if !processed_decls.insert(*index) {
                continue;
            }
            let Some(decl) = decls.get(*index).copied() else {
                continue;
            };
            for name in decl_names(decl) {
                enqueue_reachable(&mut reachable, &mut reachable_queue, name);
            }

            let mut refs = HashSet::new();
            refs_from_decl(decl, &mut refs);
            let mut method_refs = HashSet::new();
            method_refs_from_decl(decl, &env, &mut method_refs);
            refs.extend(method_refs);
            for reference in refs {
                if top_names.contains(reference.as_str()) {
                    enqueue_reachable(&mut reachable, &mut reachable_queue, reference);
                }
            }

            let mut value_refs = HashSet::new();
            value_refs_from_decl(decl, &mut value_refs);
            enqueue_value_refs(
                value_refs,
                &top_names,
                &mut reachable,
                &mut reachable_queue,
                &mut value_reachable,
                &mut value_queue,
            );

            let mut type_switch_refs = HashSet::new();
            expand_type_switch_case_interface_methods(
                &mut type_switch_refs,
                &top_names,
                decl,
                &env,
            );
            for reference in type_switch_refs {
                enqueue_reachable(&mut reachable, &mut reachable_queue, reference);
            }
        }
    }

    reachable
}

fn enqueue_reachable(reachable: &mut HashSet<String>, queue: &mut VecDeque<String>, name: String) {
    if reachable.insert(name.clone()) {
        queue.push_back(name);
    }
}

fn enqueue_value_refs(
    refs: HashSet<String>,
    top_names: &HashSet<String>,
    reachable: &mut HashSet<String>,
    reachable_queue: &mut VecDeque<String>,
    value_reachable: &mut HashSet<String>,
    value_queue: &mut VecDeque<String>,
) {
    for reference in refs {
        if !top_names.contains(reference.as_str()) {
            continue;
        }
        enqueue_reachable(reachable, reachable_queue, reference.clone());
        if value_reachable.insert(reference.clone()) {
            value_queue.push_back(reference);
        }
    }
}

fn interface_method_roots(
    roots: &HashSet<String>,
    top_names: &HashSet<String>,
    env: &TypeEnv,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut method_roots: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for root in roots {
        let Some((receiver, method)) = root.split_once("::") else {
            continue;
        };
        if top_names.contains(receiver) && env.is_interface(receiver) {
            method_roots
                .entry(receiver.to_string())
                .or_default()
                .insert(method.to_string());
        }
    }
    method_roots
}

fn expand_type_switch_case_interface_methods(
    reachable: &mut HashSet<String>,
    top_names: &HashSet<String>,
    decl: &crate::ast::Decl<'_>,
    env: &TypeEnv,
) -> bool {
    let mut changed = false;
    type_switch_case_interface_methods_from_decl(decl, env, &mut |case_type, methods| {
        changed |= reachable.insert(case_type.to_string());
        for method in methods {
            let method_root = format!("{case_type}::{method}");
            if top_names.contains(&method_root) {
                changed |= reachable.insert(method_root);
            }
        }
    });
    changed
}

fn type_switch_case_interface_methods_from_decl<'a>(
    decl: &'a crate::ast::Decl<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(body) = &func.body {
                type_switch_case_interface_methods_from_block(body, env, on_case);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            type_switch_case_interface_methods_from_gen_decl(gen_decl, env, on_case);
        }
    }
}

fn type_switch_case_interface_methods_from_gen_decl<'a>(
    gen_decl: &'a crate::ast::GenDecl<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    for spec in &gen_decl.specs {
        if let crate::ast::Spec::ValueSpec(value_spec) = spec {
            for expr in value_spec.values.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
    }
}

fn type_switch_case_interface_methods_from_block<'a>(
    block: &'a crate::ast::BlockStmt<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    for stmt in &block.list {
        type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
    }
}

fn type_switch_case_interface_methods_from_stmt<'a>(
    stmt: &'a crate::ast::Stmt<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Stmt::BlockStmt(block) => {
            type_switch_case_interface_methods_from_block(block, env, on_case);
        }
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            for expr in case_clause.list.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
            for stmt in &case_clause.body {
                type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                type_switch_case_interface_methods_from_stmt(comm, env, on_case);
            }
            for stmt in &comm_clause.body {
                type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            type_switch_case_interface_methods_from_gen_decl(&decl_stmt.decl, env, on_case);
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => {
            type_switch_case_interface_methods_from_call(&defer_stmt.call, env, on_case);
        }
        crate::ast::Stmt::ExprStmt(expr_stmt) => {
            type_switch_case_interface_methods_from_expr(&expr_stmt.x, env, on_case);
        }
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            if let Some(cond) = &for_stmt.cond {
                type_switch_case_interface_methods_from_expr(cond, env, on_case);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                type_switch_case_interface_methods_from_stmt(post, env, on_case);
            }
            type_switch_case_interface_methods_from_block(&for_stmt.body, env, on_case);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => {
            type_switch_case_interface_methods_from_call(&go_stmt.call, env, on_case);
        }
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&if_stmt.cond, env, on_case);
            type_switch_case_interface_methods_from_block(&if_stmt.body, env, on_case);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                type_switch_case_interface_methods_from_stmt(else_stmt, env, on_case);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => {
            type_switch_case_interface_methods_from_expr(&inc_dec.x, env, on_case);
        }
        crate::ast::Stmt::LabeledStmt(labeled) => {
            type_switch_case_interface_methods_from_stmt(&labeled.stmt, env, on_case);
        }
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                type_switch_case_interface_methods_from_expr(key, env, on_case);
            }
            if let Some(value) = &range.value {
                type_switch_case_interface_methods_from_expr(value, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&range.x, env, on_case);
            type_switch_case_interface_methods_from_block(&range.body, env, on_case);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            for expr in &return_stmt.results {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => {
            type_switch_case_interface_methods_from_block(&select_stmt.body, env, on_case);
        }
        crate::ast::Stmt::SendStmt(send_stmt) => {
            type_switch_case_interface_methods_from_expr(&send_stmt.chan, env, on_case);
            type_switch_case_interface_methods_from_expr(&send_stmt.value, env, on_case);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            if let Some(tag) = &switch_stmt.tag {
                type_switch_case_interface_methods_from_expr(tag, env, on_case);
            }
            type_switch_case_interface_methods_from_block(&switch_stmt.body, env, on_case);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            type_switch_case_interface_methods_from_stmt(&type_switch.assign, env, on_case);
            let guard_methods = type_switch_guard_interface_methods(type_switch, env);
            for stmt in &type_switch.body.list {
                let crate::ast::Stmt::CaseClause(case_clause) = stmt else {
                    type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
                    continue;
                };
                if let Some(methods) = guard_methods.as_deref()
                    && let Some(exprs) = &case_clause.list
                {
                    for expr in exprs {
                        let Some((case_type, include_pointer_methods)) =
                            type_switch_case_named_type(expr)
                        else {
                            continue;
                        };
                        if env.named_type_implements_methods(
                            case_type,
                            methods,
                            include_pointer_methods,
                        ) {
                            on_case(case_type, methods);
                        }
                    }
                }
                for stmt in &case_clause.body {
                    type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
                }
            }
        }
    }
}

fn type_switch_case_interface_methods_from_expr<'a>(
    expr: &'a crate::ast::Expr<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                type_switch_case_interface_methods_from_expr(len, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&array.elt, env, on_case);
        }
        crate::ast::Expr::BasicLit(_) | crate::ast::Expr::Ident(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            type_switch_case_interface_methods_from_expr(&binary.x, env, on_case);
            type_switch_case_interface_methods_from_expr(&binary.y, env, on_case);
        }
        crate::ast::Expr::CallExpr(call) => {
            type_switch_case_interface_methods_from_call(call, env, on_case);
        }
        crate::ast::Expr::ChanType(chan) => {
            type_switch_case_interface_methods_from_expr(&chan.value, env, on_case);
        }
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                type_switch_case_interface_methods_from_expr(type_expr, env, on_case);
            }
            for expr in composite.elts.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                type_switch_case_interface_methods_from_expr(elt, env, on_case);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            type_switch_case_interface_methods_from_block(&func_lit.body, env, on_case);
        }
        crate::ast::Expr::FuncType(_)
        | crate::ast::Expr::InterfaceType(_)
        | crate::ast::Expr::MapType(_)
        | crate::ast::Expr::StructType(_) => {}
        crate::ast::Expr::IndexExpr(index) => {
            type_switch_case_interface_methods_from_expr(&index.x, env, on_case);
            type_switch_case_interface_methods_from_expr(&index.index, env, on_case);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            type_switch_case_interface_methods_from_expr(&index.x, env, on_case);
            for expr in &index.indices {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            type_switch_case_interface_methods_from_expr(&key_value.key, env, on_case);
            type_switch_case_interface_methods_from_expr(&key_value.value, env, on_case);
        }
        crate::ast::Expr::ParenExpr(paren) => {
            type_switch_case_interface_methods_from_expr(&paren.x, env, on_case);
        }
        crate::ast::Expr::SelectorExpr(selector) => {
            type_switch_case_interface_methods_from_expr(&selector.x, env, on_case);
        }
        crate::ast::Expr::SliceExpr(slice) => {
            type_switch_case_interface_methods_from_expr(&slice.x, env, on_case);
            if let Some(low) = slice.low.as_deref() {
                type_switch_case_interface_methods_from_expr(low, env, on_case);
            }
            if let Some(high) = slice.high.as_deref() {
                type_switch_case_interface_methods_from_expr(high, env, on_case);
            }
            if let Some(max) = slice.max.as_deref() {
                type_switch_case_interface_methods_from_expr(max, env, on_case);
            }
        }
        crate::ast::Expr::StarExpr(star) => {
            type_switch_case_interface_methods_from_expr(&star.x, env, on_case);
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            type_switch_case_interface_methods_from_expr(&type_assert.x, env, on_case);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                type_switch_case_interface_methods_from_expr(type_expr, env, on_case);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => {
            type_switch_case_interface_methods_from_expr(&unary.x, env, on_case);
        }
    }
}

fn type_switch_case_interface_methods_from_call<'a>(
    call: &'a crate::ast::CallExpr<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    type_switch_case_interface_methods_from_expr(&call.fun, env, on_case);
    for expr in call.args.as_deref().unwrap_or(&[]) {
        type_switch_case_interface_methods_from_expr(expr, env, on_case);
    }
}

fn type_switch_guard_interface_methods(
    type_switch: &crate::ast::TypeSwitchStmt<'_>,
    env: &TypeEnv,
) -> Option<Vec<String>> {
    let guard = type_switch_guard_operand(&type_switch.assign)?;
    let guard_type = env.resolve_alias(&crate::compiler::typeinfer::GoType::infer_expr(guard, env));
    let crate::compiler::typeinfer::GoType::Named(name) = guard_type else {
        return None;
    };
    env.is_interface(&name)
        .then(|| env.get_interface_methods(&name))
        .flatten()
        .filter(|methods| !methods.is_empty())
}

fn type_switch_guard_operand<'a>(
    stmt: &'a crate::ast::Stmt<'a>,
) -> Option<&'a crate::ast::Expr<'a>> {
    match stmt {
        crate::ast::Stmt::ExprStmt(expr) => type_switch_guard_operand_expr(&expr.x),
        crate::ast::Stmt::AssignStmt(assign) => {
            assign.rhs.first().and_then(type_switch_guard_operand_expr)
        }
        _ => None,
    }
}

fn type_switch_guard_operand_expr<'a>(
    expr: &'a crate::ast::Expr<'a>,
) -> Option<&'a crate::ast::Expr<'a>> {
    match expr {
        crate::ast::Expr::ParenExpr(paren) => type_switch_guard_operand_expr(&paren.x),
        crate::ast::Expr::TypeAssertExpr(assert) if assert.type_.is_none() => Some(&assert.x),
        _ => None,
    }
}

fn type_switch_case_named_type<'a>(expr: &'a crate::ast::Expr<'a>) -> Option<(&'a str, bool)> {
    match expr {
        crate::ast::Expr::Ident(ident) => Some((ident.name, false)),
        crate::ast::Expr::ParenExpr(paren) => type_switch_case_named_type(&paren.x),
        crate::ast::Expr::StarExpr(star) => {
            let (name, _) = type_switch_case_named_type(&star.x)?;
            Some((name, true))
        }
        _ => None,
    }
}

fn package_top_level_names(parsed_files: &[(&str, crate::ast::File<'_>)]) -> HashSet<String> {
    parsed_files
        .iter()
        .flat_map(|(_, file)| file.decls.iter())
        .flat_map(decl_names)
        .collect()
}

fn decl_names(decl: &crate::ast::Decl<'_>) -> Vec<String> {
    match decl {
        crate::ast::Decl::FuncDecl(func) if func.recv.is_none() => {
            vec![func.name.name.to_string()]
        }
        crate::ast::Decl::FuncDecl(func) => receiver_method_name(func).into_iter().collect(),
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .flat_map(spec_names)
            .collect::<Vec<_>>(),
    }
}

fn spec_names(spec: &crate::ast::Spec<'_>) -> Vec<String> {
    match spec {
        crate::ast::Spec::ImportSpec(_) => Vec::new(),
        crate::ast::Spec::TypeSpec(type_spec) => type_spec
            .name
            .as_ref()
            .map(|name| vec![name.name.to_string()])
            .unwrap_or_default(),
        crate::ast::Spec::ValueSpec(value_spec) => value_spec
            .names
            .iter()
            .map(|name| name.name.to_string())
            .collect(),
    }
}

fn func_decl_is_package_init(func: &crate::ast::FuncDecl<'_>) -> bool {
    func.recv.is_none() && func.name.name == "init"
}

fn receiver_type_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    func.recv
        .as_ref()
        .and_then(|recv| recv.list.first())
        .and_then(|field| field.type_.as_ref())
        .and_then(named_type_from_expr)
}

fn receiver_method_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    receiver_type_name(func).map(|receiver| format!("{receiver}::{}", func.name.name))
}

fn named_type_from_expr(expr: &crate::ast::Expr<'_>) -> Option<String> {
    match expr {
        crate::ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        crate::ast::Expr::StarExpr(star) => named_type_from_expr(&star.x),
        crate::ast::Expr::ParenExpr(paren) => named_type_from_expr(&paren.x),
        crate::ast::Expr::IndexExpr(index) => named_type_from_expr(&index.x),
        crate::ast::Expr::IndexListExpr(index) => named_type_from_expr(&index.x),
        _ => None,
    }
}

fn filter_file_to_reachable<'a>(
    mut file: crate::ast::File<'a>,
    reachable: &HashSet<String>,
) -> crate::ast::File<'a> {
    file.decls = file
        .decls
        .into_iter()
        .filter_map(|decl| filter_decl_to_reachable(decl, reachable))
        .collect();
    file
}

fn filter_decl_to_reachable<'a>(
    decl: crate::ast::Decl<'a>,
    reachable: &HashSet<String>,
) -> Option<crate::ast::Decl<'a>> {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            let keep = if func_decl_is_package_init(&func) {
                true
            } else if func.recv.is_none() {
                reachable.contains(func.name.name)
            } else {
                receiver_method_name(&func).is_some_and(|name| reachable.contains(&name))
            };
            keep.then_some(crate::ast::Decl::FuncDecl(func))
        }
        crate::ast::Decl::GenDecl(mut gen_decl) => {
            if gen_decl.tok == crate::token::Token::IMPORT {
                return Some(crate::ast::Decl::GenDecl(gen_decl));
            }
            gen_decl
                .specs
                .retain(|spec| spec_names(spec).iter().any(|name| reachable.contains(name)));
            (!gen_decl.specs.is_empty()).then_some(crate::ast::Decl::GenDecl(gen_decl))
        }
    }
}

fn file_has_compilable_decl(file: &crate::ast::File<'_>) -> bool {
    file.decls.iter().any(|decl| {
        !matches!(
            decl,
            crate::ast::Decl::GenDecl(gen_decl) if gen_decl.tok == crate::token::Token::IMPORT
        )
    })
}

fn refs_from_decl(decl: &crate::ast::Decl<'_>, refs: &mut HashSet<String>) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => refs_from_func_decl(func, refs),
        crate::ast::Decl::GenDecl(gen_decl) => refs_from_gen_decl(gen_decl, refs),
    }
}

fn refs_from_gen_decl(gen_decl: &crate::ast::GenDecl<'_>, refs: &mut HashSet<String>) {
    for spec in &gen_decl.specs {
        refs_from_spec(spec, refs);
    }
}

fn refs_from_spec(spec: &crate::ast::Spec<'_>, refs: &mut HashSet<String>) {
    match spec {
        crate::ast::Spec::ImportSpec(_) => {}
        crate::ast::Spec::TypeSpec(type_spec) => {
            refs_from_field_list(type_spec.type_params.as_ref(), refs);
            refs_from_expr(&type_spec.type_, refs);
        }
        crate::ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_expr) = &value_spec.type_ {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
        }
    }
}

fn refs_from_func_decl(func: &crate::ast::FuncDecl<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func.recv.as_ref(), refs);
    refs_from_func_type(&func.type_, refs);
    if let Some(body) = &func.body {
        refs_from_block(body, refs);
    }
}

fn refs_from_func_type(func_type: &crate::ast::FuncType<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func_type.type_params.as_ref(), refs);
    refs_from_field_list(Some(&func_type.params), refs);
    refs_from_field_list(func_type.results.as_ref(), refs);
}

fn refs_from_field_list(fields: Option<&crate::ast::FieldList<'_>>, refs: &mut HashSet<String>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        if let Some(type_expr) = &field.type_ {
            refs_from_expr(type_expr, refs);
        }
    }
}

fn refs_from_block(block: &crate::ast::BlockStmt<'_>, refs: &mut HashSet<String>) {
    for stmt in &block.list {
        refs_from_stmt(stmt, refs);
    }
}

fn refs_from_stmt(stmt: &crate::ast::Stmt<'_>, refs: &mut HashSet<String>) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            refs_from_exprs(&assign.lhs, refs);
            refs_from_exprs(&assign.rhs, refs);
        }
        crate::ast::Stmt::BlockStmt(block) => refs_from_block(block, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), refs);
            for stmt in &case_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                refs_from_stmt(comm, refs);
            }
            for stmt in &comm_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => refs_from_gen_decl(&decl_stmt.decl, refs),
        crate::ast::Stmt::DeferStmt(defer_stmt) => refs_from_call(&defer_stmt.call, refs),
        crate::ast::Stmt::ExprStmt(expr_stmt) => refs_from_expr(&expr_stmt.x, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                refs_from_expr(cond, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                refs_from_stmt(post, refs);
            }
            refs_from_block(&for_stmt.body, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => refs_from_call(&go_stmt.call, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                refs_from_stmt(init, refs);
            }
            refs_from_expr(&if_stmt.cond, refs);
            refs_from_block(&if_stmt.body, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                refs_from_stmt(else_stmt, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => refs_from_expr(&inc_dec.x, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => refs_from_stmt(&labeled.stmt, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                refs_from_expr(key, refs);
            }
            if let Some(value) = &range.value {
                refs_from_expr(value, refs);
            }
            refs_from_expr(&range.x, refs);
            refs_from_block(&range.body, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => refs_from_exprs(&return_stmt.results, refs),
        crate::ast::Stmt::SelectStmt(select_stmt) => refs_from_block(&select_stmt.body, refs),
        crate::ast::Stmt::SendStmt(send_stmt) => {
            refs_from_expr(&send_stmt.chan, refs);
            refs_from_expr(&send_stmt.value, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                refs_from_expr(tag, refs);
            }
            refs_from_block(&switch_stmt.body, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            refs_from_stmt(&type_switch.assign, refs);
            refs_from_block(&type_switch.body, refs);
        }
    }
}

fn refs_from_call(call: &crate::ast::CallExpr<'_>, refs: &mut HashSet<String>) {
    refs_from_expr(&call.fun, refs);
    refs_from_exprs(call.args.as_deref().unwrap_or(&[]), refs);
}

fn refs_from_exprs(exprs: &[crate::ast::Expr<'_>], refs: &mut HashSet<String>) {
    for expr in exprs {
        refs_from_expr(expr, refs);
    }
}

fn refs_from_expr(expr: &crate::ast::Expr<'_>, refs: &mut HashSet<String>) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                refs_from_expr(len, refs);
            }
            refs_from_expr(&array.elt, refs);
        }
        crate::ast::Expr::BasicLit(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            refs_from_expr(&binary.x, refs);
            refs_from_expr(&binary.y, refs);
        }
        crate::ast::Expr::CallExpr(call) => refs_from_call(call, refs),
        crate::ast::Expr::ChanType(chan) => refs_from_expr(&chan.value, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                refs_from_expr(elt, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            refs_from_func_type(&func_lit.type_, refs);
            refs_from_block(&func_lit.body, refs);
        }
        crate::ast::Expr::FuncType(func_type) => refs_from_func_type(func_type, refs),
        crate::ast::Expr::Ident(ident) => {
            if ident.name != "_" {
                refs.insert(ident.name.to_string());
            }
        }
        crate::ast::Expr::IndexExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_expr(&index.index, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_exprs(&index.indices, refs);
        }
        crate::ast::Expr::InterfaceType(interface) => {
            refs_from_field_list(interface.methods.as_ref(), refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            refs_from_expr(&key_value.key, refs);
            refs_from_expr(&key_value.value, refs);
        }
        crate::ast::Expr::MapType(map) => {
            refs_from_expr(&map.key, refs);
            refs_from_expr(&map.value, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => refs_from_expr(&paren.x, refs),
        crate::ast::Expr::SelectorExpr(selector) => refs_from_expr(&selector.x, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            refs_from_expr(&slice.x, refs);
            if let Some(low) = slice.low.as_deref() {
                refs_from_expr(low, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                refs_from_expr(high, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                refs_from_expr(max, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => refs_from_expr(&star.x, refs),
        crate::ast::Expr::StructType(struct_type) => {
            refs_from_field_list(struct_type.fields.as_ref(), refs);
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            refs_from_expr(&type_assert.x, refs);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => refs_from_expr(&unary.x, refs),
    }
}

fn method_refs_from_decl(decl: &crate::ast::Decl<'_>, env: &TypeEnv, refs: &mut HashSet<String>) {
    let mut env = env.clone();
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            seed_func_decl_method_ref_bindings(func, &mut env);
            let return_types = func_result_types_for_method_refs(func);
            if let Some(body) = &func.body {
                method_refs_from_block(body, &mut env, refs, &return_types);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            method_refs_from_gen_decl(gen_decl, &mut env, refs);
        }
    }
}

fn func_result_types_for_method_refs(
    func: &crate::ast::FuncDecl<'_>,
) -> Vec<crate::compiler::typeinfer::GoType> {
    field_list_types_for_method_refs(func.type_.results.as_ref())
}

fn field_list_types_for_method_refs(
    fields: Option<&crate::ast::FieldList<'_>>,
) -> Vec<crate::compiler::typeinfer::GoType> {
    fields
        .map(|fields| {
            fields
                .list
                .iter()
                .flat_map(|field| {
                    let ty = field
                        .type_
                        .as_ref()
                        .map(crate::compiler::typeinfer::GoType::from_expr)
                        .unwrap_or(crate::compiler::typeinfer::GoType::Unknown);
                    std::iter::repeat_n(ty, field.names.as_ref().map_or(1, Vec::len))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn seed_func_decl_method_ref_bindings(func: &crate::ast::FuncDecl<'_>, env: &mut TypeEnv) {
    seed_field_list_method_ref_bindings(func.recv.as_ref(), env);
    seed_field_list_method_ref_bindings(Some(&func.type_.params), env);
    seed_field_list_method_ref_bindings(func.type_.results.as_ref(), env);
}

fn seed_field_list_method_ref_bindings(
    fields: Option<&crate::ast::FieldList<'_>>,
    env: &mut TypeEnv,
) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        let Some(type_expr) = &field.type_ else {
            continue;
        };
        let Some(names) = field.names.as_ref() else {
            continue;
        };
        let ty = crate::compiler::typeinfer::GoType::from_expr(type_expr);
        for name in names {
            if name.name != "_" {
                env.set_var(name.name, ty.clone());
            }
        }
    }
}

fn method_refs_from_gen_decl(
    gen_decl: &crate::ast::GenDecl<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    for spec in &gen_decl.specs {
        let crate::ast::Spec::ValueSpec(value_spec) = spec else {
            continue;
        };
        if let Some(values) = value_spec.values.as_deref() {
            method_refs_from_exprs(values, env, refs);
            method_refs_from_value_spec_interface_dependencies(value_spec, values, env, refs);
        }
        if let Some(type_expr) = &value_spec.type_ {
            let ty = crate::compiler::typeinfer::GoType::from_expr(type_expr);
            for name in &value_spec.names {
                if name.name != "_" {
                    env.set_var(name.name, ty.clone());
                }
            }
        } else if let Some(values) = value_spec.values.as_deref() {
            for (name, value) in value_spec.names.iter().zip(values.iter()) {
                if name.name != "_" {
                    let ty = crate::compiler::typeinfer::GoType::infer_expr(value, env);
                    env.set_var(name.name, ty);
                }
            }
        }
    }
}

fn method_refs_from_value_spec_interface_dependencies(
    value_spec: &crate::ast::ValueSpec<'_>,
    values: &[crate::ast::Expr<'_>],
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    let Some(type_expr) = &value_spec.type_ else {
        return;
    };
    let expected = crate::compiler::typeinfer::GoType::from_expr(type_expr);
    for value in values {
        method_refs_from_interface_value(&expected, value, env, refs);
    }
}

fn method_refs_from_block(
    block: &crate::ast::BlockStmt<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
    return_types: &[crate::compiler::typeinfer::GoType],
) {
    let mut block_env = env.clone();
    for stmt in &block.list {
        method_refs_from_stmt(stmt, &mut block_env, refs, return_types);
    }
}

fn method_refs_from_stmt(
    stmt: &crate::ast::Stmt<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
    return_types: &[crate::compiler::typeinfer::GoType],
) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            method_refs_from_exprs(&assign.lhs, env, refs);
            method_refs_from_exprs(&assign.rhs, env, refs);
            method_refs_from_assignment_interface_dependencies(assign, env, refs);
            if assign.tok == crate::token::Token::DEFINE && assign.lhs.len() == assign.rhs.len() {
                for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
                    let crate::ast::Expr::Ident(ident) = lhs else {
                        continue;
                    };
                    if ident.name != "_" {
                        let ty = crate::compiler::typeinfer::GoType::infer_expr(rhs, env);
                        env.set_var(ident.name, ty);
                    }
                }
            }
        }
        crate::ast::Stmt::BlockStmt(block) => {
            method_refs_from_block(block, env, refs, return_types)
        }
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            method_refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), env, refs);
            let mut case_env = env.clone();
            for stmt in &case_clause.body {
                method_refs_from_stmt(stmt, &mut case_env, refs, return_types);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            let mut comm_env = env.clone();
            if let Some(comm) = comm_clause.comm.as_deref() {
                method_refs_from_stmt(comm, &mut comm_env, refs, return_types);
            }
            for stmt in &comm_clause.body {
                method_refs_from_stmt(stmt, &mut comm_env, refs, return_types);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            method_refs_from_gen_decl(&decl_stmt.decl, env, refs);
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => {
            method_refs_from_call(&defer_stmt.call, env, refs);
        }
        crate::ast::Stmt::ExprStmt(expr_stmt) => method_refs_from_expr(&expr_stmt.x, env, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            if let Some(init) = for_stmt.init.as_deref() {
                method_refs_from_stmt(init, &mut loop_env, refs, return_types);
            }
            if let Some(cond) = &for_stmt.cond {
                method_refs_from_expr(cond, &mut loop_env, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                method_refs_from_stmt(post, &mut loop_env, refs, return_types);
            }
            method_refs_from_block(&for_stmt.body, &mut loop_env, refs, return_types);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => method_refs_from_call(&go_stmt.call, env, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                method_refs_from_stmt(init, &mut if_env, refs, return_types);
            }
            method_refs_from_expr(&if_stmt.cond, &mut if_env, refs);
            method_refs_from_block(&if_stmt.body, &mut if_env, refs, return_types);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                method_refs_from_stmt(else_stmt, &mut if_env, refs, return_types);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => method_refs_from_expr(&inc_dec.x, env, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => {
            method_refs_from_stmt(&labeled.stmt, env, refs, return_types)
        }
        crate::ast::Stmt::RangeStmt(range) => {
            method_refs_from_expr(&range.x, env, refs);
            let mut range_env = env.clone();
            if range.tok == Some(crate::token::Token::DEFINE) {
                seed_range_method_ref_bindings(range, &mut range_env, env);
            }
            if let Some(key) = &range.key {
                method_refs_from_expr(key, &mut range_env, refs);
            }
            if let Some(value) = &range.value {
                method_refs_from_expr(value, &mut range_env, refs);
            }
            method_refs_from_block(&range.body, &mut range_env, refs, return_types);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            method_refs_from_exprs(&return_stmt.results, env, refs);
            method_refs_from_return_interface_dependencies(
                &return_stmt.results,
                return_types,
                env,
                refs,
            );
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => {
            method_refs_from_block(&select_stmt.body, env, refs, return_types)
        }
        crate::ast::Stmt::SendStmt(send_stmt) => {
            method_refs_from_expr(&send_stmt.chan, env, refs);
            method_refs_from_expr(&send_stmt.value, env, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            let mut switch_env = env.clone();
            if let Some(init) = switch_stmt.init.as_deref() {
                method_refs_from_stmt(init, &mut switch_env, refs, return_types);
            }
            if let Some(tag) = &switch_stmt.tag {
                method_refs_from_expr(tag, &mut switch_env, refs);
            }
            method_refs_from_block(&switch_stmt.body, &mut switch_env, refs, return_types);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = type_switch.init.as_deref() {
                method_refs_from_stmt(init, &mut switch_env, refs, return_types);
            }
            method_refs_from_stmt(&type_switch.assign, &mut switch_env, refs, return_types);
            method_refs_from_block(&type_switch.body, &mut switch_env, refs, return_types);
        }
    }
}

fn method_refs_from_assignment_interface_dependencies(
    assign: &crate::ast::AssignStmt<'_>,
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    if assign.lhs.len() != assign.rhs.len() {
        return;
    }
    for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
        let expected = crate::compiler::typeinfer::GoType::infer_expr(lhs, env);
        method_refs_from_interface_value(&expected, rhs, env, refs);
    }
}

fn method_refs_from_return_interface_dependencies(
    results: &[crate::ast::Expr<'_>],
    return_types: &[crate::compiler::typeinfer::GoType],
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    if results.len() != return_types.len() {
        return;
    }
    for (expected, result) in return_types.iter().zip(results.iter()) {
        method_refs_from_interface_value(expected, result, env, refs);
    }
}

fn seed_range_method_ref_bindings(
    range: &crate::ast::RangeStmt<'_>,
    range_env: &mut TypeEnv,
    outer_env: &TypeEnv,
) {
    let container_ty = outer_env.resolve_alias(&crate::compiler::typeinfer::GoType::infer_expr(
        &range.x, outer_env,
    ));
    let (key_ty, value_ty) = match container_ty {
        crate::compiler::typeinfer::GoType::Slice(elem)
        | crate::compiler::typeinfer::GoType::Array(elem) => {
            (crate::compiler::typeinfer::GoType::Int, *elem)
        }
        crate::compiler::typeinfer::GoType::Map(key, value) => (*key, *value),
        crate::compiler::typeinfer::GoType::String => (
            crate::compiler::typeinfer::GoType::Int,
            crate::compiler::typeinfer::GoType::Int32,
        ),
        _ => (
            crate::compiler::typeinfer::GoType::Unknown,
            crate::compiler::typeinfer::GoType::Unknown,
        ),
    };
    if let Some(crate::ast::Expr::Ident(ident)) = &range.key
        && ident.name != "_"
    {
        range_env.set_var(ident.name, key_ty);
    }
    if let Some(crate::ast::Expr::Ident(ident)) = &range.value
        && ident.name != "_"
    {
        range_env.set_var(ident.name, value_ty);
    }
}

fn method_refs_from_call(
    call: &crate::ast::CallExpr<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    if let crate::ast::Expr::SelectorExpr(selector) = call.fun.as_ref()
        && let Some(receiver) = receiver_name_for_method_expr(&selector.x, env)
    {
        refs.insert(format!("{receiver}::{}", selector.sel.name));
    }
    method_refs_from_interface_arg_dependencies(call, env, refs);
    method_refs_from_expr(&call.fun, env, refs);
    method_refs_from_exprs(call.args.as_deref().unwrap_or(&[]), env, refs);
}

fn method_refs_from_interface_arg_dependencies(
    call: &crate::ast::CallExpr<'_>,
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    let params = call_param_types_for_method_refs(call, env);
    let Some(args) = call.args.as_deref() else {
        return;
    };
    for (expected, arg) in params.iter().zip(args.iter()) {
        method_refs_from_interface_value(expected, arg, env, refs);
    }
}

fn call_param_types_for_method_refs(
    call: &crate::ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Vec<crate::compiler::typeinfer::GoType> {
    match call.fun.as_ref() {
        crate::ast::Expr::Ident(ident) => env.get_func_params(ident.name),
        crate::ast::Expr::SelectorExpr(selector) => {
            if let crate::ast::Expr::Ident(pkg_or_recv) = selector.x.as_ref() {
                let package_key = format!("{}.{}", pkg_or_recv.name, selector.sel.name);
                let package_params = env.get_func_params(&package_key);
                if !package_params.is_empty() {
                    return package_params;
                }
            }
            let receiver_type = crate::compiler::typeinfer::GoType::infer_expr(&selector.x, env);
            for receiver in receiver_names_for_method_lookup(&receiver_type, env) {
                let params = env.get_method_params(&receiver, selector.sel.name);
                if !params.is_empty() {
                    return params;
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn receiver_names_for_method_lookup(
    ty: &crate::compiler::typeinfer::GoType,
    env: &TypeEnv,
) -> Vec<String> {
    let Some(name) = receiver_name_from_go_type_preserving_package(ty, env) else {
        return Vec::new();
    };
    let local = local_receiver_name(&name);
    if local == name {
        vec![name]
    } else {
        vec![name, local]
    }
}

fn interface_methods_for_method_refs(
    ty: &crate::compiler::typeinfer::GoType,
    env: &TypeEnv,
) -> Option<Vec<String>> {
    match env.resolve_alias(ty) {
        crate::compiler::typeinfer::GoType::Error => Some(vec!["Error".to_string()]),
        crate::compiler::typeinfer::GoType::Named(name)
        | crate::compiler::typeinfer::GoType::Interface(name)
            if env.is_interface(&name) =>
        {
            env.get_interface_methods(&name)
        }
        _ => None,
    }
}

fn concrete_receiver_name_for_interface_arg(
    ty: &crate::compiler::typeinfer::GoType,
    env: &TypeEnv,
) -> Option<(String, bool)> {
    match ty {
        crate::compiler::typeinfer::GoType::Named(name)
        | crate::compiler::typeinfer::GoType::Instantiated { name, .. } => {
            concrete_receiver_name_if_not_interface(name, false, env)
        }
        crate::compiler::typeinfer::GoType::Pointer(inner) => match inner.as_ref() {
            crate::compiler::typeinfer::GoType::Named(name)
            | crate::compiler::typeinfer::GoType::Instantiated { name, .. } => {
                concrete_receiver_name_if_not_interface(name, true, env)
            }
            other => match env.resolve_alias(other) {
                crate::compiler::typeinfer::GoType::Named(name)
                | crate::compiler::typeinfer::GoType::Instantiated { name, .. } => {
                    concrete_receiver_name_if_not_interface(&name, true, env)
                }
                _ => None,
            },
        },
        other => match env.resolve_alias(other) {
            crate::compiler::typeinfer::GoType::Named(name)
            | crate::compiler::typeinfer::GoType::Instantiated { name, .. } => {
                concrete_receiver_name_if_not_interface(&name, false, env)
            }
            _ => None,
        },
    }
}

fn concrete_receiver_name_if_not_interface(
    name: &str,
    include_pointer_receiver_methods: bool,
    env: &TypeEnv,
) -> Option<(String, bool)> {
    if env.is_interface(name) {
        None
    } else {
        Some((name.to_string(), include_pointer_receiver_methods))
    }
}

fn method_refs_from_interface_value(
    expected: &crate::compiler::typeinfer::GoType,
    actual: &crate::ast::Expr<'_>,
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    let Some(methods) = interface_methods_for_method_refs(expected, env) else {
        return;
    };
    if methods.is_empty() {
        return;
    }
    let actual = crate::compiler::typeinfer::GoType::infer_expr(actual, env);
    let Some((type_name, include_pointer_receiver_methods)) =
        concrete_receiver_name_for_interface_arg(&actual, env)
    else {
        return;
    };
    if type_name.contains('.') {
        return;
    }
    if !env.named_type_implements_methods(&type_name, &methods, include_pointer_receiver_methods) {
        return;
    }
    for method in methods {
        refs.insert(format!("{type_name}::{method}"));
    }
}

fn method_refs_from_exprs(
    exprs: &[crate::ast::Expr<'_>],
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    for expr in exprs {
        method_refs_from_expr(expr, env, refs);
    }
}

fn method_refs_from_expr(
    expr: &crate::ast::Expr<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                method_refs_from_expr(len, env, refs);
            }
            method_refs_from_expr(&array.elt, env, refs);
        }
        crate::ast::Expr::BasicLit(_) | crate::ast::Expr::Ident(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            method_refs_from_expr(&binary.x, env, refs);
            method_refs_from_expr(&binary.y, env, refs);
        }
        crate::ast::Expr::CallExpr(call) => method_refs_from_call(call, env, refs),
        crate::ast::Expr::ChanType(chan) => method_refs_from_expr(&chan.value, env, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                method_refs_from_expr(type_expr, env, refs);
            }
            method_refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), env, refs);
            method_refs_from_composite_literal_interface_dependencies(composite, env, refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                method_refs_from_expr(elt, env, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            let mut func_env = env.clone();
            seed_field_list_method_ref_bindings(Some(&func_lit.type_.params), &mut func_env);
            seed_field_list_method_ref_bindings(func_lit.type_.results.as_ref(), &mut func_env);
            let return_types = field_list_types_for_method_refs(func_lit.type_.results.as_ref());
            method_refs_from_block(&func_lit.body, &mut func_env, refs, &return_types);
        }
        crate::ast::Expr::FuncType(_) | crate::ast::Expr::InterfaceType(_) => {}
        crate::ast::Expr::IndexExpr(index) => {
            method_refs_from_expr(&index.x, env, refs);
            method_refs_from_expr(&index.index, env, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            method_refs_from_expr(&index.x, env, refs);
            method_refs_from_exprs(&index.indices, env, refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            method_refs_from_expr(&key_value.key, env, refs);
            method_refs_from_expr(&key_value.value, env, refs);
        }
        crate::ast::Expr::MapType(map) => {
            method_refs_from_expr(&map.key, env, refs);
            method_refs_from_expr(&map.value, env, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => method_refs_from_expr(&paren.x, env, refs),
        crate::ast::Expr::SelectorExpr(selector) => method_refs_from_expr(&selector.x, env, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            method_refs_from_expr(&slice.x, env, refs);
            if let Some(low) = slice.low.as_deref() {
                method_refs_from_expr(low, env, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                method_refs_from_expr(high, env, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                method_refs_from_expr(max, env, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => method_refs_from_expr(&star.x, env, refs),
        crate::ast::Expr::StructType(struct_type) => {
            if let Some(fields) = struct_type.fields.as_ref() {
                for field in &fields.list {
                    if let Some(type_expr) = &field.type_ {
                        method_refs_from_expr(type_expr, env, refs);
                    }
                }
            }
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            method_refs_from_expr(&type_assert.x, env, refs);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                method_refs_from_expr(type_expr, env, refs);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => method_refs_from_expr(&unary.x, env, refs),
    }
}

fn method_refs_from_composite_literal_interface_dependencies(
    composite: &crate::ast::CompositeLit<'_>,
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    let Some(type_expr) = composite.type_.as_deref() else {
        return;
    };
    let Some(elts) = composite.elts.as_deref() else {
        return;
    };
    match env.resolve_alias(&crate::compiler::typeinfer::GoType::from_expr(type_expr)) {
        crate::compiler::typeinfer::GoType::Named(name) => {
            method_refs_from_struct_literal_interface_dependencies(
                &env.get_struct_fields(&name),
                elts,
                env,
                refs,
            );
        }
        crate::compiler::typeinfer::GoType::Instantiated { name, args } => {
            method_refs_from_struct_literal_interface_dependencies(
                &env.get_struct_fields_with_type_args(&name, &args),
                elts,
                env,
                refs,
            );
        }
        crate::compiler::typeinfer::GoType::Slice(elem)
        | crate::compiler::typeinfer::GoType::Array(elem) => {
            for elt in elts {
                let value = match elt {
                    crate::ast::Expr::KeyValueExpr(key_value) => key_value.value.as_ref(),
                    other => other,
                };
                method_refs_from_interface_value(&elem, value, env, refs);
            }
        }
        crate::compiler::typeinfer::GoType::Map(key, value) => {
            for elt in elts {
                let crate::ast::Expr::KeyValueExpr(key_value) = elt else {
                    continue;
                };
                method_refs_from_interface_value(&key, &key_value.key, env, refs);
                method_refs_from_interface_value(&value, &key_value.value, env, refs);
            }
        }
        _ => {}
    }
}

fn method_refs_from_struct_literal_interface_dependencies(
    fields: &[(String, crate::compiler::typeinfer::GoType)],
    elts: &[crate::ast::Expr<'_>],
    env: &TypeEnv,
    refs: &mut HashSet<String>,
) {
    let mut positional_index = 0usize;
    for elt in elts {
        if let crate::ast::Expr::KeyValueExpr(key_value) = elt {
            let crate::ast::Expr::Ident(field_name) = key_value.key.as_ref() else {
                continue;
            };
            if let Some((_, expected)) = fields
                .iter()
                .find(|(candidate, _)| candidate == field_name.name)
            {
                method_refs_from_interface_value(expected, &key_value.value, env, refs);
            }
        } else if let Some((_, expected)) = fields.get(positional_index) {
            method_refs_from_interface_value(expected, elt, env, refs);
            positional_index += 1;
        }
    }
}

fn receiver_name_for_method_expr(expr: &crate::ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    let ty = crate::compiler::typeinfer::GoType::infer_expr(expr, env);
    receiver_name_from_go_type(&ty)
}

fn receiver_name_from_go_type(ty: &crate::compiler::typeinfer::GoType) -> Option<String> {
    receiver_name_from_go_type_preserving_package(ty, &TypeEnv::new())
        .map(|name| local_receiver_name(&name))
}

fn receiver_name_from_go_type_preserving_package(
    ty: &crate::compiler::typeinfer::GoType,
    env: &TypeEnv,
) -> Option<String> {
    match ty {
        crate::compiler::typeinfer::GoType::Named(name)
        | crate::compiler::typeinfer::GoType::Instantiated { name, .. }
        | crate::compiler::typeinfer::GoType::Interface(name) => Some(name.clone()),
        crate::compiler::typeinfer::GoType::Pointer(inner) => {
            receiver_name_from_go_type_preserving_package(inner, env)
        }
        other => match env.resolve_alias(other) {
            crate::compiler::typeinfer::GoType::Named(name)
            | crate::compiler::typeinfer::GoType::Instantiated { name, .. }
            | crate::compiler::typeinfer::GoType::Interface(name) => Some(name),
            crate::compiler::typeinfer::GoType::Pointer(inner) => {
                receiver_name_from_go_type_preserving_package(&inner, env)
            }
            _ => None,
        },
    }
}

fn local_receiver_name(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

fn value_refs_from_decl(decl: &crate::ast::Decl<'_>, refs: &mut HashSet<String>) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(body) = &func.body {
                value_refs_from_block(body, refs);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            for spec in &gen_decl.specs {
                if let crate::ast::Spec::ValueSpec(value_spec) = spec {
                    value_refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
                }
            }
        }
    }
}

fn value_field_refs_from_decl(
    decl: &crate::ast::Decl<'_>,
    value_reachable: &HashSet<String>,
    refs: &mut HashSet<String>,
) {
    let crate::ast::Decl::GenDecl(gen_decl) = decl else {
        return;
    };
    for spec in &gen_decl.specs {
        let crate::ast::Spec::TypeSpec(type_spec) = spec else {
            continue;
        };
        let Some(type_name) = type_spec.name.as_ref().map(|name| name.name) else {
            continue;
        };
        if !value_reachable.contains(type_name) {
            continue;
        }
        let crate::ast::Expr::StructType(struct_type) = &type_spec.type_ else {
            continue;
        };
        let Some(fields) = struct_type.fields.as_ref() else {
            continue;
        };
        for field in &fields.list {
            if let Some(type_expr) = &field.type_ {
                refs_from_expr(type_expr, refs);
            }
        }
    }
}

fn value_refs_from_block(block: &crate::ast::BlockStmt<'_>, refs: &mut HashSet<String>) {
    for stmt in &block.list {
        value_refs_from_stmt(stmt, refs);
    }
}

fn value_refs_from_stmt(stmt: &crate::ast::Stmt<'_>, refs: &mut HashSet<String>) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            value_refs_from_exprs(&assign.lhs, refs);
            value_refs_from_exprs(&assign.rhs, refs);
        }
        crate::ast::Stmt::BlockStmt(block) => value_refs_from_block(block, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            value_refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), refs);
            for stmt in &case_clause.body {
                value_refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                value_refs_from_stmt(comm, refs);
            }
            for stmt in &comm_clause.body {
                value_refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            for spec in &decl_stmt.decl.specs {
                if let crate::ast::Spec::ValueSpec(value_spec) = spec {
                    value_refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
                }
            }
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => value_refs_from_call(&defer_stmt.call, refs),
        crate::ast::Stmt::ExprStmt(expr_stmt) => value_refs_from_expr(&expr_stmt.x, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                value_refs_from_expr(cond, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                value_refs_from_stmt(post, refs);
            }
            value_refs_from_block(&for_stmt.body, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => value_refs_from_call(&go_stmt.call, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                value_refs_from_stmt(init, refs);
            }
            value_refs_from_expr(&if_stmt.cond, refs);
            value_refs_from_block(&if_stmt.body, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                value_refs_from_stmt(else_stmt, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => value_refs_from_expr(&inc_dec.x, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => value_refs_from_stmt(&labeled.stmt, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                value_refs_from_expr(key, refs);
            }
            if let Some(value) = &range.value {
                value_refs_from_expr(value, refs);
            }
            value_refs_from_expr(&range.x, refs);
            value_refs_from_block(&range.body, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            value_refs_from_exprs(&return_stmt.results, refs)
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => value_refs_from_block(&select_stmt.body, refs),
        crate::ast::Stmt::SendStmt(send_stmt) => {
            value_refs_from_expr(&send_stmt.chan, refs);
            value_refs_from_expr(&send_stmt.value, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                value_refs_from_expr(tag, refs);
            }
            value_refs_from_block(&switch_stmt.body, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            value_refs_from_stmt(&type_switch.assign, refs);
            for stmt in &type_switch.body.list {
                if let crate::ast::Stmt::CaseClause(case_clause) = stmt {
                    for stmt in &case_clause.body {
                        value_refs_from_stmt(stmt, refs);
                    }
                }
            }
        }
    }
}

fn value_refs_from_call(call: &crate::ast::CallExpr<'_>, refs: &mut HashSet<String>) {
    value_refs_from_expr(&call.fun, refs);
    value_refs_from_exprs(call.args.as_deref().unwrap_or(&[]), refs);
}

fn value_refs_from_exprs(exprs: &[crate::ast::Expr<'_>], refs: &mut HashSet<String>) {
    for expr in exprs {
        value_refs_from_expr(expr, refs);
    }
}

fn value_refs_from_expr(expr: &crate::ast::Expr<'_>, refs: &mut HashSet<String>) {
    match expr {
        crate::ast::Expr::BasicLit(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            value_refs_from_expr(&binary.x, refs);
            value_refs_from_expr(&binary.y, refs);
        }
        crate::ast::Expr::CallExpr(call) => value_refs_from_call(call, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
            value_refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                value_refs_from_expr(elt, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => value_refs_from_block(&func_lit.body, refs),
        crate::ast::Expr::Ident(ident) => {
            if ident.name != "_" {
                refs.insert(ident.name.to_string());
            }
        }
        crate::ast::Expr::IndexExpr(index) => {
            value_refs_from_expr(&index.x, refs);
            value_refs_from_expr(&index.index, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            value_refs_from_expr(&index.x, refs);
            value_refs_from_exprs(&index.indices, refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            value_refs_from_expr(&key_value.key, refs);
            value_refs_from_expr(&key_value.value, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => value_refs_from_expr(&paren.x, refs),
        crate::ast::Expr::SelectorExpr(selector) => value_refs_from_expr(&selector.x, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            value_refs_from_expr(&slice.x, refs);
            if let Some(low) = slice.low.as_deref() {
                value_refs_from_expr(low, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                value_refs_from_expr(high, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                value_refs_from_expr(max, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => value_refs_from_expr(&star.x, refs),
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            value_refs_from_expr(&type_assert.x, refs);
        }
        crate::ast::Expr::UnaryExpr(unary) => value_refs_from_expr(&unary.x, refs),
        crate::ast::Expr::ArrayType(_)
        | crate::ast::Expr::ChanType(_)
        | crate::ast::Expr::FuncType(_)
        | crate::ast::Expr::InterfaceType(_)
        | crate::ast::Expr::MapType(_)
        | crate::ast::Expr::StructType(_) => {}
    }
}

fn package_import_renames(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> BTreeMap<String, String> {
    let mut rewrites = BTreeMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if !is_known(import_path) {
                continue;
            }
            let mod_name = module_name(import_path);
            let Some(local_name) = import_local_name(import) else {
                continue;
            };
            if local_name != mod_name {
                rewrites.insert(local_name, mod_name);
            }
        }
    }
    rewrites
}

fn package_import_path_by_module(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if is_known(import_path) {
                imports.insert(module_name(import_path), import_path.to_string());
                if let Some(local_name) = import_local_name(import) {
                    imports.insert(local_name, import_path.to_string());
                }
            }
        }
    }
    imports
}

fn used_imports_from_items(
    items: &mut [syn::Item],
    import_path_by_module: &HashMap<String, String>,
) -> Vec<String> {
    use syn::visit_mut::VisitMut;

    struct UsedImportCollector<'a> {
        import_path_by_module: &'a HashMap<String, String>,
        used: HashSet<String>,
    }

    impl VisitMut for UsedImportCollector<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if let Some(import_path) = self.import_path_by_module.get(&first) {
                self.used.insert(import_path.clone());
            }
        }
    }

    let mut collector = UsedImportCollector {
        import_path_by_module,
        used: HashSet::new(),
    };
    for item in items {
        collector.visit_item_mut(item);
    }

    let mut used: Vec<_> = collector.used.into_iter().collect();
    used.sort();
    used
}

fn import_local_name(import: &crate::ast::ImportSpec<'_>) -> Option<String> {
    if let Some(name) = &import.name {
        if name.name == "." || name.name == "_" {
            return None;
        }
        return Some(name.name.to_string());
    }

    let import_path = import.path.value.trim_matches('"');
    scan_type_env(import_path)
        .map(|(package_name, _)| package_name)
        .or_else(|| import_path.rsplit('/').next().map(str::to_string))
}

fn prefix_crate_paths(items: &mut [syn::Item], module_refs: &HashSet<String>) {
    use syn::visit_mut::VisitMut;

    struct CratePrefixer<'a> {
        module_refs: &'a HashSet<String>,
    }

    impl VisitMut for CratePrefixer<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.leading_colon.is_some() || path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if first == "builtin" || self.module_refs.contains(&first) {
                path.segments.insert(
                    0,
                    syn::PathSegment {
                        ident: syn::Ident::new("crate", proc_macro2::Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    },
                );
            }
        }
    }

    let mut prefixer = CratePrefixer { module_refs };
    for item in items.iter_mut() {
        prefixer.visit_item_mut(item);
    }
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::{GoType, TypeKind};
    use quote::ToTokens;

    fn cache_archive(
        entries: Vec<ResolvedCacheRecord>,
        type_envs: Vec<ResolvedTypeEnvRecord>,
    ) -> ResolvedCacheArchive {
        ResolvedCacheArchive {
            schema: RESOLVED_CACHE_SCHEMA,
            go_version: crate::GO_VERSION.to_string(),
            stdlib_version: crate::STDLIB_VERSION.to_string(),
            resolver_fingerprint: crate::RESOLVER_CACHE_FINGERPRINT.to_string(),
            entries,
            type_envs,
        }
    }

    fn cache_record(
        import_path: &str,
        roots: Option<Vec<&str>>,
        source: Option<&str>,
        imports: Vec<&str>,
    ) -> ResolvedCacheRecord {
        let mut record = ResolvedCacheRecord {
            import_path: import_path.to_string(),
            roots: roots.map(|roots| roots.into_iter().map(str::to_string).collect()),
            source: source.map(str::to_string),
            imports: imports.into_iter().map(str::to_string).collect(),
            integrity: String::new(),
        };
        record.integrity = resolved_record_integrity(&record).unwrap();
        record
    }

    fn type_env_record(import_path: &str) -> ResolvedTypeEnvRecord {
        let (package_name, env) = scan_type_env_uncached(import_path).unwrap();
        let env = canonical_type_env_value(&env).unwrap();
        let integrity = type_env_record_integrity(import_path, &package_name, &env).unwrap();
        ResolvedTypeEnvRecord {
            import_path: import_path.to_string(),
            package_name,
            env,
            integrity,
        }
    }

    fn initialized_resolved_slot(entry: ResolvedModuleEntry) -> Arc<ResolvedModuleSlot> {
        let slot = Arc::new(ResolvedModuleSlot::new());
        slot.entry.set(entry).unwrap();
        slot
    }

    fn source_entry(source: &str, imports: &[&str]) -> ResolvedModuleEntry {
        ResolvedModuleEntry::Source {
            source: source.to_string(),
            imports: imports.iter().map(|import| (*import).to_string()).collect(),
        }
    }

    #[test]
    fn resolved_cache_key_round_trips_sorted_roots() {
        let roots = HashSet::from(["Write".to_string(), "Read".to_string()]);
        let cache_key = resolve_cache_key("io", Some(&roots));

        assert_eq!(
            parse_resolve_cache_key(&cache_key),
            Some((
                "io".to_string(),
                Some(vec!["Read".to_string(), "Write".to_string()])
            ))
        );
        assert_eq!(
            parse_resolve_cache_key("fmt"),
            Some(("fmt".to_string(), None))
        );
    }

    #[test]
    fn resolved_cache_prefers_smallest_deterministic_initialized_superset() {
        let requested = HashSet::from(["A".to_string()]);
        let exact_key = resolve_cache_key("cmp", Some(&requested));
        let ab_key = resolve_cache_key(
            "cmp",
            Some(&HashSet::from(["A".to_string(), "B".to_string()])),
        );
        let ac_key = resolve_cache_key(
            "cmp",
            Some(&HashSet::from(["A".to_string(), "C".to_string()])),
        );
        let abc_key = resolve_cache_key(
            "cmp",
            Some(&HashSet::from([
                "A".to_string(),
                "B".to_string(),
                "C".to_string(),
            ])),
        );
        let uncacheable_key = resolve_cache_key(
            "cmp",
            Some(&HashSet::from(["A".to_string(), "AA".to_string()])),
        );
        let initializing_key = resolve_cache_key(
            "cmp",
            Some(&HashSet::from(["A".to_string(), "AB".to_string()])),
        );
        let unrelated_key = resolve_cache_key(
            "io",
            Some(&HashSet::from(["A".to_string(), "B".to_string()])),
        );
        let cache = HashMap::from([
            (
                abc_key,
                initialized_resolved_slot(source_entry("abc", &["os"])),
            ),
            (
                ac_key,
                initialized_resolved_slot(source_entry("ac", &["strings"])),
            ),
            (
                ab_key.clone(),
                initialized_resolved_slot(source_entry("ab", &["bytes"])),
            ),
            (
                uncacheable_key,
                initialized_resolved_slot(ResolvedModuleEntry::Uncacheable),
            ),
            (initializing_key, Arc::new(ResolvedModuleSlot::new())),
            (
                unrelated_key,
                initialized_resolved_slot(source_entry("unrelated", &["io"])),
            ),
        ]);

        let (selected_key, selected_slot) =
            reusable_resolved_module_slot(&cache, "cmp", Some(&requested), &exact_key).unwrap();
        assert_eq!(selected_key, ab_key);
        assert_eq!(
            selected_slot
                .entry
                .get()
                .and_then(ResolvedModuleEntry::imports),
            Some(["bytes".to_string()].as_slice())
        );
    }

    #[test]
    fn resolved_cache_exact_slot_wins_and_unfiltered_entries_stay_separate() {
        let requested = HashSet::from(["A".to_string()]);
        let exact_key = resolve_cache_key("cmp", Some(&requested));
        let full_slot = initialized_resolved_slot(source_entry("full", &["reflect"]));
        let exact_slot = Arc::new(ResolvedModuleSlot::new());
        let mut cache = HashMap::from([("cmp".to_string(), full_slot)]);

        assert!(
            reusable_resolved_module_slot(&cache, "cmp", Some(&requested), &exact_key).is_none()
        );

        cache.insert(exact_key.clone(), exact_slot.clone());
        let (selected_key, selected_slot) =
            reusable_resolved_module_slot(&cache, "cmp", Some(&requested), &exact_key).unwrap();
        assert_eq!(selected_key, exact_key);
        assert!(Arc::ptr_eq(selected_slot, &exact_slot));
    }

    #[test]
    fn imported_resolved_cache_superset_serves_module_and_dependency_metadata() {
        let narrow_root = "__gors_cache_superset_probe_a";
        let broad_root = "__gors_cache_superset_probe_b";
        let narrow_roots = HashSet::from([narrow_root.to_string()]);
        let narrow_key = resolve_cache_key("cmp", Some(&narrow_roots));
        assert!(
            resolved_modules()
                .read()
                .unwrap()
                .get(&narrow_key)
                .is_none()
        );

        let record = cache_record(
            "cmp",
            Some(vec![narrow_root, broad_root]),
            Some("pub fn __gors_cache_superset_probe() {}\n"),
            vec!["io"],
        );
        let bytes = serde_json::to_vec(&cache_archive(vec![record], Vec::new())).unwrap();
        let stats = import_resolved_module_cache(&bytes).unwrap();
        assert_eq!(stats.imported, 1);
        assert!(has_initialized_resolved_module("cmp", &narrow_roots));

        let module = resolve_with_roots("cmp", &narrow_roots).unwrap();
        assert!(
            module
                .to_token_stream()
                .to_string()
                .contains("__gors_cache_superset_probe")
        );
        assert_eq!(
            collect_resolved_imports("cmp", &narrow_roots),
            vec!["io".to_string()]
        );
        assert!(
            resolved_modules()
                .read()
                .unwrap()
                .get(&narrow_key)
                .is_none()
        );
    }

    #[test]
    fn resolved_cache_rejects_other_resolver_fingerprints() {
        let archive = ResolvedCacheArchive {
            schema: RESOLVED_CACHE_SCHEMA,
            go_version: crate::GO_VERSION.to_string(),
            stdlib_version: crate::STDLIB_VERSION.to_string(),
            resolver_fingerprint: "other-resolver".to_string(),
            entries: Vec::new(),
            type_envs: Vec::new(),
        };

        assert_eq!(
            validate_resolved_cache_archive(&archive),
            Err("resolved module cache resolver fingerprint mismatch".to_string())
        );
    }

    #[test]
    fn resolved_cache_validates_every_record_before_mutating_global_caches() {
        let probe_root = "__gors_cache_atomicity_probe";
        let probe_key = resolve_cache_key("cmp", Some(&HashSet::from([probe_root.to_string()])));
        assert!(resolved_modules().read().unwrap().get(&probe_key).is_none());

        let valid = cache_record(
            "cmp",
            Some(vec![probe_root]),
            Some("pub fn cache_atomicity_probe() {}\n"),
            Vec::new(),
        );
        let invalid = cache_record(
            "gors/cache/unknown",
            Some(vec!["Probe"]),
            Some("pub fn invalid_package() {}\n"),
            Vec::new(),
        );
        let bytes = serde_json::to_vec(&cache_archive(vec![valid, invalid], Vec::new())).unwrap();

        assert!(
            import_resolved_module_cache(&bytes)
                .unwrap_err()
                .contains("unknown package")
        );
        assert!(resolved_modules().read().unwrap().get(&probe_key).is_none());
    }

    #[test]
    fn resolved_cache_rejects_duplicate_records_and_invalid_generated_source() {
        let record = cache_record(
            "cmp",
            Some(vec!["__gors_duplicate_probe"]),
            Some("pub fn duplicate_probe() {}\n"),
            Vec::new(),
        );
        assert!(
            prepare_resolved_cache_archive(cache_archive(vec![record.clone(), record], Vec::new()))
                .unwrap_err()
                .contains("duplicate entry")
        );

        let invalid = cache_record(
            "cmp",
            Some(vec!["__gors_invalid_source_probe"]),
            Some("pub fn invalid source"),
            Vec::new(),
        );
        assert!(
            prepare_resolved_cache_archive(cache_archive(vec![invalid], Vec::new()))
                .unwrap_err()
                .contains("invalid Rust")
        );

        let duplicate_dependencies = cache_record(
            "cmp",
            Some(vec!["__gors_duplicate_dependency_probe"]),
            Some("pub fn duplicate_dependency_probe() {}\n"),
            vec!["io", "io"],
        );
        assert!(
            prepare_resolved_cache_archive(cache_archive(vec![duplicate_dependencies], Vec::new()))
                .unwrap_err()
                .contains("duplicate imports")
        );
    }

    #[test]
    fn resolved_cache_rejects_type_env_corruption_and_package_name_mismatches() {
        let mut corrupted = type_env_record("cmp");
        corrupted.integrity = "corrupted".to_string();
        assert!(
            prepare_resolved_cache_archive(cache_archive(Vec::new(), vec![corrupted]))
                .unwrap_err()
                .contains("type environment integrity mismatch")
        );

        let mut mismatched = type_env_record("cmp");
        mismatched.package_name = "not_cmp".to_string();
        mismatched.integrity =
            type_env_record_integrity("cmp", &mismatched.package_name, &mismatched.env).unwrap();
        assert!(
            prepare_resolved_cache_archive(cache_archive(Vec::new(), vec![mismatched]))
                .unwrap_err()
                .contains("package name mismatch")
        );

        let duplicate = type_env_record("cmp");
        assert!(
            prepare_resolved_cache_archive(cache_archive(
                Vec::new(),
                vec![duplicate.clone(), duplicate]
            ))
            .unwrap_err()
            .contains("duplicate type environment")
        );
    }

    #[test]
    fn type_env_wire_encoding_is_deterministic_across_independent_scans() {
        let (_, first) = scan_type_env_uncached("cmp").unwrap();
        let (_, second) = scan_type_env_uncached("cmp").unwrap();
        let first = serde_json::to_vec(&canonical_type_env_value(&first).unwrap()).unwrap();
        let second = serde_json::to_vec(&canonical_type_env_value(&second).unwrap()).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn resolved_cache_eviction_is_bounded_and_keeps_records_paired() {
        fn initialized_slot(entry: ResolvedModuleEntry, last_used: u64) -> Arc<ResolvedModuleSlot> {
            let slot = initialized_resolved_slot(entry);
            slot.last_used.store(last_used, Ordering::Relaxed);
            slot
        }

        let initializing = Arc::new(ResolvedModuleSlot::new());
        let mut cache = HashMap::from([
            (
                "old".to_string(),
                initialized_slot(
                    ResolvedModuleEntry::Source {
                        source: "old source".to_string(),
                        imports: vec!["cmp".to_string()],
                    },
                    1,
                ),
            ),
            (
                "new".to_string(),
                initialized_slot(
                    ResolvedModuleEntry::Source {
                        source: "new source".to_string(),
                        imports: vec!["io".to_string()],
                    },
                    2,
                ),
            ),
            ("initializing".to_string(), initializing),
        ]);

        assert_eq!(trim_resolved_module_cache_map(&mut cache, 2, usize::MAX), 1);
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains_key("old"));
        assert!(cache.contains_key("initializing"));
        assert!(
            cache
                .get("initializing")
                .is_some_and(|slot| slot.entry.get().is_none())
        );
        let entry = cache.get("new").and_then(|slot| slot.entry.get());
        assert!(entry.is_some());
        if let Some(entry) = entry {
            let source = match entry {
                ResolvedModuleEntry::Source { source, .. } => Some(source.as_str()),
                _ => None,
            };
            assert_eq!(source, Some("new source"));
            assert_eq!(entry.imports(), Some(["io".to_string()].as_slice()));
        }

        let mut oversized = HashMap::from([(
            "oversized".to_string(),
            initialized_slot(
                ResolvedModuleEntry::Source {
                    source: "generated source larger than the test budget".to_string(),
                    imports: Vec::new(),
                },
                1,
            ),
        )]);
        assert_eq!(trim_resolved_module_cache_map(&mut oversized, 1, 8), 1);
        assert!(oversized.is_empty());
    }

    #[test]
    fn resolved_cache_exports_and_reimports_generated_modules() {
        let roots = HashSet::from(["Compare".to_string()]);
        assert!(resolve_with_roots("cmp", &roots).is_some());
        assert!(scan_type_env("cmp").is_some());
        assert!(has_initialized_type_env("cmp"));

        let bytes = export_resolved_module_cache().unwrap();
        let archive: ResolvedCacheArchive = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(archive.schema, RESOLVED_CACHE_SCHEMA);
        assert_eq!(
            archive.resolver_fingerprint,
            crate::RESOLVER_CACHE_FINGERPRINT
        );
        let encoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            encoded
                .get("compilerFingerprint")
                .and_then(serde_json::Value::as_str),
            Some(crate::RESOLVER_CACHE_FINGERPRINT)
        );
        assert!(encoded.get("resolverFingerprint").is_none());
        assert!(archive.entries.iter().any(|entry| {
            entry.import_path == "cmp"
                && entry
                    .roots
                    .as_ref()
                    .is_some_and(|roots| roots == &["Compare".to_string()])
                && entry.source.is_some()
        }));
        assert!(
            archive
                .type_envs
                .iter()
                .any(|entry| entry.import_path == "cmp")
        );

        let stats = import_resolved_module_cache(&bytes).unwrap();
        assert!(stats.already_present >= 1);
        assert!(stats.type_envs_already_present >= 1);
    }

    #[cfg(all(feature = "parallel", not(target_family = "wasm")))]
    #[test]
    fn parallel_resolved_file_compilation_matches_sequential_output() {
        fn compile_fixture(jobs: usize) -> (String, ResolvedFileCompileMode) {
            let parsed_files = vec![
                (
                    "views.go",
                    crate::parser::parse_file(
                        "views.go",
                        r#"
package views

const blockSize = 8

type block [blockSize]byte
type header [blockSize]byte

func (b *block) header() *header {
	return (*header)(b)
}

func (h *header) name() []byte {
	return h[1:][:3]
}
"#,
                    )
                    .unwrap(),
                ),
                (
                    "write.go",
                    crate::parser::parse_file(
                        "write.go",
                        r#"
package views

func Fill(b *block) {
	field := b.header().name()
	field[0] = 'A'
}
"#,
                    )
                    .unwrap(),
                ),
            ];
            let parsed_file_refs = parsed_files
                .iter()
                .map(|(_, file)| file)
                .collect::<Vec<_>>();
            let mut package_type_env = TypeEnv::new();
            package_type_env.scan_files(&parsed_file_refs);
            let imported_type_envs = BTreeMap::new();
            let import_renames = BTreeMap::new();
            let package_mutable_top_level_vars =
                crate::compiler::mutable_top_level_var_names_for_files_with_type_env(
                    parsed_file_refs.iter().copied(),
                    false,
                    &package_type_env,
                );
            let view_method_seed = crate::compiler::borrowed_view_method_seed_for_files(
                &parsed_file_refs,
                &package_type_env,
            );
            let context = ResolvedRecoveryContext {
                import_path: "views",
                package_type_env: &package_type_env,
                imported_type_envs: &imported_type_envs,
                import_renames: &import_renames,
                package_mutable_top_level_vars: &package_mutable_top_level_vars,
                view_method_seed: &view_method_seed,
            };

            let (compiled, mode) = compile_resolved_files(parsed_files, &context, jobs);
            let mut items = Vec::new();
            for compiled_file in compiled {
                items.extend(compiled_file.result.unwrap().items);
            }
            (
                prettyplease::unparse(&syn::File {
                    shebang: None,
                    attrs: vec![],
                    items,
                }),
                mode,
            )
        }

        let (sequential, sequential_mode) = compile_fixture(1);
        let (parallel, parallel_mode) = compile_fixture(4);
        let outer_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let (nested, nested_mode) = outer_pool.install(|| compile_fixture(4));

        assert_eq!(sequential_mode, ResolvedFileCompileMode::Sequential);
        assert_eq!(parallel_mode, ResolvedFileCompileMode::Parallel);
        assert_eq!(nested_mode, ResolvedFileCompileMode::Sequential);
        assert_eq!(parallel, sequential);
        assert_eq!(nested, sequential);
        assert!(parallel.contains(".to_vec();"), "{parallel}");
        assert!(parallel.contains("__gors_slice_alias_value"), "{parallel}");
    }

    #[test]
    fn refresh_top_level_vars_with_imports_promotes_imported_receiver_facts() {
        let common = crate::parser::parse_file(
            "common.go",
            r#"
package tar

import "internal/godebug"

var tarinsecurepath = godebug.New("tarinsecurepath")
"#,
        )
        .unwrap();
        let reader = crate::parser::parse_file(
            "reader.go",
            r#"
package tar

func read() string {
	return tarinsecurepath.Value()
}
"#,
        )
        .unwrap();
        let mut godebug_env = TypeEnv::new();
        godebug_env.set_type_kind("Setting", TypeKind::Struct);
        godebug_env.set_func(
            "New",
            vec![GoType::Pointer(Box::new(GoType::Named(
                "Setting".to_string(),
            )))],
        );
        godebug_env.set_func("Setting.Value", vec![GoType::String]);
        godebug_env.set_pointer_receiver_method("Setting.Value");
        let imported_type_envs = BTreeMap::from([(
            "internal/godebug".to_string(),
            crate::compiler::PackageFacts::new("godebug".to_string(), godebug_env),
        )]);
        let mut package_env = TypeEnv::new();
        let files = [&common, &reader];
        package_env.scan_files(&files);

        refresh_top_level_vars_with_imports(&mut package_env, &files, &imported_type_envs);

        assert_eq!(
            package_env.get_top_level_var("tarinsecurepath"),
            Some(GoType::Pointer(Box::new(GoType::Named(
                "godebug.Setting".to_string()
            ))))
        );
        assert!(package_env.has_func("godebug.Setting.Value"));
        assert!(package_env.method_has_pointer_receiver("godebug.Setting.Value"));
        assert!(!package_env.has_func("godebug.New"));
    }

    #[test]
    fn reachable_names_include_typed_local_and_named_slice_method_calls() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package pkg

type parser struct{}

func (*parser) parseString() string {
	return ""
}

type sparseArray []byte

func (s sparseArray) entry() int {
	return 0
}

func root(s sparseArray) {
	var p parser
	_ = p.parseString()
	_ = s.entry()
}
"#,
        )
        .unwrap();
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["root".to_string()]);

        let reachable = reachable_package_names(&parsed, &roots);

        assert!(reachable.contains("parser::parseString"), "{reachable:?}");
        assert!(reachable.contains("sparseArray::entry"), "{reachable:?}");
    }

    #[test]
    fn reachable_names_include_pointer_methods_needed_for_imported_interface_args() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package godebug

import "internal/bisect"

type runtimeStderr struct{}

var stderr runtimeStderr

func (*runtimeStderr) Write(b []byte) (int, error) {
	return len(b), nil
}

func use(m *bisect.Matcher) {
	m.Stack(&stderr)
}
"#,
        )
        .unwrap();

        let mut bisect_env = TypeEnv::new();
        bisect_env.set_type_kind("Writer", TypeKind::Interface);
        bisect_env.set_interface_methods("Writer", vec!["Write".to_string()]);
        bisect_env.set_type_kind("Matcher", TypeKind::Struct);
        bisect_env.set_func_params("Matcher.Stack", vec![GoType::Named("Writer".to_string())]);
        bisect_env.set_func("Matcher.Stack", vec![GoType::Bool]);
        bisect_env.set_pointer_receiver_method("Matcher.Stack");
        let imported_type_envs = BTreeMap::from([(
            "internal/bisect".to_string(),
            crate::compiler::PackageFacts::new("bisect".to_string(), bisect_env),
        )]);
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["use".to_string()]);

        let reachable = reachable_package_names_with_imports(&parsed, &roots, &imported_type_envs);

        assert!(reachable.contains("stderr"), "{reachable:?}");
        assert!(reachable.contains("runtimeStderr"), "{reachable:?}");
        assert!(reachable.contains("runtimeStderr::Write"), "{reachable:?}");
    }

    #[test]
    fn reachable_names_include_methods_needed_for_concrete_error_values() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package pkg

type Error int

const ErrSyntax Error = 1

func (e Error) Error() string {
	return "syntax"
}

type holder struct {
	err error
}

func root() error {
	var err error
	err = ErrSyntax
	_ = holder{err: ErrSyntax}
	return ErrSyntax
}
"#,
        )
        .unwrap();
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["root".to_string()]);

        let reachable = reachable_package_names(&parsed, &roots);

        assert!(reachable.contains("Error"), "{reachable:?}");
        assert!(reachable.contains("Error::Error"), "{reachable:?}");
        assert!(reachable.contains("ErrSyntax"), "{reachable:?}");
        assert!(reachable.contains("holder"), "{reachable:?}");
    }

    #[test]
    fn reachable_names_include_methods_called_on_method_return_locals() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package pkg

type block []byte

func (b *block) toGNU() *headerGNU {
	return nil
}

type headerGNU []byte

func (h *headerGNU) sparse() sparseArray {
	return nil
}

type sparseArray []byte

func (s sparseArray) maxEntries() int {
	return 0
}

func root(blk *block) {
	s := blk.toGNU().sparse()
	_ = s.maxEntries()
}
"#,
        )
        .unwrap();
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["root".to_string()]);

        let reachable = reachable_package_names(&parsed, &roots);

        assert!(reachable.contains("block::toGNU"), "{reachable:?}");
        assert!(reachable.contains("headerGNU::sparse"), "{reachable:?}");
        assert!(
            reachable.contains("sparseArray::maxEntries"),
            "{reachable:?}"
        );
    }

    #[test]
    fn dedupe_use_items_matches_use_trees_structurally() {
        let duplicate_group: syn::ItemUse =
            syn::parse_quote! { use crate::io::{Read, Write as W}; };
        let duplicate_private: syn::ItemUse = syn::parse_quote! { use crate::io::Read; };
        let distinct_public: syn::ItemUse = syn::parse_quote! { pub use crate::io::Read; };
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! { use crate::io::{Read, Write as W}; },
            syn::parse_quote! { use crate::io::{Read, Write as W}; },
            syn::parse_quote! { use crate::io::Read; },
            syn::parse_quote! { fn keep() {} },
            syn::parse_quote! { use crate::io::Read; },
            syn::parse_quote! { pub use crate::io::Read; },
        ];

        dedupe_use_items(&mut items);

        assert_eq!(items.len(), 4);
        assert!(
            items
                .get(2)
                .is_some_and(|item| matches!(item, syn::Item::Fn(_)))
        );
        assert_eq!(matching_use_item_count(&items, &duplicate_group), 1);
        assert_eq!(matching_use_item_count(&items, &duplicate_private), 1);
        assert_eq!(matching_use_item_count(&items, &distinct_public), 1);
    }

    fn matching_use_item_count(items: &[syn::Item], expected: &syn::ItemUse) -> usize {
        items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Use(item_use) => Some(item_use),
                _ => None,
            })
            .filter(|item_use| use_items_match(item_use, expected))
            .count()
    }

    #[test]
    fn scanned_type_env_preserves_named_constants_fields_and_method_results()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
package fixture

type Errno uintptr
const ENOENT Errno = 2

type Timespec struct{}
type Stat_t struct {
	Atimespec Timespec
}

func (Timespec) Unix() (int64, int64) {
	return 0, 0
}
"#,
        )?;
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert_eq!(
            env.get_var("ENOENT"),
            Some(crate::compiler::typeinfer::GoType::Named(
                "Errno".to_string()
            ))
        );
        assert_eq!(
            env.get_field_type("Stat_t", "Atimespec"),
            crate::compiler::typeinfer::GoType::Named("Timespec".to_string())
        );
        assert_eq!(
            env.get_func_returns("Timespec.Unix"),
            vec![
                crate::compiler::typeinfer::GoType::Int64,
                crate::compiler::typeinfer::GoType::Int64,
            ]
        );
        Ok(())
    }

    #[test]
    fn resolve_internal_godebug_value_keeps_runtime_stderr_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["Setting".to_string(), "Setting::Value".to_string()]);
        let module = resolve_with_roots("internal/godebug", &roots)
            .ok_or_else(|| std::io::Error::other("resolve internal/godebug"))?;
        let items = module
            .content
            .as_ref()
            .map(|(_, items)| items.as_slice())
            .unwrap_or_default();
        let output = quote::quote! { #(#items)* }.to_string();

        assert!(output.contains("fn Value"), "{output}");
        assert!(output.contains("struct runtimeStderr"), "{output}");
        assert!(output.contains("fn Write"), "{output}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_instantiated_field_type_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
                package fixture

                import "sync/atomic"

                type Matcher struct {
                    dedup atomic.Pointer[dedup]
                }

                type dedup struct {
                    recent [128][4]uint64
                }

                func (d *dedup) seenLossy(h uint64) bool {
                    cache := &d.recent[uint(h)%uint(len(d.recent))]
                    for i := 0; i < len(cache); i++ {
                    }
                    return false
                }
            "#,
        )?;
        let parsed_files = vec![("fixture.go", file)];
        let roots = HashSet::from(["Matcher".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("Matcher"));
        assert!(reachable.contains("dedup"));
        Ok(())
    }

    #[test]
    fn reachable_names_include_package_vars_reached_from_transitive_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let entry_file = crate::parser::parse_file(
            "entry.go",
            r#"
                package fixture

                func Entry() bool {
                    return helper()
                }

                func helper() bool {
                    return !flag
                }
            "#,
        )?;
        let var_file = crate::parser::parse_file(
            "vars.go",
            r#"
                package fixture

                var flag = true
            "#,
        )?;
        let parsed_files = vec![("entry.go", entry_file), ("vars.go", var_file)];
        let roots = HashSet::from(["Entry".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("Entry"), "{reachable:?}");
        assert!(reachable.contains("helper"), "{reachable:?}");
        assert!(reachable.contains("flag"), "{reachable:?}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_type_switch_case_interface_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
                package fixture

                type I interface {
                    A() int
                    B() int
                }

                type T struct{}

                func (T) A() int { return 1 }
                func (T) B() int { return 2 }

                func Use(i I) int {
                    switch i.(type) {
                    case T:
                        return 1
                    default:
                        return 0
                    }
                }
            "#,
        )?;
        let parsed_files = vec![("fixture.go", file)];
        let roots = HashSet::from(["Use".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in ["I", "T", "T::A", "T::B"] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn reachable_names_include_private_helpers_called_by_receiver_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package fixture

type MapIter struct{}

func (iter *MapIter) Key() int {
	return copyVal(1)
}

func copyVal(value int) int {
	return value
}

func deadHelper() int {
	return 0
}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let parsed_files = vec![("fixture.go", file)];
        let roots = HashSet::from(["MapIter".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("MapIter"), "{reachable:?}");
        assert!(reachable.contains("MapIter::Key"), "{reachable:?}");
        assert!(reachable.contains("copyVal"), "{reachable:?}");
        assert!(!reachable.contains("deadHelper"), "{reachable:?}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_context_interface_receiver_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let files = package_files("context").ok_or_else(|| std::io::Error::other("files"))?;
        let mut parsed_files = Vec::new();
        for (filename, content) in files.iter() {
            parsed_files.push((*filename, crate::parser::parse_file(filename, content)?));
        }
        let roots = HashSet::from([
            "Background".to_string(),
            "TODO".to_string(),
            "Context".to_string(),
            "Context::Deadline".to_string(),
            "Context::Done".to_string(),
            "Context::Err".to_string(),
            "Context::Value".to_string(),
            "WithValue".to_string(),
        ]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in [
            "backgroundCtx",
            "todoCtx",
            "emptyCtx",
            "valueCtx",
            "cancelCtx",
            "timerCtx",
            "withoutCancelCtx",
            "withoutCancelCtx::Deadline",
            "withoutCancelCtx::Done",
            "withoutCancelCtx::Err",
            "withoutCancelCtx::Value",
        ] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }
        let module = resolve_with_roots("context", &roots)
            .ok_or_else(|| std::io::Error::other("resolve context"))?;
        let tokens = module.to_token_stream().to_string();
        for expected in [
            "impl emptyCtx",
            "pub fn Deadline",
            "pub fn Done",
            "pub fn Err",
            "pub fn Value",
            "impl Context for backgroundCtx",
            "impl Context for todoCtx",
            "impl < '__gors : 'static > Context for crate :: builtin :: GorsPtr < valueCtx < '__gors > >",
            "fn value",
        ] {
            assert!(
                tokens.contains(expected),
                "{expected} missing from {tokens}"
            );
        }
        assert!(!tokens.contains("impl stringer for timerCtx"), "{tokens}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_package_init_dependencies() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = r#"
package fixture

var closedchan = make(chan struct{})

func init() {
	close(closedchan)
}

func WithCancel() {}

func deadHelper() {}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let reachable_file = crate::parser::parse_file("fixture.go", source)?;
        let parsed_files = vec![("fixture.go", reachable_file)];
        let roots = HashSet::from(["WithCancel".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in ["init", "closedchan"] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }

        assert!(!reachable.contains("deadHelper"), "{reachable:?}");
        let filtered = filter_file_to_reachable(file, &reachable);
        let compiled = crate::compiler::compile(filtered)?;
        let tokens = compiled.to_token_stream().to_string();

        for expected in ["pub fn __gors_init", "closedchan", "close"] {
            assert!(
                tokens.contains(expected),
                "{expected} missing from {tokens}"
            );
        }
        Ok(())
    }

    #[test]
    fn resolved_files_merge_multiple_package_init_functions() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                pub fn __gors_init() {
                    runtime_args();
                }
            },
            syn::parse_quote! {
                pub fn helper() {}
            },
            syn::parse_quote! {
                pub fn __gors_init() {
                    runtime_envs();
                }
            },
        ];

        crate::compiler::merge_package_init_items(&mut items);
        let tokens = quote::quote! { #(#items)* }.to_string();

        assert_eq!(tokens.matches("pub fn __gors_init").count(), 1, "{tokens}");
        assert!(tokens.contains("runtime_args"), "{tokens}");
        assert!(tokens.contains("runtime_envs"), "{tokens}");
        assert!(tokens.contains("pub fn helper"), "{tokens}");
    }

    #[test]
    fn runtime_gomaxprocs_resolves_as_runtime_primitive() -> Result<(), Box<dyn std::error::Error>>
    {
        let roots = HashSet::from(["GOMAXPROCS".to_string()]);
        let module = resolve_with_roots("runtime", &roots)
            .ok_or_else(|| std::io::Error::other("resolve runtime"))?;
        let tokens = module.to_token_stream().to_string();

        assert!(tokens.contains("pub fn GOMAXPROCS"));
        assert!(!tokens.contains("sched"));
        assert!(collect_resolved_imports("runtime", &roots).is_empty());
        Ok(())
    }

    #[test]
    fn filtered_package_retains_vars_reached_through_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package fixture

var optimize = true

func AppendFloat() bool {
	return genericFtoa()
}

func genericFtoa() bool {
	return optimize
}

func deadHelper() bool {
	return false
}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let reachable_file = crate::parser::parse_file("fixture.go", source)?;
        let parsed_files = vec![("fixture.go", reachable_file)];
        let roots = HashSet::from(["AppendFloat".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);
        let filtered = filter_file_to_reachable(file, &reachable);
        let compiled = crate::compiler::compile(filtered)?;
        let tokens = compiled.to_token_stream().to_string();

        assert!(tokens.contains("pub fn AppendFloat"), "{tokens}");
        assert!(tokens.contains("fn genericFtoa"), "{tokens}");
        assert!(tokens.contains("static optimize_"), "{tokens}");
        assert!(!tokens.contains("deadHelper"), "{tokens}");
        Ok(())
    }

    #[test]
    fn filtered_package_retains_private_helpers_constants_and_vars()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package fixture

const (
	ENOENT   = 2
	O_RDONLY = 0
	deadCode = 99
)

var errors = []string{"ok"}

func Open(path string, mode int) (int, int) {
	return open(path, mode), ENOENT
}

func open(path string, mode int) int {
	_ = errors
	return len(path) + mode
}

func Read(fd int) int {
	return read(fd)
}

func read(fd int) int {
	return fd
}

func deadHelper() {}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let reachable_file = crate::parser::parse_file("fixture.go", source)?;
        let parsed_files = vec![("fixture.go", reachable_file)];
        let roots = HashSet::from([
            "ENOENT".to_string(),
            "Open".to_string(),
            "O_RDONLY".to_string(),
            "Read".to_string(),
        ]);
        let reachable = reachable_package_names(&parsed_files, &roots);
        let filtered = filter_file_to_reachable(file, &reachable);
        let compiled = crate::compiler::compile(filtered)?;
        let tokens = compiled.to_token_stream().to_string();

        assert!(tokens.contains("pub fn Open"), "{tokens}");
        assert!(tokens.contains("pub fn Read"), "{tokens}");
        assert!(tokens.contains("fn read"), "{tokens}");
        assert!(tokens.contains("fn open"), "{tokens}");
        assert!(tokens.contains("pub const ENOENT"), "{tokens}");
        assert!(tokens.contains("pub const O_RDONLY"), "{tokens}");
        assert!(tokens.contains("static errors"), "{tokens}");
        assert!(!tokens.contains("deadHelper"), "{tokens}");
        Ok(())
    }

    #[test]
    fn generated_interface_adapter_borrows_resliced_reader_buffers()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package fixture

type Reader interface {
	Read([]byte) (int, error)
}

type regFileReader struct {
	r  Reader
	nb int
}

func (fr *regFileReader) Read(b []byte) (n int, err error) {
	if len(b) > fr.nb {
		b = b[:fr.nb]
	}
	if len(b) > 0 {
		n, err = fr.r.Read(b)
	}
	return n, err
}

func use(reader Reader, b []byte) {
	reader.Read(b)
}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let compiled = crate::compiler::compile(file)?;
        let compact = compiled
            .to_token_stream()
            .to_string()
            .split_whitespace()
            .collect::<String>();

        assert!(
            compact.contains("pubfnRead(mutfr:crate::builtin::GorsPtr<Self>,mutb:&mut[u8]"),
            "{compact}"
        );
        assert!(
            compact.contains("regFileReader::Read(self.clone(),&mut*b)"),
            "{compact}"
        );
        assert!(
            !compact.contains("regFileReader::Read(self.clone(),(b).to_vec())"),
            "{compact}"
        );
        Ok(())
    }

    #[test]
    fn rooted_interface_return_keeps_composite_value_method_set()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
package fixture

type Info interface {
	Name() string
}

type Entry interface {
	Name() string
	Info() Info
}

type entryInfo struct {
	info Info
}

func (entryInfo) Name() string { return "entry" }
func (e entryInfo) Info() Info { return e.info }

func Wrap(info Info) Entry {
	return entryInfo{info: info}
}
"#;
        let file = crate::parser::parse_file("fixture.go", source)?;
        let reachable_file = crate::parser::parse_file("fixture.go", source)?;
        let parsed_files = vec![("fixture.go", reachable_file)];
        let roots = HashSet::from(["Wrap".to_string()]);

        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in ["entryInfo", "entryInfo::Name", "entryInfo::Info"] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}",
            );
        }

        let filtered = filter_file_to_reachable(file, &reachable);
        let compiled = crate::compiler::compile(filtered)?;
        let tokens = compiled.to_token_stream().to_string();
        assert!(tokens.contains("impl Entry for entryInfo"), "{tokens}");
        Ok(())
    }

    #[test]
    fn rooted_sdk_interface_conversion_keeps_private_concrete_impl()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["FileInfoToDirEntry".to_string()]);
        let module = resolve_with_roots("io/fs", &roots)
            .ok_or_else(|| std::io::Error::other("resolve io/fs"))?;
        let tokens = module.to_token_stream().to_string();

        assert!(tokens.contains("impl DirEntry for dirInfo"), "{tokens}");
        Ok(())
    }
}
