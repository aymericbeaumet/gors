//! Filesystem discovery for syntax-unvalidated compiler inputs.
//!
//! This boundary selects and reads source files. It deliberately performs no
//! scanning, parsing, import discovery, package-clause validation, or semantic
//! work. Callers supply the stable logical workspace identity explicitly;
//! physical source paths are presentation and filesystem state only.

mod error;
mod loader;
pub mod local_module;
mod model;
mod module_loader;

pub use error::{LoadError, PathExpectation};
pub use loader::{load_program, load_program_files};
pub use model::LoadedProgram;
pub use module_loader::load_program_files_auto;

#[cfg(test)]
mod tests;
