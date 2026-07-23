//! Lazy, parser-free source materialization for one local Go module.

mod catalog;
mod error;
mod go_mod;
mod model;

pub use catalog::LocalModuleCatalog;
pub use error::{LocalModuleError, ModuleFileIssue};
pub use go_mod::parse_module_directive;
pub use model::{MaterializedPackage, MaterializedSourceFile, PackageSourceCatalog};

#[cfg(test)]
mod tests;
