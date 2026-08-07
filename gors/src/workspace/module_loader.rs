//! Module-aware package loading for production compiler entrypoints.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::compiler::input::{
    EmbeddedGoSdkPackageManifestCatalog, GoLanguageVersion, LayeredPackageManifestCatalog,
    PackageInputManifest, PackageKey, PackageManifestCatalog, ProgramInput, WorkspaceKey,
};
use crate::import_path::CanonicalImportPath;

use super::local_module::{LocalModuleCatalog, LocalModuleManifestCatalog};
use super::{LoadError, LoadedProgram, load_program_files};

/// Load an explicit source selection and attach the nearest containing Go
/// module and embedded Go SDK as its dependency catalogs.
///
/// A directory invocation receives its canonical module import identity.
/// Explicit source files remain the Go command-line package, but imports are
/// resolved within the containing module. If no `go.mod` exists, the input
/// keeps `standalone_workspace` while still resolving embedded standard-library
/// packages. Catalog expansion remains query-owned and demand-driven.
pub fn load_program_files_auto<P: AsRef<Path>>(
    standalone_workspace: WorkspaceKey,
    paths: &[P],
) -> Result<LoadedProgram, LoadError> {
    let directory_invocation =
        paths.len() == 1 && paths.first().is_some_and(|path| path.as_ref().is_dir());
    let loaded = load_program_files(standalone_workspace, paths)?;
    let entry_directory = entry_directory(&loaded)?;
    let Some(module_root) = nearest_module_root(&entry_directory)? else {
        let (input, watched_directories, primary_diagnostic_path) = loaded.into_parts();
        let manifest_catalog: Arc<dyn PackageManifestCatalog> =
            Arc::new(EmbeddedGoSdkPackageManifestCatalog::new());
        let input = ProgramInput::new(
            input.workspace().clone(),
            input.entry_package().clone(),
            manifest_catalog,
        )?;
        return Ok(LoadedProgram::new(
            input,
            watched_directories,
            primary_diagnostic_path,
        ));
    };

    let source_catalog = Arc::new(LocalModuleCatalog::open(&module_root)?);
    let workspace = WorkspaceKey::module(source_catalog.module_path().shared())?;
    let entry = if directory_invocation {
        rekey_directory_entry(
            loaded.input().entry_package(),
            source_catalog.module_path(),
            source_catalog.language_version(),
            source_catalog.canonical_root(),
            &entry_directory,
        )?
    } else {
        reversion_entry(
            loaded.input().entry_package(),
            source_catalog.language_version(),
        )?
    };
    let sdk_catalog: Arc<dyn PackageManifestCatalog> =
        Arc::new(EmbeddedGoSdkPackageManifestCatalog::new());
    let local_catalog: Arc<dyn PackageManifestCatalog> =
        Arc::new(LocalModuleManifestCatalog::new(source_catalog));
    let manifest_catalog: Arc<dyn PackageManifestCatalog> =
        Arc::new(LayeredPackageManifestCatalog::new([
            sdk_catalog,
            local_catalog,
        ]));
    let input = ProgramInput::new(workspace, entry, manifest_catalog)?;
    let (_, watched_directories, primary_diagnostic_path) = loaded.into_parts();
    Ok(LoadedProgram::new(
        input,
        watched_directories,
        primary_diagnostic_path,
    ))
}

fn entry_directory(loaded: &LoadedProgram) -> Result<PathBuf, LoadError> {
    let entry = loaded
        .input()
        .entry_package()
        .files()
        .first()
        .ok_or(LoadError::NoInputPaths)?;
    let snapshot = entry.snapshot();
    let path = Path::new(snapshot.diagnostic_path());
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| LoadError::InvalidPathKind {
            path: path.to_path_buf(),
            expected: super::PathExpectation::SourceFile,
        })
}

fn nearest_module_root(start: &Path) -> Result<Option<PathBuf>, LoadError> {
    for directory in start.ancestors() {
        let go_mod = directory.join("go.mod");
        match go_mod.try_exists() {
            Ok(true) => return Ok(Some(directory.to_path_buf())),
            Ok(false) => {}
            Err(source) => {
                return Err(LoadError::io("inspect go.mod", go_mod, source));
            }
        }
    }
    Ok(None)
}

fn rekey_directory_entry(
    entry: &PackageInputManifest,
    module_path: &CanonicalImportPath,
    language_version: GoLanguageVersion,
    module_root: &Path,
    entry_directory: &Path,
) -> Result<PackageInputManifest, LoadError> {
    let relative =
        entry_directory
            .strip_prefix(module_root)
            .map_err(|_| LoadError::InvalidPathKind {
                path: entry_directory.to_path_buf(),
                expected: super::PathExpectation::SourceFile,
            })?;
    let mut import_path = module_path.as_str().to_string();
    for component in relative.components() {
        let component = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| LoadError::NonUtf8Path {
                path: entry_directory.to_path_buf(),
            })?;
        import_path.push('/');
        import_path.push_str(component);
    }
    let key = PackageKey::import_path(import_path)?;
    PackageInputManifest::new_with_language_version(
        key,
        language_version,
        entry.files().iter().cloned(),
    )
    .map_err(LoadError::from)
}

fn reversion_entry(
    entry: &PackageInputManifest,
    language_version: GoLanguageVersion,
) -> Result<PackageInputManifest, LoadError> {
    PackageInputManifest::new_with_language_version(
        entry.key().clone(),
        language_version,
        entry.files().iter().cloned(),
    )
    .map_err(LoadError::from)
}
