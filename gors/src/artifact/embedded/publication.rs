//! Symlink-safe runtime artifact publication.

use std::path::{Path, PathBuf};

use super::{EmbeddedRuntimeArtifact, RuntimeArtifactError};

#[cfg(unix)]
mod unix;

#[cfg(not(unix))]
mod portable;

#[cfg(unix)]
use unix as platform;

#[cfg(not(unix))]
use portable as platform;

pub(super) fn verify_materialized(
    artifact: &EmbeddedRuntimeArtifact,
    path: &Path,
) -> Result<bool, RuntimeArtifactError> {
    platform::verify_materialized(artifact, path)
}

pub(super) fn materialize(
    artifact: &EmbeddedRuntimeArtifact,
    cache_root: &Path,
) -> Result<PathBuf, RuntimeArtifactError> {
    platform::materialize(artifact, cache_root)
}
