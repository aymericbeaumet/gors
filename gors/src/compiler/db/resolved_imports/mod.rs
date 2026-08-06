//! File-scoped import resolution inputs.
//!
//! Source projection owns import syntax and provenance. Package admission
//! resolves each occurrence to an exact package identity and installs this
//! immutable product. Later semantic queries can therefore consume package
//! identity and local binding facts without rediscovering either from strings.

mod mutation;

use std::fmt;
use std::sync::Arc;

use crate::compiler::ids::{FileId, PackageId};
use crate::compiler::provenance::FileRange;
use crate::import_path::CanonicalImportPath;

use super::queries::PackageInput;
use super::{DirectImport, ImportBinding};

pub(in crate::compiler) use mutation::ResolvedImportInputMutation;
pub use mutation::ResolvedImportsUpdate;

#[cfg(test)]
mod tests;

/// File-scoped name introduced by one resolved Go import.
///
/// Default imports retain the target package's declared package-clause name.
/// That name is not derivable from the import path: a package may legally
/// declare a different name than its path base.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResolvedImportBinding {
    /// The target package's actual declared package name.
    Default {
        local_name: Arc<str>,
        source: FileRange,
    },
    /// An explicit source alias.
    Named {
        local_name: Arc<str>,
        source: FileRange,
    },
    /// A side-effect-only import.
    Blank { source: FileRange },
    /// An import whose exported declarations enter the file scope.
    Dot { source: FileRange },
}

impl ResolvedImportBinding {
    /// Ordinary file-scope package name, when this binding introduces one.
    #[must_use]
    pub fn local_name(&self) -> Option<&str> {
        match self {
            Self::Default { local_name, .. } | Self::Named { local_name, .. } => Some(local_name),
            Self::Blank { .. } | Self::Dot { .. } => None,
        }
    }

    /// Physical source token selecting this binding.
    #[must_use]
    pub const fn source(&self) -> FileRange {
        match self {
            Self::Default { source, .. }
            | Self::Named { source, .. }
            | Self::Blank { source }
            | Self::Dot { source } => *source,
        }
    }

    fn from_occurrence(binding: &ImportBinding, target_package_name: Arc<str>) -> Self {
        match binding {
            ImportBinding::Default { source } => Self::Default {
                local_name: target_package_name,
                source: *source,
            },
            ImportBinding::Named { name, source } => Self::Named {
                local_name: Arc::clone(name),
                source: *source,
            },
            ImportBinding::Blank { source } => Self::Blank { source: *source },
            ImportBinding::Dot { source } => Self::Dot { source: *source },
        }
    }

    fn retained_bytes(&self) -> usize {
        self.local_name().map_or(0, str::len)
    }
}

/// One source occurrence resolved to an exact admitted package.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResolvedImport {
    target_package: PackageId,
    canonical_path: CanonicalImportPath,
    binding: ResolvedImportBinding,
    source: FileRange,
}

impl ResolvedImport {
    /// Resolve a projected source occurrence.
    ///
    /// `target_package_name` must be the parsed package-clause name of
    /// `target_package`, not an import-path-derived guess. It is consumed only
    /// by default imports; explicit aliases already own their local name.
    #[must_use]
    pub fn from_occurrence(
        target_package: PackageId,
        target_package_name: impl Into<Arc<str>>,
        occurrence: &DirectImport,
    ) -> Self {
        Self {
            target_package,
            canonical_path: occurrence.canonical_path().clone(),
            binding: ResolvedImportBinding::from_occurrence(
                occurrence.binding(),
                target_package_name.into(),
            ),
            source: occurrence.source(),
        }
    }

