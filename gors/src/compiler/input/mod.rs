//! Syntax-unvalidated, ambient-environment-free compiler inputs.
//!
//! This layer owns invocation identity and immutable source revisions. It
//! validates only manifest structure; Go syntax and package-clause validation
//! belong to demand-driven compiler queries.

mod catalog;
mod error;
mod go_sdk_catalog;
mod keys;
mod language_version;
mod manifest;
mod path;
mod source;

pub use catalog::{
    EmptyPackageManifestCatalog, LayeredPackageManifestCatalog, PackageCatalogError,
    PackageManifestCatalog,
};
pub use error::InputError;
pub use go_sdk_catalog::EmbeddedGoSdkPackageManifestCatalog;
pub use keys::{PackageKey, WorkspaceKey};
pub use language_version::{GoLanguageVersion, GoLanguageVersionParseError};
pub use manifest::{PackageInputManifest, ProgramInput, SourceFileInput};
pub use path::LogicalPathIssue;
pub use source::{SourceContent, SourceSnapshot};

#[cfg(test)]
mod tests;
