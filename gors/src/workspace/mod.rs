//! Filesystem discovery for syntax-unvalidated compiler inputs.
//!
//! This boundary selects and reads source files. It deliberately performs no
//! scanning, parsing, import discovery, package-clause validation, or semantic
//! work. Callers supply the stable logical workspace identity explicitly;
//! physical source paths are presentation and filesystem state only.

mod error;
mod loader;
mod model;

pub use error::{LoadError, PathExpectation};
pub use loader::{load_program, load_program_files};
pub use model::LoadedProgram;

#[cfg(test)]
mod tests;
