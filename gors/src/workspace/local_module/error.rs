use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::compiler::input::GoLanguageVersionParseError;
use crate::import_path::{CanonicalImportPath, ImportPathIssue};

/// Structural failure in a local module's `go.mod` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleFileIssue {
    NonUtf8,
    MissingModuleDirective,
    DuplicateModuleDirective {
        first_line: usize,
        duplicate_line: usize,
    },
    MalformedModuleDirective {
        line: usize,
    },
    DuplicateGoDirective {
        first_line: usize,
        duplicate_line: usize,
    },
    MalformedGoDirective {
        line: usize,
    },
    InvalidGoVersion {
        line: usize,
        version: std::sync::Arc<str>,
        issue: GoLanguageVersionParseError,
    },
    InvalidModulePath {
        line: usize,
        issue: ImportPathIssue,
    },
}

impl fmt::Display for ModuleFileIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUtf8 => formatter.write_str("go.mod is not valid UTF-8"),
            Self::MissingModuleDirective => formatter.write_str("go.mod has no module directive"),
            Self::DuplicateModuleDirective {
                first_line,
                duplicate_line,
            } => write!(
                formatter,
                "go.mod repeats its module directive on line {duplicate_line} \
                 (first declared on line {first_line})"
            ),
            Self::MalformedModuleDirective { line } => {
                write!(formatter, "malformed module directive on line {line}")
            }
            Self::DuplicateGoDirective {
                first_line,
                duplicate_line,
            } => write!(
                formatter,
                "go.mod repeats its go directive on line {duplicate_line} \
                 (first declared on line {first_line})"
            ),
            Self::MalformedGoDirective { line } => {
                write!(formatter, "malformed go directive on line {line}")
            }
            Self::InvalidGoVersion {
                line,
                version,
                issue,
            } => write!(
                formatter,
                "invalid Go language version {version:?} on line {line}: {issue}"
            ),
            Self::InvalidModulePath { line, issue } => {
                write!(formatter, "invalid module path on line {line}: {issue}")
            }
        }
    }
}

impl std::error::Error for ModuleFileIssue {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidModulePath { issue, .. } => Some(issue),
            Self::InvalidGoVersion { issue, .. } => Some(issue),
            _ => None,
        }
    }
}

/// Failure while opening or lazily materializing a local Go module.
#[derive(Debug)]
pub enum LocalModuleError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    InvalidModuleRoot {
        path: PathBuf,
    },
    MalformedModule {
        path: PathBuf,
        issue: ModuleFileIssue,
    },
    ExternalImport {
        requested: CanonicalImportPath,
        module: CanonicalImportPath,
    },
    MissingPackage {
        requested: CanonicalImportPath,
        directory: PathBuf,
    },
    PackageHasNoGoFiles {
        requested: CanonicalImportPath,
        directory: PathBuf,
    },
    ContainmentEscape {
        requested: Option<CanonicalImportPath>,
        module_root: PathBuf,
        lexical_path: PathBuf,
        canonical_path: Option<PathBuf>,
    },
    NonUtf8Path {
        path: PathBuf,
    },
    NonUtf8Source {
        path: PathBuf,
    },
}

impl LocalModuleError {
    pub(super) fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for LocalModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "cannot {operation} '{}': {source}",
                path.to_string_lossy()
            ),
            Self::InvalidModuleRoot { path } => write!(
                formatter,
                "local module root is not a directory: '{}'",
                path.to_string_lossy()
            ),
            Self::MalformedModule { path, issue } => {
                write!(
                    formatter,
                    "malformed module file '{}': {issue}",
                    path.display()
                )
            }
            Self::ExternalImport { requested, module } => write!(
                formatter,
                "import {requested:?} is outside local module {module:?}"
            ),
            Self::MissingPackage {
                requested,
                directory,
            } => write!(
                formatter,
                "local package {requested:?} does not exist at '{}'",
                directory.display()
            ),
            Self::PackageHasNoGoFiles {
                requested,
                directory,
            } => write!(
                formatter,
                "local package {requested:?} has no eligible Go files in '{}'",
                directory.display()
            ),
            Self::ContainmentEscape {
                requested,
                module_root,
                lexical_path,
                canonical_path,
            } => {
                let subject = requested
                    .as_ref()
                    .map_or("module metadata", |path| path.as_str());
                write!(
                    formatter,
                    "{subject:?} escapes module root '{}': lexical path '{}'",
                    module_root.display(),
                    lexical_path.display()
                )?;
                if let Some(canonical_path) = canonical_path {
                    write!(
                        formatter,
                        " resolves to '{}'",
                        canonical_path.to_string_lossy()
                    )?;
                }
                Ok(())
            }
            Self::NonUtf8Path { path } => write!(
                formatter,
                "local module path is not valid UTF-8: '{}'",
                path.to_string_lossy()
            ),
            Self::NonUtf8Source { path } => write!(
                formatter,
                "Go source is not valid UTF-8: '{}'",
                path.to_string_lossy()
            ),
        }
    }
}

impl std::error::Error for LocalModuleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::MalformedModule { issue, .. } => Some(issue),
            _ => None,
        }
    }
}
