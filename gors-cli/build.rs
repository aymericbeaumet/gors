use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> BuildResult<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| std::io::Error::other("Cargo did not provide CARGO_MANIFEST_DIR"))?,
    );
    let workspace_dir = manifest_dir.parent().ok_or_else(|| {
        std::io::Error::other("gors-cli manifest directory has no workspace parent")
    })?;

    let fingerprint = cli_abi_fingerprint(&manifest_dir, workspace_dir)?;
    println!("cargo:rustc-env=GORS_CLI_ABI_FINGERPRINT={fingerprint}");
    Ok(())
}

fn cli_abi_fingerprint(manifest_dir: &Path, workspace_dir: &Path) -> BuildResult<String> {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, b"gors-cli-abi-v1");

    for key in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_PKG_VERSION",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        hash_part(&mut hasher, key.as_bytes());
        hash_os_part(
            &mut hasher,
            std::env::var_os(key)
                .as_deref()
                .unwrap_or_else(|| OsStr::new("")),
        );
    }

    let mut features = std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("CARGO_FEATURE_"))
        .collect::<Vec<_>>();
    features.sort_by(|left, right| left.0.cmp(&right.0));
    for (key, value) in features {
        hash_os_part(&mut hasher, &key);
        hash_os_part(&mut hasher, &value);
    }

    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsStr::new("rustc").to_owned());
    println!("cargo:rerun-if-env-changed=RUSTC");
    let rustc_version = Command::new(&rustc).arg("-vV").output()?;
    if !rustc_version.status.success() {
        return Err(std::io::Error::other(format!(
            "{} -vV failed with {}",
            Path::new(&rustc).display(),
            rustc_version.status
        ))
        .into());
    }
    hash_part(&mut hasher, b"rustc-vV");
    hash_part(&mut hasher, &rustc_version.stdout);
    hash_part(&mut hasher, &rustc_version.stderr);

    let source_dir = manifest_dir.join("src");
    println!("cargo:rerun-if-changed={}", source_dir.display());
    let mut source_files = Vec::new();
    collect_files(&source_dir, &mut source_files)?;
    source_files.sort();
    for path in source_files {
        hash_file(&mut hasher, &path, manifest_dir)?;
    }

    for path in [
        manifest_dir.join("Cargo.toml"),
        manifest_dir.join("build.rs"),
        workspace_dir.join("Cargo.toml"),
        workspace_dir.join("Cargo.lock"),
        workspace_dir.join("rust-toolchain.toml"),
        workspace_dir.join(".cargo").join("config.toml"),
    ] {
        if path.is_file() {
            hash_file(&mut hasher, &path, workspace_dir)?;
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
            hash_part(&mut hasher, b"missing");
            hash_os_part(
                &mut hasher,
                path.strip_prefix(workspace_dir)
                    .unwrap_or(&path)
                    .as_os_str(),
            );
        }
    }

    Ok(hex_digest(hasher.finalize()))
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> BuildResult<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn hash_file(hasher: &mut Sha256, path: &Path, relative_to: &Path) -> BuildResult<()> {
    println!("cargo:rerun-if-changed={}", path.display());
    let relative_path = path.strip_prefix(relative_to).unwrap_or(path);
    hash_os_part(hasher, relative_path.as_os_str());
    hash_part(hasher, &std::fs::read(path)?);
    Ok(())
}

#[cfg(unix)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::unix::ffi::OsStrExt as _;
    hash_part(hasher, part.as_bytes());
}

#[cfg(windows)]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    use std::os::windows::ffi::OsStrExt as _;
    let bytes = part
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    hash_part(hasher, &bytes);
}

#[cfg(not(any(unix, windows)))]
fn hash_os_part(hasher: &mut Sha256, part: &OsStr) {
    hash_part(hasher, part.to_string_lossy().as_bytes());
}

fn hash_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(part);
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
