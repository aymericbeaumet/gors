use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::compiler::input::{
    PackageCatalogError, PackageInputManifest, PackageKey, PackageManifestCatalog, SourceFileInput,
};
use crate::import_path::CanonicalImportPath;

use super::{LocalModuleCatalog, LocalModuleError, MaterializedPackage};

/// Compiler-facing lazy manifest adapter for one local module catalog.
///
/// The source catalog remains parser-free and filesystem-focused. This adapter
/// performs the compiler input conversion once per requested local package and
/// memoizes the resulting immutable manifest. External imports are reported as
/// unowned so another catalog may handle them.
#[derive(Debug)]
pub struct LocalModuleManifestCatalog {
    source_catalog: Arc<LocalModuleCatalog>,
    manifests: Mutex<BTreeMap<CanonicalImportPath, Arc<PackageInputManifest>>>,
}

impl LocalModuleManifestCatalog {
    /// Adapt one already-open local module source catalog.
    #[must_use]
    pub fn new(source_catalog: Arc<LocalModuleCatalog>) -> Self {
        Self {
            source_catalog,
            manifests: Mutex::new(BTreeMap::new()),
        }
    }

    /// Share the underlying parser-free source catalog.
    #[must_use]
    pub fn source_catalog(&self) -> Arc<LocalModuleCatalog> {
        Arc::clone(&self.source_catalog)
    }

    /// Number of compiler manifests converted on demand.
    #[must_use]
    pub fn materialized_manifest_count(&self) -> usize {
        lock_manifests(&self.manifests).len()
    }

    fn cached(&self, import_path: &CanonicalImportPath) -> Option<Arc<PackageInputManifest>> {
        lock_manifests(&self.manifests)
            .get(import_path)
            .map(Arc::clone)
    }

    fn publish(
        &self,
        import_path: CanonicalImportPath,
        manifest: Arc<PackageInputManifest>,
    ) -> Arc<PackageInputManifest> {
        let mut manifests = lock_manifests(&self.manifests);
        Arc::clone(manifests.entry(import_path).or_insert(manifest))
    }
}

impl PackageManifestCatalog for LocalModuleManifestCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        let Some(import_path) = package.canonical_import_path() else {
            return Ok(None);
        };
        if let Some(manifest) = self.cached(import_path) {
            return Ok(Some(manifest));
        }

        let source_package = match self.source_catalog.materialize(import_path) {
            Ok(package) => package,
            Err(LocalModuleError::ExternalImport { .. }) => return Ok(None),
            Err(error) => {
                return Err(PackageCatalogError::new(package.clone(), error));
            }
        };
        let manifest = Arc::new(convert_manifest(package, &source_package)?);
        Ok(Some(self.publish(import_path.clone(), manifest)))
    }
}

fn convert_manifest(
    requested: &PackageKey,
    package: &MaterializedPackage,
) -> Result<PackageInputManifest, PackageCatalogError> {
    let mut files = Vec::with_capacity(package.files().len());
    for file in package.files() {
        let Some(diagnostic_path) = file.canonical_path().to_str() else {
            return Err(PackageCatalogError::new(
                requested.clone(),
                NonUtf8MaterializedPath(file.canonical_path().to_path_buf()),
            ));
        };
        files.push(
            SourceFileInput::from_source(file.logical_path(), diagnostic_path, file.source())
                .map_err(|error| PackageCatalogError::new(requested.clone(), error))?,
        );
    }
    PackageInputManifest::new(PackageKey::ImportPath(package.import_path().clone()), files)
        .map_err(|error| PackageCatalogError::new(requested.clone(), error))
}

fn lock_manifests(
    manifests: &Mutex<BTreeMap<CanonicalImportPath, Arc<PackageInputManifest>>>,
) -> MutexGuard<'_, BTreeMap<CanonicalImportPath, Arc<PackageInputManifest>>> {
    manifests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug)]
struct NonUtf8MaterializedPath(PathBuf);

impl fmt::Display for NonUtf8MaterializedPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "materialized source path is not valid UTF-8: '{}'",
            self.0.to_string_lossy()
        )
    }
}

impl Error for NonUtf8MaterializedPath {}
