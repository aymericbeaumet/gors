//! Deterministic package dependency topology over compiler-owned identities.
//!
//! This module does not discover, resolve, or install packages. Callers supply
//! already-resolved imports and receive an immutable dependency-before-importer
//! topology or deterministic structural diagnostics.

mod build;
mod model;

pub use build::build_package_dag;
pub use model::{
    PackageCycle, PackageCycleHop, PackageDag, PackageDagEdge, PackageDagError, PackageDagImport,
    PackageDagLayer, PackageDagNode,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
