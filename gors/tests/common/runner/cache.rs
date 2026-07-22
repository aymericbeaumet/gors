use super::program_name;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

pub(super) const RUST_TOOLCHAIN: &str = "1.96.0";
pub(super) const RUST_EDITION: &str = "2024";
const INTEGRATION_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const INTEGRATION_CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const INCOMPLETE_CACHE_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60);

pub(super) fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    for (filename, source) in &output.files {
        let path = output_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn cached_fixture_output_dir(
    fixture_root: &Path,
    fixture_dir: &Path,
) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"gors-integration-fixture-v2");
    hasher.update(b"\0");
    hasher.update(program_name(fixture_root, fixture_dir).as_bytes());
    hasher.update(b"\0");
    hasher.update(test_binary_fingerprint().as_bytes());
    hasher.update(b"\0");
    hasher.update(rustc_fingerprint().as_bytes());
    hasher.update(b"\0");
    hasher.update(gors::STDLIB_VERSION.as_bytes());
    hasher.update(
        format!(
            "\0rustc-toolchain:{RUST_TOOLCHAIN},rustc-flags:edition{RUST_EDITION},deny-unused,overflow-checks-off"
        )
        .as_bytes(),
    );
    hasher.update(b"\0");
    let mut input_files = Vec::new();
    collect_regular_files_recursive(fixture_dir, &mut input_files)?;
    input_files.sort();
    for path in input_files {
        let relative = path
            .strip_prefix(fixture_dir)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?);
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    let hash = hex_hash(&digest);
    let dir = integration_cache_root().join(hash);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn integration_cache_root() -> PathBuf {
    workspace_root()
        .join("target")
        .join("gors-integration-run")
        .join("v2")
}

pub(super) fn prune_integration_cache() -> Result<(), String> {
    let root = integration_cache_root();
    if !root.exists() {
        return Ok(());
    }
    let now = SystemTime::now();
    let mut successful = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| format!("{}: {error}", root.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", root.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let marker = path.join(".rustc-ok");
        if !marker.exists() {
            let age = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or(Duration::ZERO);
            if age >= INCOMPLETE_CACHE_GRACE_PERIOD {
                let _ = fs::remove_dir_all(path);
            }
            continue;
        }
        let modified = marker
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age >= INTEGRATION_CACHE_MAX_AGE {
            let _ = fs::remove_dir_all(path);
            continue;
        }
        successful.push((modified, directory_size(&path)?, path));
    }

    successful.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let mut retained_bytes = 0_u64;
    for (_, size, path) in successful {
        if retained_bytes.saturating_add(size) <= INTEGRATION_CACHE_MAX_BYTES {
            retained_bytes = retained_bytes.saturating_add(size);
        } else {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(())
}

fn directory_size(dir: &Path) -> Result<u64, String> {
    let mut size = 0_u64;
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            size = size.saturating_add(directory_size(&path)?);
        } else {
            size = size.saturating_add(
                entry
                    .metadata()
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .len(),
            );
        }
    }
    Ok(size)
}

fn collect_regular_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files_recursive(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn test_binary_fingerprint() -> &'static str {
    static TEST_BINARY_FINGERPRINT: OnceLock<String> = OnceLock::new();
    TEST_BINARY_FINGERPRINT.get_or_init(|| {
        std::env::current_exe()
            .and_then(fs::read)
            .map(|binary| {
                let mut hasher = Sha256::new();
                hasher.update(binary);
                hex_hash(&hasher.finalize())
            })
            .unwrap_or_else(|error| format!("test-binary-fingerprint-error:{error}"))
    })
}

fn rustc_fingerprint() -> &'static str {
    static RUSTC_FINGERPRINT: OnceLock<String> = OnceLock::new();
    RUSTC_FINGERPRINT.get_or_init(|| {
        Command::new("rustup")
            .args(["run", RUST_TOOLCHAIN, "rustc", "-vV"])
            .output()
            .map(|output| {
                let mut hasher = Sha256::new();
                hasher.update(&output.stdout);
                hasher.update(&output.stderr);
                hasher.update([u8::from(output.status.success())]);
                let digest = hasher.finalize();
                hex_hash(&digest)
            })
            .unwrap_or_else(|error| format!("rustc-fingerprint-error:{error}"))
    })
}

fn hex_hash(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gors crate should live under workspace root")
        .to_path_buf()
}
