//! Lazy, parser-free source materialization for one local Go module.

mod catalog;
mod compiler_catalog;
mod error;
mod go_mod;
mod model;

pub use catalog::LocalModuleCatalog;
pub use compiler_catalog::LocalModuleManifestCatalog;
pub use error::{LocalModuleError, ModuleFileIssue};
pub use go_mod::parse_module_directive;
pub use model::{MaterializedPackage, MaterializedSourceFile};

#[cfg(test)]
mod tests;
