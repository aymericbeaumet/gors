use std::path::PathBuf;

use crate::cache::GeneratedRustIdentity;

pub fn build_cache_dir(cache_base: &std::path::Path, identity: &GeneratedRustIdentity) -> PathBuf {
    cache_base.join("build").join(identity.fingerprint())
}

pub fn run_cache_dir(cache_base: &std::path::Path, identity: &GeneratedRustIdentity) -> PathBuf {
    cache_base.join("run").join(identity.fingerprint())
}

pub fn gors_cache_base() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("gors"));
    }
    if let Some(path) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".cache").join("gors"));
    }
    Ok(std::env::temp_dir().join("gors-cache"))
}