    /// Construct a resolved import while validating its two source anchors.
    ///
    /// # Errors
    ///
    /// Returns [`ResolvedImportBuildError::BindingSourceFileMismatch`] when
    /// the binding token and import literal do not belong to the same file.
    pub fn try_new(
        target_package: PackageId,
        canonical_path: CanonicalImportPath,
        binding: ResolvedImportBinding,
        source: FileRange,
    ) -> Result<Self, ResolvedImportBuildError> {
        if binding.source().file() != source.file() {
            return Err(ResolvedImportBuildError::BindingSourceFileMismatch {
                import_file: source.file(),
                binding_file: binding.source().file(),
            });
        }
        Ok(Self {
            target_package,
            canonical_path,
            binding,
            source,
        })
    }

    /// Stable identity of the resolved dependency package.
    #[must_use]
    pub const fn target_package(&self) -> PackageId {
        self.target_package
    }

    /// Canonical import path written by the source occurrence.
    #[must_use]
    pub const fn canonical_path(&self) -> &CanonicalImportPath {
        &self.canonical_path
    }

    /// Exact file-scoped binding selected by the source occurrence.
    #[must_use]
    pub const fn binding(&self) -> &ResolvedImportBinding {
        &self.binding
    }

    /// Exact physical range of the import-path literal.
    #[must_use]
    pub const fn source(&self) -> FileRange {
        self.source
    }

    /// Stable identity of the importing source file.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.source.file()
    }

    fn retained_bytes(&self) -> usize {
        self.canonical_path
            .as_str()
            .len()
            .saturating_add(self.binding.retained_bytes())
    }
}

/// Immutable resolved-import occurrences for exactly one source file.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ResolvedFileImports {
    file: FileId,
    imports: Arc<[ResolvedImport]>,
}

impl ResolvedFileImports {
    /// Construct one source-order file product.
    ///
    /// # Errors
    ///
    /// Returns [`ResolvedImportBuildError::ImportSourceFileMismatch`] when an
    /// occurrence belongs to another source file.
    pub fn try_new(
        file: FileId,
        imports: impl Into<Arc<[ResolvedImport]>>,
    ) -> Result<Self, ResolvedImportBuildError> {
        let imports = imports.into();
        if let Some(import) = imports.iter().find(|import| import.file() != file) {
            return Err(ResolvedImportBuildError::ImportSourceFileMismatch {
                expected: file,
                actual: import.file(),
            });
        }
        Ok(Self { file, imports })
    }

    pub(super) fn empty(file: FileId) -> Self {
        Self {
            file,
            imports: Arc::from([]),
        }
    }

    /// Stable identity of the importing source file.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Resolved import occurrences in source order, including duplicates.
    #[must_use]
    pub fn imports(&self) -> &[ResolvedImport] {
        &self.imports
    }

    /// Approximate retained bytes for memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.imports.iter().fold(32_usize, |total, import| {
            total.saturating_add(import.retained_bytes())
        })
    }
}

/// Structural inconsistency while constructing a resolved-import product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedImportBuildError {
    BindingSourceFileMismatch {
        import_file: FileId,
        binding_file: FileId,
    },
    ImportSourceFileMismatch {
        expected: FileId,
        actual: FileId,
    },
}

impl fmt::Display for ResolvedImportBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindingSourceFileMismatch {
                import_file,
                binding_file,
            } => write!(
                formatter,
                "resolved import literal belongs to {import_file:?}, but its binding belongs to {binding_file:?}"
            ),
            Self::ImportSourceFileMismatch { expected, actual } => write!(
                formatter,
                "resolved import set belongs to {expected:?}, but contains an occurrence from {actual:?}"
            ),
        }
    }
}

impl std::error::Error for ResolvedImportBuildError {}

/// Salsa-owned input handle for one file-scoped resolution product.
#[salsa::input]
pub(super) struct ResolvedImportsInput {
    #[returns(copy)]
    pub(super) file: FileId,
    #[returns(clone)]
    pub(super) value: Arc<ResolvedFileImports>,
    #[returns(clone)]
    pub(super) targets: Arc<[(PackageId, PackageInput)]>,
}
