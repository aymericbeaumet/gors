//! Deterministic file and package indexing issues.

use std::sync::Arc;

use super::ParseFailure;
use crate::compiler::ids::{DefId, FileId};
use crate::import_path::ImportPathIssue;

/// Non-syntax issue discovered while indexing declarations.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FileIssue {
    /// A Go file declares the same package-level name more than once.
    DuplicateDefinition(Arc<str>),
    /// Parser observations could not be projected into owned function syntax.
    FunctionProjectionFailure { name: Arc<str>, message: Arc<str> },
    /// A parsed constant could not be projected into owned semantic syntax.
    ConstantProjectionFailure { name: Arc<str>, message: Arc<str> },
    /// A parsed variable could not be projected into owned semantic syntax.
    VariableProjectionFailure { name: Arc<str>, message: Arc<str> },
    /// A parsed type declaration could not be projected into owned semantic syntax.
    TypeProjectionFailure { name: Arc<str>, message: Arc<str> },
}

/// Deterministic package-index issue, distinct from stable-ID interning.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PackageIssue {
    /// One package input file currently has invalid Go syntax.
    FileParseFailure { file: FileId, failure: ParseFailure },
    /// One import literal cannot name a canonical Go package.
    InvalidImportPath {
        file: FileId,
        literal: Arc<str>,
        line: usize,
        column: usize,
        virtual_file: Option<Arc<str>>,
        issue: ImportPathIssue,
    },
    /// Independently parsed files disagree on their package clause.
    PackageClauseMismatch {
        file: FileId,
        expected: Arc<str>,
        found: Arc<str>,
    },
    /// The same package-level name was declared by two source files.
    DuplicateDefinition {
        name: Arc<str>,
        first_file: FileId,
        second_file: FileId,
    },
    /// Distinct complete definition keys produced the same compact digest.
    IdentityCollision {
        id: DefId,
        existing_key: Arc<str>,
        requested_key: Arc<str>,
    },
    /// One parser product could not be projected into owned semantic syntax.
    FunctionProjectionFailure {
        file: FileId,
        name: Arc<str>,
        message: Arc<str>,
    },
    /// One parsed constant could not be projected into owned semantic syntax.
    ConstantProjectionFailure {
        file: FileId,
        name: Arc<str>,
        message: Arc<str>,
    },
    /// One parsed variable could not be projected into owned semantic syntax.
    VariableProjectionFailure {
        file: FileId,
        name: Arc<str>,
        message: Arc<str>,
    },
    /// One parsed type declaration could not be projected into owned semantic syntax.
    TypeProjectionFailure {
        file: FileId,
        name: Arc<str>,
        message: Arc<str>,
    },
}
