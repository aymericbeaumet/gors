use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use crate::compiler::input::GoLanguageVersion;
use crate::import_path::CanonicalImportPath;

use super::{
    LocalModuleError, MaterializedPackage, MaterializedSourceFile, ModuleFileIssue,
    go_mod::parse_module_metadata,
};

type PackageCell = Arc<PackageSingleFlight>;

/// Lazy immutable package snapshots rooted in one canonical local Go module.
///
/// Construction reads only `go.mod`. Each requested package is selected and
/// read at most once after a per-import single-flight admission. No Go source
/// is scanned or parsed and imports are never followed recursively.
#[derive(Debug)]
pub struct LocalModuleCatalog {
    canonical_root: PathBuf,
    module_path: CanonicalImportPath,
    language_version: GoLanguageVersion,
    packages: Mutex<BTreeMap<CanonicalImportPath, PackageCell>>,
}

impl LocalModuleCatalog {
    /// Open one module root and own its canonical root and module identity.
    pub fn open(module_root: impl AsRef<Path>) -> Result<Self, LocalModuleError> {
        let supplied_root = module_root.as_ref();
        let canonical_root = std::fs::canonicalize(supplied_root).map_err(|error| {
            LocalModuleError::io("canonicalize module root", supplied_root, error)
        })?;
        ensure_utf8(&canonical_root)?;
        let metadata = std::fs::metadata(&canonical_root)
            .map_err(|error| LocalModuleError::io("inspect module root", &canonical_root, error))?;
        if !metadata.is_dir() {
            return Err(LocalModuleError::InvalidModuleRoot {
                path: canonical_root,
            });
        }

        let lexical_go_mod = canonical_root.join("go.mod");
        let canonical_go_mod = std::fs::canonicalize(&lexical_go_mod)
            .map_err(|error| LocalModuleError::io("canonicalize go.mod", &lexical_go_mod, error))?;
        ensure_canonical_file_parent(None, &canonical_root, &lexical_go_mod, &canonical_go_mod)?;
        ensure_utf8(&canonical_go_mod)?;
        let bytes = std::fs::read(&canonical_go_mod)
            .map_err(|error| LocalModuleError::io("read go.mod", &canonical_go_mod, error))?;
        let source =
            std::str::from_utf8(&bytes).map_err(|_| LocalModuleError::MalformedModule {
                path: canonical_go_mod.clone(),
                issue: ModuleFileIssue::NonUtf8,
            })?;
        let metadata =
            parse_module_metadata(source).map_err(|issue| LocalModuleError::MalformedModule {
                path: canonical_go_mod,
                issue,
            })?;

        Ok(Self {
            canonical_root,
            module_path: metadata.module_path,
            language_version: metadata.language_version,
            packages: Mutex::new(BTreeMap::new()),
        })
    }

    /// Canonical filesystem root owned by this catalog.
    #[must_use]
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    /// Canonical import identity declared by `go.mod`.
    #[must_use]
    pub const fn module_path(&self) -> &CanonicalImportPath {
        &self.module_path
    }

    /// Go language version declared by this module, or Go's fixed `go1.16`
    /// compatibility default when the directive is absent.
    #[must_use]
    pub const fn language_version(&self) -> GoLanguageVersion {
        self.language_version
    }

    /// Number of successfully materialized package snapshots.
    #[must_use]
    pub fn materialized_package_count(&self) -> usize {
        let packages = lock_packages(&self.packages);
        packages
            .values()
            .filter(|package| package.is_ready())
            .count()
    }

    fn package_cell(&self, import_path: &CanonicalImportPath) -> PackageCell {
        let mut packages = lock_packages(&self.packages);
        Arc::clone(
            packages
                .entry(import_path.clone())
                .or_insert_with(|| Arc::new(PackageSingleFlight::new())),
        )
    }

    fn load_package(
        &self,
        import_path: &CanonicalImportPath,
    ) -> Result<Arc<MaterializedPackage>, LocalModuleError> {
        let relative = self.local_relative_path(import_path)?;
        let lexical_directory = relative
            .iter()
            .fold(self.canonical_root.clone(), |directory, component| {
                directory.join(component)
            });
        ensure_lexical_containment(import_path, &self.canonical_root, &lexical_directory)?;
        let canonical_directory = match std::fs::canonicalize(&lexical_directory) {
            Ok(directory) => directory,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(LocalModuleError::MissingPackage {
                    requested: import_path.clone(),
                    directory: lexical_directory,
                });
            }
            Err(error) => {
                return Err(LocalModuleError::io(
                    "canonicalize local package",
                    lexical_directory,
                    error,
                ));
            }
        };
        ensure_canonical_containment(
            Some(import_path),
            &self.canonical_root,
            &lexical_directory,
            &canonical_directory,
        )?;
        ensure_utf8(&canonical_directory)?;
        let metadata = std::fs::metadata(&canonical_directory).map_err(|error| {
            LocalModuleError::io("inspect local package", &canonical_directory, error)
        })?;
        if !metadata.is_dir() {
            return Err(LocalModuleError::MissingPackage {
                requested: import_path.clone(),
                directory: canonical_directory,
            });
        }

