//! Compiler-input adapter for the embedded Go SDK source catalog.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use super::{
    PackageCatalogError, PackageInputManifest, PackageKey, PackageManifestCatalog, SourceFileInput,
};

/// Lazy immutable package manifests backed by the build-selected Go SDK.
///
/// The resolver remains source metadata only. This adapter converts exactly
/// one explicitly requested package into compiler-owned raw source inputs; it
/// never parses those sources or follows their imports. Successful manifests
/// are memoized because their embedded source snapshot is immutable.
#[derive(Debug, Default)]
pub struct EmbeddedGoSdkPackageManifestCatalog {
    manifests: RwLock<BTreeMap<PackageKey, Arc<PackageInputManifest>>>,
}

impl EmbeddedGoSdkPackageManifestCatalog {
    /// Construct an initially empty lazy view of the embedded SDK.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of package manifests materialized by this catalog instance.
    #[must_use]
    pub fn materialized_package_count(&self) -> usize {
        self.manifests
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn cached(&self, package: &PackageKey) -> Option<Arc<PackageInputManifest>> {
        self.manifests
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(package)
            .cloned()
    }

    fn publish(
        &self,
        package: PackageKey,
        manifest: Arc<PackageInputManifest>,
    ) -> Arc<PackageInputManifest> {
        let mut manifests = self
            .manifests
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(manifests.entry(package).or_insert(manifest))
    }
}

impl PackageManifestCatalog for EmbeddedGoSdkPackageManifestCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        let Some(import_path) = package.as_import_path() else {
            return Ok(None);
        };
        if let Some(manifest) = self.cached(package) {
            return Ok(Some(manifest));
        }
        let Some(files) = crate::resolve::package_files(import_path) else {
            return Ok(None);
        };

        let files = files
            .into_iter()
            .map(|(filename, source)| {
                SourceFileInput::from_source(
                    filename,
                    format!("gors://go-sdk/{import_path}/{filename}"),
                    source,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| PackageCatalogError::new(package.clone(), error))?;
        let manifest = PackageInputManifest::new(package.clone(), files)
            .map(Arc::new)
            .map_err(|error| PackageCatalogError::new(package.clone(), error))?;
        Ok(Some(self.publish(package.clone(), manifest)))
    }
}
