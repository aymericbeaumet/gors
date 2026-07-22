use std::path::PathBuf;

pub fn build_cache_dir(source_path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(source_path.as_bytes());
    if let Ok(canonical) = std::fs::canonicalize(source_path) {
        hasher.update(b"\0");
        hasher.update(canonical.to_string_lossy().as_bytes());
    }

    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("build").join(key))
}

pub fn run_cache_dir(
    source_paths: &[String],
    release: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(if release {
        b"release".as_slice()
    } else {
        b"debug".as_slice()
    });
    hasher.update(b"\0");
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    for path in source_paths {
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        if let Ok(canonical) = std::fs::canonicalize(path) {
            hasher.update(b"\0");
            hasher.update(canonical.to_string_lossy().as_bytes());
        }
    }
    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("run").join(key))
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
