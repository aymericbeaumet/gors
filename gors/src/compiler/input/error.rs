use std::fmt;
use std::sync::Arc;

use super::{LogicalPathIssue, PackageKey};

/// Structural failure while constructing compiler-owned raw inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputError {
    EmptyWorkspaceKey,
    EmptyPackageKey,
    InvalidLogicalPath {
        path: Arc<str>,
        issue: LogicalPathIssue,
    },
    SourceTooLarge {
        path: Arc<str>,
        byte_len: usize,
    },
    DuplicateLogicalPath {
        package: PackageKey,
        path: Arc<str>,
    },
    PackageHasNoFiles {
        package: PackageKey,
    },
    DuplicatePackageKey {
        package: PackageKey,
    },
    MissingEntryPackage {
        package: PackageKey,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWorkspaceKey => formatter.write_str("workspace key must not be empty"),
            Self::EmptyPackageKey => formatter.write_str("package key must not be empty"),
            Self::InvalidLogicalPath { path, issue } => {
                write!(formatter, "invalid logical source path {path:?}: {issue}")
            }
            Self::SourceTooLarge { path, byte_len } => write!(
                formatter,
                "logical source {path:?} contains {byte_len} bytes; the compiler limit is {}",
                u32::MAX
            ),
            Self::DuplicateLogicalPath { package, path } => write!(
                formatter,
                "{package} contains duplicate logical source path {path:?}"
            ),
            Self::PackageHasNoFiles { package } => {
                write!(formatter, "{package} contains no source files")
            }
            Self::DuplicatePackageKey { package } => {
                write!(formatter, "duplicate package key: {package}")
            }
            Self::MissingEntryPackage { package } => {
                write!(formatter, "entry {package} is not in the manifest")
            }
        }
    }
}

impl std::error::Error for InputError {}