        let files = select_source_files(import_path, &self.canonical_root, &canonical_directory)?;
        if files.is_empty() {
            return Err(LocalModuleError::PackageHasNoGoFiles {
                requested: import_path.clone(),
                directory: canonical_directory,
            });
        }
        let files = read_source_files(files)?;
        Ok(Arc::new(MaterializedPackage::new(
            import_path.clone(),
            canonical_directory,
            files.into(),
        )))
    }

    fn local_relative_path<'path>(
        &self,
        import_path: &'path CanonicalImportPath,
    ) -> Result<Vec<&'path str>, LocalModuleError> {
        if import_path == &self.module_path {
            return Ok(Vec::new());
        }
        let Some(suffix) = import_path
            .as_str()
            .strip_prefix(self.module_path.as_str())
            .and_then(|suffix| suffix.strip_prefix('/'))
        else {
            return Err(LocalModuleError::ExternalImport {
                requested: import_path.clone(),
                module: self.module_path.clone(),
            });
        };
        Ok(suffix.split('/').collect())
    }

    fn require_local_import(
        &self,
        import_path: &CanonicalImportPath,
    ) -> Result<(), LocalModuleError> {
        let is_local = import_path == &self.module_path
            || import_path
                .as_str()
                .strip_prefix(self.module_path.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'));
        if is_local {
            return Ok(());
        }
        Err(LocalModuleError::ExternalImport {
            requested: import_path.clone(),
            module: self.module_path.clone(),
        })
    }
}

impl LocalModuleCatalog {
    /// Materialize exactly one local package without traversing its imports.
    pub fn materialize(
        &self,
        import_path: &CanonicalImportPath,
    ) -> Result<Arc<MaterializedPackage>, LocalModuleError> {
        self.require_local_import(import_path)?;
        let cell = self.package_cell(import_path);
        cell.get_or_load(|| self.load_package(import_path))
    }
}

struct SelectedSourceFile {
    logical_path: Arc<str>,
    canonical_path: PathBuf,
}

#[derive(Debug)]
enum PackageLoadState {
    Empty,
    Loading,
    Ready(Arc<MaterializedPackage>),
}

#[derive(Debug)]
struct PackageSingleFlight {
    state: Mutex<PackageLoadState>,
    ready: Condvar,
}

impl PackageSingleFlight {
    fn new() -> Self {
        Self {
            state: Mutex::new(PackageLoadState::Empty),
            ready: Condvar::new(),
        }
    }

    fn is_ready(&self) -> bool {
        matches!(*lock_load_state(&self.state), PackageLoadState::Ready(_))
    }

    fn get_or_load(
        &self,
        load: impl FnOnce() -> Result<Arc<MaterializedPackage>, LocalModuleError>,
    ) -> Result<Arc<MaterializedPackage>, LocalModuleError> {
        let mut state = lock_load_state(&self.state);
        loop {
            match &*state {
                PackageLoadState::Ready(package) => return Ok(Arc::clone(package)),
                PackageLoadState::Loading => {
                    state = wait_for_load(&self.ready, state);
                }
                PackageLoadState::Empty => {
                    *state = PackageLoadState::Loading;
                    break;
                }
            }
        }
        drop(state);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(load));
        let mut completed = lock_load_state(&self.state);
        *completed = match &result {
            Ok(Ok(package)) => PackageLoadState::Ready(Arc::clone(package)),
            Ok(Err(_)) | Err(_) => PackageLoadState::Empty,
        };
        self.ready.notify_all();
        drop(completed);
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

fn select_source_files(
    import_path: &CanonicalImportPath,
    module_root: &Path,
    package_directory: &Path,
) -> Result<Vec<SelectedSourceFile>, LocalModuleError> {
    let entries = std::fs::read_dir(package_directory).map_err(|error| {
        LocalModuleError::io("read local package directory", package_directory, error)
    })?;
    let mut selected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            LocalModuleError::io("read local package directory", package_directory, error)
        })?;
        let lexical_path = entry.path();
        let Some(logical_path) = eligible_filename(&lexical_path)? else {
            continue;
        };
        let canonical_path = std::fs::canonicalize(&lexical_path).map_err(|error| {
            LocalModuleError::io("canonicalize local source file", &lexical_path, error)
        })?;
        ensure_canonical_file_parent(
            Some(import_path),
            module_root,
            &lexical_path,
            &canonical_path,
        )?;
        ensure_utf8(&canonical_path)?;
        let metadata = std::fs::metadata(&canonical_path).map_err(|error| {
            LocalModuleError::io("inspect local source file", &canonical_path, error)
        })?;
        if metadata.is_file() {
            selected.push(SelectedSourceFile {
                logical_path,
                canonical_path,
            });
        }
    }
    selected.sort_by(|left, right| {
        left.logical_path
            .cmp(&right.logical_path)
            .then_with(|| left.canonical_path.cmp(&right.canonical_path))
    });
    Ok(selected)
}

