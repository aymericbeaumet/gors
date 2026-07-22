//! Filesystem discovery for syntax-unvalidated compiler inputs.
//!
//! This boundary selects and reads source files. It deliberately performs no
//! scanning, parsing, import discovery, package-clause validation, or semantic
//! work.

mod error;
mod loader;
mod model;

pub use error::{LoadError, PathExpectation};
pub use loader::{load_program, load_program_files};
pub use model::LoadedProgram;

#[cfg(test)]
mod tests;
