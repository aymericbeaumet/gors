use std::error::Error;
use std::fmt;
use std::sync::Arc;

use super::{PackageInputManifest, PackageKey};

/// Demand-driven provider of immutable, syntax-unvalidated package manifests.
///
/// A catalog must not follow imports on its own. Each call materializes at most
/// the requested package, and repeated successful requests must describe the
/// same immutable source snapshot. `Ok(None)` means that the catalog does not
/// own the requested package namespace; an owned package that cannot be loaded
/// is an error.
pub trait PackageManifestCatalog: fmt::Debug + Send + Sync {
    /// Materialize one package without recursively resolving its imports.
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError>;
}

/// Ordered composition of independent package namespaces.
///
/// Each provider remains demand-driven: materializing one package asks layers
/// in precedence order and stops at the first owner. A layer error is retained
/// instead of being mistaken for an unowned namespace and falling through to
/// another provider.
#[derive(Debug)]
pub struct LayeredPackageManifestCatalog {
    layers: Arc<[Arc<dyn PackageManifestCatalog>]>,
}

impl LayeredPackageManifestCatalog {
    /// Compose package providers in explicit precedence order.
    #[must_use]
    pub fn new(layers: impl IntoIterator<Item = Arc<dyn PackageManifestCatalog>>) -> Self {
        Self {
            layers: layers.into_iter().collect(),
        }
    }

    /// Number of independently owned namespaces in this composition.
    #[must_use]
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

impl PackageManifestCatalog for LayeredPackageManifestCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        for layer in self.layers.iter() {
            if let Some(manifest) = layer.materialize(package)? {
                return Ok(Some(manifest));
            }
        }
        Ok(None)
    }
}

/// Failure to materialize an owned package manifest.
///
/// The concrete cause is retained as an error source so resolver, filesystem,
/// and manifest failures remain inspectable instead of being flattened into a
/// string at the compiler boundary.
#[derive(Debug)]
pub struct PackageCatalogError {
    package: PackageKey,
    cause: Box<dyn Error + Send + Sync>,
}

impl PackageCatalogError {
    /// Preserve one concrete catalog failure for the requested package.
    pub fn new(package: PackageKey, cause: impl Error + Send + Sync + 'static) -> Self {
        Self {
            package,
            cause: Box::new(cause),
        }
    }

    /// Package whose manifest could not be materialized.
    #[must_use]
    pub const fn package(&self) -> &PackageKey {
        &self.package
    }

    /// Concrete catalog failure retained for structured inspection.
    #[must_use]
    pub fn cause(&self) -> &(dyn Error + Send + Sync + 'static) {
        self.cause.as_ref()
    }
}

impl fmt::Display for PackageCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot materialize {}: {}",
            self.package, self.cause
        )
    }
}

impl Error for PackageCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

/// Catalog for a deliberately closed, entry-only input.
#[derive(Debug, Default)]
pub struct EmptyPackageManifestCatalog;

impl PackageManifestCatalog for EmptyPackageManifestCatalog {
    fn materialize(
        &self,
        _package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        Ok(None)
    }
}
