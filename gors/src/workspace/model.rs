use std::path::PathBuf;
use std::sync::Arc;

use crate::compiler::input::ProgramInput;

/// Raw compiler input plus canonical filesystem observation metadata.
#[derive(Clone, Debug)]
pub struct LoadedProgram {
    input: ProgramInput,
    watched_directories: Arc<[PathBuf]>,
    primary_diagnostic_path: Arc<str>,
}

impl LoadedProgram {
    pub(super) fn new(
        input: ProgramInput,
        watched_directories: Arc<[PathBuf]>,
        primary_diagnostic_path: Arc<str>,
    ) -> Self {
        Self {
            input,
            watched_directories,
            primary_diagnostic_path,
        }
    }

    /// Canonical, syntax-unvalidated compiler manifest.
    #[must_use]
    pub const fn input(&self) -> &ProgramInput {
        &self.input
    }

    /// Canonical directories whose eligible-file membership affects this load.
    ///
    /// Explicit file lists have no watched directory: adding an unlisted file
    /// must not change that invocation.
    #[must_use]
    pub fn watched_directories(&self) -> &[PathBuf] {
        &self.watched_directories
    }

    /// Canonical UTF-8 path used when a later diagnostic has no source span.
    #[must_use]
    pub fn primary_diagnostic_path(&self) -> &str {
        &self.primary_diagnostic_path
    }

    /// Consume the loader wrapper and return the compiler-owned raw manifest.
    #[must_use]
    pub fn into_input(self) -> ProgramInput {
        self.input
    }

    /// Consume this result without discarding cache-observation metadata.
    #[must_use]
    pub fn into_parts(self) -> (ProgramInput, Arc<[PathBuf]>, Arc<str>) {
        (
            self.input,
            self.watched_directories,
            self.primary_diagnostic_path,
        )
    }
}
