use std::sync::Arc;

use super::path::validate_logical_path;
use super::{InputError, PackageKey, SourceSnapshot, WorkspaceKey};
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
        key.validate()?;
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
    packages: Arc<[PackageInputManifest]>,
}

impl ProgramInput {
    /// Validate package uniqueness and entry membership, then canonicalize order.
    pub fn new(
        workspace: WorkspaceKey,
        entry_package: PackageKey,
        packages: impl IntoIterator<Item = PackageInputManifest>,
    ) -> Result<Self, InputError> {
        workspace.validate()?;
        entry_package.validate()?;
        let mut packages = packages.into_iter().collect::<Vec<_>>();
        packages.sort_by(|left, right| left.key.cmp(&right.key));
        if let Some(duplicate_key) = packages.windows(2).find_map(|pair| {
            let [left, right] = pair else {
                return None;
            };
            (left.key == right.key).then(|| left.key.clone())
        }) {
            return Err(InputError::DuplicatePackageKey {
                package: duplicate_key,
            });
        }
        let entry = packages
            .binary_search_by(|package| package.key.cmp(&entry_package))
            .ok()
            .and_then(|index| packages.get(index))
            .cloned()
            .ok_or(InputError::MissingEntryPackage {
                package: entry_package,
            })?;
        Ok(Self {
            workspace,
            entry_package: entry,
            packages: packages.into(),
        })
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

    /// Every package in canonical package-key order, including the entry.
    #[must_use]
    pub fn packages(&self) -> &[PackageInputManifest] {
        &self.packages
    }

    /// Look up one package by stable key.
    #[must_use]
    pub fn package(&self, key: &PackageKey) -> Option<&PackageInputManifest> {
        self.packages
            .binary_search_by(|package| package.key.cmp(key))
            .ok()
            .and_then(|index| self.packages.get(index))
    }
}
