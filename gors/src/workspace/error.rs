use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::compiler::input::InputError;

use super::local_module::LocalModuleError;

/// Required filesystem shape for an invocation path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathExpectation {
    FileOrDirectory,
    SourceFile,
    EligibleGoSource,
}

impl fmt::Display for PathExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::FileOrDirectory => "a regular file or directory",
            Self::SourceFile => "a regular source file",
            Self::EligibleGoSource => {
                "an eligible .go file that is not hidden, underscore-prefixed, or a test"
            }
        };
        formatter.write_str(message)
    }
}

/// Failure while discovering or reading a raw compiler workspace.
#[derive(Debug)]
pub enum LoadError {
    NoInputPaths,
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    NonUtf8Path {
        path: PathBuf,
    },
    NonUtf8Source {
        path: PathBuf,
    },
    InvalidPathKind {
        path: PathBuf,
        expected: PathExpectation,
    },
    NoGoFiles {
        directory: PathBuf,
    },
    DuplicateSourceFile {
        path: PathBuf,
    },
    SourceFilesFromDifferentDirectories {
        expected_directory: PathBuf,
        file: PathBuf,
        found_directory: PathBuf,
    },
    LocalModule(LocalModuleError),
    InvalidManifest(InputError),
}

impl LoadError {
    pub(super) fn io(operation: &'static str, path: PathBuf, source: io::Error) -> Self {
        Self::Io {
            operation,
            path,
            source,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputPaths => formatter.write_str("no source paths were provided"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "cannot {operation} '{}': {source}",
                path.to_string_lossy()
            ),
            Self::NonUtf8Path { path } => write!(
                formatter,
                "source path is not valid UTF-8: '{}'",
                path.to_string_lossy()
            ),
            Self::NonUtf8Source { path } => write!(
                formatter,
                "Go source is not valid UTF-8: '{}'",
                path.to_string_lossy()
            ),
            Self::InvalidPathKind { path, expected } => write!(
                formatter,
                "invalid source path '{}': expected {expected}",
                path.to_string_lossy()
            ),
            Self::NoGoFiles { directory } => write!(
                formatter,
                "no eligible Go files found in '{}'",
                directory.to_string_lossy()
            ),
            Self::DuplicateSourceFile { path } => write!(
                formatter,
                "source file was supplied more than once: '{}'",
                path.to_string_lossy()
            ),
            Self::SourceFilesFromDifferentDirectories {
                expected_directory,
                file,
                found_directory,
            } => write!(
                formatter,
                "source file '{}' is in '{}', expected '{}'",
                file.to_string_lossy(),
                found_directory.to_string_lossy(),
                expected_directory.to_string_lossy()
            ),
            Self::LocalModule(error) => error.fmt(formatter),
            Self::InvalidManifest(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::LocalModule(error) => Some(error),
            Self::InvalidManifest(error) => Some(error),
            _ => None,
        }
    }
}

impl From<InputError> for LoadError {
    fn from(error: InputError) -> Self {
        Self::InvalidManifest(error)
    }
}

impl From<LocalModuleError> for LoadError {
    fn from(error: LocalModuleError) -> Self {
        Self::LocalModule(error)
    }
}
