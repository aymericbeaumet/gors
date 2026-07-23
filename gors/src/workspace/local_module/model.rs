use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::import_path::CanonicalImportPath;

/// One immutable, syntax-unvalidated source snapshot from a local package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedSourceFile {
    logical_path: Arc<str>,
    canonical_path: PathBuf,
    source: Arc<str>,
}

impl MaterializedSourceFile {
    pub(super) fn new(logical_path: Arc<str>, canonical_path: PathBuf, source: Arc<str>) -> Self {
        Self {
            logical_path,
            canonical_path,
            source,
        }
    }

    /// Package-relative source identity.
    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    /// Canonical presentation path observed when the package was materialized.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Exact UTF-8 source bytes observed once for this catalog snapshot.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Canonically ordered immutable source inputs for one local package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedPackage {
    import_path: CanonicalImportPath,
    canonical_directory: PathBuf,
    files: Arc<[MaterializedSourceFile]>,
}

impl MaterializedPackage {
    pub(super) fn new(
        import_path: CanonicalImportPath,
        canonical_directory: PathBuf,
        files: Arc<[MaterializedSourceFile]>,
    ) -> Self {
        Self {
            import_path,
            canonical_directory,
            files,
        }
    }

    /// Stable canonical import identity requested by the caller.
    #[must_use]
    pub const fn import_path(&self) -> &CanonicalImportPath {
        &self.import_path
    }

    /// Canonical package directory contained by the module root.
    #[must_use]
    pub fn canonical_directory(&self) -> &Path {
        &self.canonical_directory
    }

    /// Immediate eligible non-test Go files in logical-path order.
    #[must_use]
    pub fn files(&self) -> &[MaterializedSourceFile] {
        &self.files
    }
}

/// Demand-driven package-source provider independent of compiler manifests.
pub trait PackageSourceCatalog: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Materialize exactly one requested package without following its imports.
    fn materialize(
        &self,
        import_path: &CanonicalImportPath,
    ) -> Result<Arc<MaterializedPackage>, Self::Error>;
}
