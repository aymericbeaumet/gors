use std::fmt;
use std::sync::Arc;

use super::InputError;
use crate::import_path::CanonicalImportPath;

/// Stable caller-selected identity of one compiler workspace.
///
/// The identity is logical rather than physical. Callers must preserve it when
/// a checkout moves if they want semantic IDs to remain stable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WorkspaceKey {
    /// Workspace rooted in a Go module with this canonical module path.
    Module(CanonicalImportPath),
    /// Filesystem-free or command-line workspace with a caller-owned name.
    AdHoc(Arc<str>),
}

impl WorkspaceKey {
    /// Construct a module workspace with a canonical Go module identity.
    pub fn module(module_path: impl Into<Arc<str>>) -> Result<Self, InputError> {
        let module_path = module_path.into();
        CanonicalImportPath::new(Arc::clone(&module_path))
            .map(Self::Module)
            .map_err(|issue| InputError::InvalidWorkspaceModulePath {
                path: module_path,
                issue,
            })
    }

    /// Construct an ad-hoc workspace with a nonempty stable caller name.
    pub fn ad_hoc(name: impl Into<Arc<str>>) -> Result<Self, InputError> {
        let key = Self::AdHoc(name.into());
        key.validate()?;
        Ok(key)
    }

    /// Caller-selected module path or ad-hoc name.
    #[must_use]
    pub fn logical_name(&self) -> &str {
        match self {
            Self::Module(module_path) => module_path.as_str(),
            Self::AdHoc(name) => name,
        }
    }

    /// Typed canonical module identity when this is a Go module workspace.
    #[must_use]
    pub const fn canonical_module_path(&self) -> Option<&CanonicalImportPath> {
        match self {
            Self::Module(module_path) => Some(module_path),
            Self::AdHoc(_) => None,
        }
    }

    pub(super) fn validate(&self) -> Result<(), InputError> {
        if matches!(self, Self::AdHoc(name) if name.is_empty()) {
            return Err(InputError::EmptyAdHocWorkspaceKey);
        }
        Ok(())
    }
}

impl fmt::Display for WorkspaceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module(module_path) => write!(formatter, "module {module_path:?}"),
            Self::AdHoc(name) => write!(formatter, "ad-hoc workspace {name:?}"),
        }
    }
}

/// Stable caller-selected identity of one package in a [`WorkspaceKey`].
///
/// This key is independent of the parsed Go package clause. Entry-package
/// selection is represented separately by [`super::ProgramInput`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PackageKey {
    /// Package selected through this canonical Go import path.
    ImportPath(CanonicalImportPath),
    /// Package formed directly from command-line source arguments.
    CommandLine,
}

impl PackageKey {
    /// Construct a package with a canonical Go import-path identity.
    pub fn import_path(import_path: impl Into<Arc<str>>) -> Result<Self, InputError> {
        let import_path = import_path.into();
        CanonicalImportPath::new(Arc::clone(&import_path))
            .map(Self::ImportPath)
            .map_err(|issue| InputError::InvalidPackageImportPath {
                path: import_path,
                issue,
            })
    }

    /// Construct the unique command-line package identity.
    #[must_use]
    pub const fn command_line() -> Self {
        Self::CommandLine
    }

    /// Import path when this is an imported or module entry package.
    #[must_use]
    pub fn as_import_path(&self) -> Option<&str> {
        match self {
            Self::ImportPath(path) => Some(path.as_str()),
            Self::CommandLine => None,
        }
    }

    /// Typed canonical import identity when this is not a command-line package.
    #[must_use]
    pub const fn canonical_import_path(&self) -> Option<&CanonicalImportPath> {
        match self {
            Self::ImportPath(path) => Some(path),
            Self::CommandLine => None,
        }
    }

    /// Whether this key denotes the explicit command-line source package.
    #[must_use]
    pub const fn is_command_line(&self) -> bool {
        matches!(self, Self::CommandLine)
    }
}

impl fmt::Display for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImportPath(import_path) => write!(formatter, "import {import_path:?}"),
            Self::CommandLine => formatter.write_str("command-line package"),
        }
    }
}
