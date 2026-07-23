use std::sync::Arc;

use super::path::validate_logical_path;
use super::{
    EmptyPackageManifestCatalog, InputError, PackageKey, PackageManifestCatalog, SourceSnapshot,
    WorkspaceKey,
};
use crate::source::TextSizeOverflow;

/// One stable logical source file paired with an immutable raw source revision.
///
/// `logical_path` participates in semantic identity. The diagnostic path held
/// by [`SourceSnapshot`] is presentation-only and does not affect ordering or
/// duplicate detection.
#[derive(Clone, Debug)]
pub struct SourceFileInput {
    logical_path: Arc<str>,
    snapshot: Arc<SourceSnapshot>,
}

impl SourceFileInput {
    /// Construct one raw source input after lexical path validation only.
    pub fn new(
        logical_path: impl Into<Arc<str>>,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<Self, InputError> {
        let logical_path = logical_path.into();
        validate_logical_path(&logical_path).map_err(|issue| InputError::InvalidLogicalPath {
            path: Arc::clone(&logical_path),
            issue,
        })?;
        Ok(Self {
            logical_path,
            snapshot,
        })
    }

    pub(super) fn source_size_error(
        logical_path: &Arc<str>,
        error: TextSizeOverflow,
    ) -> InputError {
        InputError::SourceTooLarge {
            path: Arc::clone(logical_path),
            byte_len: error.value(),
        }
    }

    /// Construct an owned snapshot and pair it with a stable logical path.
    pub fn from_source(
        logical_path: impl Into<Arc<str>>,
        diagnostic_path: impl Into<Arc<str>>,
        source: impl Into<Arc<str>>,
    ) -> Result<Self, InputError> {
        let logical_path = logical_path.into();
        let snapshot = SourceSnapshot::from_source(diagnostic_path, source)
            .map_err(|error| Self::source_size_error(&logical_path, error))?;
        Self::new(logical_path, Arc::new(snapshot))
    }

    /// Slash-normalized package-relative identity path.
    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    /// Shared syntax-unvalidated source revision.
    #[must_use]
    pub fn snapshot(&self) -> Arc<SourceSnapshot> {
        Arc::clone(&self.snapshot)
    }
}

/// Canonically ordered raw source inputs for one explicitly identified package.
#[derive(Clone, Debug)]
pub struct PackageInputManifest {
    key: PackageKey,
    files: Arc<[SourceFileInput]>,
}

impl PackageInputManifest {
    /// Validate duplicate logical paths and canonicalize file ordering.
    pub fn new(
        key: PackageKey,
        files: impl IntoIterator<Item = SourceFileInput>,
    ) -> Result<Self, InputError> {
        let mut files = files.into_iter().collect::<Vec<_>>();
        if files.is_empty() {
            return Err(InputError::PackageHasNoFiles { package: key });
        }
        files.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        if let Some(duplicate_path) = files.windows(2).find_map(|pair| {
            let [left, right] = pair else {
                return None;
            };
            (left.logical_path == right.logical_path).then(|| Arc::clone(&left.logical_path))
        }) {
            return Err(InputError::DuplicateLogicalPath {
                package: key,
                path: duplicate_path,
            });
        }
        Ok(Self {
            key,
            files: files.into(),
        })
    }

    /// Stable package identity independent of parsed source text.
    #[must_use]
    pub const fn key(&self) -> &PackageKey {
        &self.key
    }

    /// Source inputs in canonical logical-path order.
    #[must_use]
    pub fn files(&self) -> &[SourceFileInput] {
        &self.files
    }
}

/// Complete syntax-unvalidated input manifest for one compiler invocation.
#[derive(Clone, Debug)]
pub struct ProgramInput {
    workspace: WorkspaceKey,
    entry_package: PackageInputManifest,
    package_catalog: Arc<dyn PackageManifestCatalog>,
}

impl ProgramInput {
    /// Bind one explicit entry package to its demand-driven package catalog.
    ///
    /// The entry manifest is authoritative and is never rematerialized through
    /// the catalog. Imported packages are requested individually only when a
    /// later compiler query proves them reachable.
    pub fn new(
        workspace: WorkspaceKey,
        entry_package: PackageInputManifest,
        package_catalog: Arc<dyn PackageManifestCatalog>,
    ) -> Result<Self, InputError> {
        workspace.validate()?;
        Ok(Self {
            workspace,
            entry_package,
            package_catalog,
        })
    }

    /// Construct an intentionally closed input containing only its entry.
    pub fn standalone(
        workspace: WorkspaceKey,
        entry_package: PackageInputManifest,
    ) -> Result<Self, InputError> {
        Self::new(
            workspace,
            entry_package,
            Arc::new(EmptyPackageManifestCatalog),
        )
    }

    /// Explicit workspace identity for every package and source in this input.
    #[must_use]
    pub const fn workspace(&self) -> &WorkspaceKey {
        &self.workspace
    }

    /// Explicitly selected entry package, independent of Go package clauses.
    #[must_use]
    pub const fn entry_package(&self) -> &PackageInputManifest {
        &self.entry_package
    }

    /// Share the lazy provider used for individually requested dependencies.
    #[must_use]
    pub fn package_catalog(&self) -> Arc<dyn PackageManifestCatalog> {
        Arc::clone(&self.package_catalog)
    }
}
