//! Syntax-unvalidated, ambient-environment-free compiler inputs.
//!
//! This layer owns invocation identity and immutable source revisions. It
//! validates only manifest structure; Go syntax and package-clause validation
//! belong to demand-driven compiler queries.

mod error;
mod keys;
mod manifest;
mod path;

pub use error::InputError;
pub use keys::{PackageKey, WorkspaceKey};
pub use manifest::{PackageInputManifest, ProgramInput, SourceFileInput};
pub use path::LogicalPathIssue;

#[cfg(test)]
mod tests;