fn read_source_files(
    files: Vec<SelectedSourceFile>,
) -> Result<Vec<MaterializedSourceFile>, LocalModuleError> {
    files
        .into_iter()
        .map(|file| {
            let bytes = std::fs::read(&file.canonical_path).map_err(|error| {
                LocalModuleError::io("read local source file", &file.canonical_path, error)
            })?;
            let source = String::from_utf8(bytes).map_err(|_| LocalModuleError::NonUtf8Source {
                path: file.canonical_path.clone(),
            })?;
            Ok(MaterializedSourceFile::new(
                file.logical_path,
                file.canonical_path,
                Arc::from(source),
            ))
        })
        .collect()
}

fn eligible_filename(path: &Path) -> Result<Option<Arc<str>>, LocalModuleError> {
    if path.extension() != Some(OsStr::new("go")) {
        return Ok(None);
    }
    let filename =
        path.file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| LocalModuleError::NonUtf8Path {
                path: path.to_path_buf(),
            })?;
    let eligible =
        !filename.starts_with('.') && !filename.starts_with('_') && !filename.ends_with("_test.go");
    Ok(eligible.then(|| Arc::from(filename)))
}

fn ensure_utf8(path: &Path) -> Result<(), LocalModuleError> {
    path.to_str()
        .map(|_| ())
        .ok_or_else(|| LocalModuleError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

fn ensure_lexical_containment(
    import_path: &CanonicalImportPath,
    module_root: &Path,
    lexical_path: &Path,
) -> Result<(), LocalModuleError> {
    if lexical_path.starts_with(module_root) {
        return Ok(());
    }
    Err(LocalModuleError::ContainmentEscape {
        requested: Some(import_path.clone()),
        module_root: module_root.to_path_buf(),
        lexical_path: lexical_path.to_path_buf(),
        canonical_path: None,
    })
}

fn ensure_canonical_containment(
    requested: Option<&CanonicalImportPath>,
    module_root: &Path,
    lexical_path: &Path,
    canonical_path: &Path,
) -> Result<(), LocalModuleError> {
    if canonical_path.starts_with(module_root) {
        return Ok(());
    }
    Err(LocalModuleError::ContainmentEscape {
        requested: requested.cloned(),
        module_root: module_root.to_path_buf(),
        lexical_path: lexical_path.to_path_buf(),
        canonical_path: Some(canonical_path.to_path_buf()),
    })
}

fn ensure_canonical_file_parent(
    requested: Option<&CanonicalImportPath>,
    module_root: &Path,
    lexical_path: &Path,
    canonical_path: &Path,
) -> Result<(), LocalModuleError> {
    ensure_canonical_containment(requested, module_root, lexical_path, canonical_path)?;
    let expected_parent = lexical_path.parent();
    let canonical_parent = canonical_path.parent();
    if expected_parent == canonical_parent {
        return Ok(());
    }
    Err(LocalModuleError::ContainmentEscape {
        requested: requested.cloned(),
        module_root: module_root.to_path_buf(),
        lexical_path: lexical_path.to_path_buf(),
        canonical_path: Some(canonical_path.to_path_buf()),
    })
}

fn lock_packages(
    packages: &Mutex<BTreeMap<CanonicalImportPath, PackageCell>>,
) -> std::sync::MutexGuard<'_, BTreeMap<CanonicalImportPath, PackageCell>> {
    match packages.lock() {
        Ok(packages) => packages,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_load_state(state: &Mutex<PackageLoadState>) -> std::sync::MutexGuard<'_, PackageLoadState> {
    match state.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_for_load<'state>(
    ready: &Condvar,
    state: std::sync::MutexGuard<'state, PackageLoadState>,
) -> std::sync::MutexGuard<'state, PackageLoadState> {
    match ready.wait(state) {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}
