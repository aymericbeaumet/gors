use std::fmt;
use std::sync::Arc;

use super::{GoLanguageVersionParseError, LogicalPathIssue, PackageKey};
use crate::import_path::ImportPathIssue;

/// Structural failure while constructing compiler-owned raw inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputError {
    EmptyAdHocWorkspaceKey,
    InvalidWorkspaceModulePath {
        path: Arc<str>,
        issue: ImportPathIssue,
    },
    InvalidPackageImportPath {
        path: Arc<str>,
        issue: ImportPathIssue,
    },
    InvalidLogicalPath {
        path: Arc<str>,
        issue: LogicalPathIssue,
    },
    SourceTooLarge {
        path: Arc<str>,
        byte_len: usize,
    },
    InvalidLanguageVersion {
        value: Arc<str>,
        issue: GoLanguageVersionParseError,
    },
    DuplicateLogicalPath {
        package: PackageKey,
        path: Arc<str>,
    },
    PackageHasNoFiles {
        package: PackageKey,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAdHocWorkspaceKey => {
                formatter.write_str("ad-hoc workspace key must not be empty")
            }
            Self::InvalidWorkspaceModulePath { path, issue } => {
                write!(formatter, "invalid workspace module path {path:?}: {issue}")
            }
            Self::InvalidPackageImportPath { path, issue } => {
                write!(formatter, "invalid package import path {path:?}: {issue}")
            }
            Self::InvalidLogicalPath { path, issue } => {
                write!(formatter, "invalid logical source path {path:?}: {issue}")
            }
            Self::SourceTooLarge { path, byte_len } => write!(
                formatter,
                "logical source {path:?} contains {byte_len} bytes; the compiler limit is {}",
                u32::MAX
            ),
            Self::InvalidLanguageVersion { value, issue } => {
                write!(
                    formatter,
                    "invalid compiler language version {value:?}: {issue}"
                )
            }
            Self::DuplicateLogicalPath { package, path } => write!(
                formatter,
                "{package} contains duplicate logical source path {path:?}"
            ),
            Self::PackageHasNoFiles { package } => {
                write!(formatter, "{package} contains no source files")
            }
        }
    }
}

impl std::error::Error for InputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageVersion { issue, .. } => Some(issue),
            _ => None,
        }
    }
}
