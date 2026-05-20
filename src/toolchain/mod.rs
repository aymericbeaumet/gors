use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const DEFAULT_GO_VERSION: &str = "1.24.3";

#[derive(Debug)]
pub struct GoToolchain {
    root: PathBuf,
    version: String,
}

#[derive(Debug)]
pub enum ToolchainError {
    Io(std::io::Error),
    Download(String),
    Checksum { expected: String, actual: String },
    UnsupportedPlatform(String),
    NoDataDir,
}

impl std::fmt::Display for ToolchainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Download(msg) => write!(f, "download error: {msg}"),
            Self::Checksum { expected, actual } => {
                write!(f, "checksum mismatch: expected {expected}, got {actual}")
            }
            Self::UnsupportedPlatform(p) => write!(f, "unsupported platform: {p}"),
            Self::NoDataDir => write!(f, "cannot determine data directory"),
        }
    }
}

impl std::error::Error for ToolchainError {}

impl From<std::io::Error> for ToolchainError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

fn data_dir() -> Result<PathBuf, ToolchainError> {
    dirs::data_dir()
        .map(|d| d.join("gors"))
        .ok_or(ToolchainError::NoDataDir)
}

fn toolchain_dir(version: &str) -> Result<PathBuf, ToolchainError> {
    Ok(data_dir()?.join("toolchains").join(format!("go{version}")))
}

fn go_arch() -> Result<&'static str, ToolchainError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        arch => Err(ToolchainError::UnsupportedPlatform(format!(
            "unsupported arch: {arch}"
        ))),
    }
}

fn go_os() -> Result<&'static str, ToolchainError> {
    match std::env::consts::OS {
        "macos" => Ok("darwin"),
        "linux" => Ok("linux"),
        os => Err(ToolchainError::UnsupportedPlatform(format!(
            "unsupported OS: {os}"
        ))),
    }
}

fn download_url(version: &str) -> Result<String, ToolchainError> {
    let os = go_os()?;
    let arch = go_arch()?;
    Ok(format!(
        "https://dl.google.com/go/go{version}.{os}-{arch}.tar.gz"
    ))
}

fn checksum_url(version: &str) -> Result<String, ToolchainError> {
    let os = go_os()?;
    let arch = go_arch()?;
    Ok(format!(
        "https://dl.google.com/go/go{version}.{os}-{arch}.tar.gz.sha256"
    ))
}

fn download_bytes(url: &str) -> Result<Vec<u8>, ToolchainError> {
    let response = reqwest::blocking::get(url)
        .map_err(|e| ToolchainError::Download(format!("HTTP request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(ToolchainError::Download(format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }

    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| ToolchainError::Download(format!("failed to read response body: {e}")))
}

fn verify_checksum(data: &[u8], expected_hex: &str) -> Result<(), ToolchainError> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual_hex = format!("{:x}", hasher.finalize());
    if actual_hex != expected_hex {
        return Err(ToolchainError::Checksum {
            expected: expected_hex.to_string(),
            actual: actual_hex,
        });
    }
    Ok(())
}

fn extract_tar_gz(data: &[u8], dest: &Path) -> Result<(), ToolchainError> {
    let decoder = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

fn install(version: &str) -> Result<GoToolchain, ToolchainError> {
    let dest = toolchain_dir(version)?;
    let tmp_dest = dest.with_file_name(format!("go{version}.{}", std::process::id()));

    eprintln!("Downloading Go {version}...");
    let url = download_url(version)?;
    let tarball = download_bytes(&url)?;

    eprintln!("Verifying checksum...");
    let checksum_url = checksum_url(version)?;
    let checksum_bytes = download_bytes(&checksum_url)?;
    let expected_checksum = String::from_utf8_lossy(&checksum_bytes).trim().to_string();
    verify_checksum(&tarball, &expected_checksum)?;

    eprintln!("Extracting to {}...", dest.display());
    let _ = std::fs::remove_dir_all(&tmp_dest);
    std::fs::create_dir_all(&tmp_dest)?;
    extract_tar_gz(&tarball, &tmp_dest)?;

    // Atomically move into place; if another process already installed this
    // version the rename will fail and we reuse the existing installation.
    match std::fs::rename(&tmp_dest, &dest) {
        Ok(()) => {}
        Err(_) if dest.join("go").join("src").is_dir() => {
            let _ = std::fs::remove_dir_all(&tmp_dest);
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_dest);
            return Err(ToolchainError::Io(e));
        }
    }

    eprintln!("Go {version} installed successfully.");
    Ok(GoToolchain {
        root: dest.join("go"),
        version: version.to_string(),
    })
}

pub fn ensure() -> Result<GoToolchain, ToolchainError> {
    ensure_version(DEFAULT_GO_VERSION)
}

pub fn ensure_version(version: &str) -> Result<GoToolchain, ToolchainError> {
    let dest = toolchain_dir(version)?;
    let root = dest.join("go");

    if root.join("src").is_dir() {
        return Ok(GoToolchain {
            root,
            version: version.to_string(),
        });
    }

    install(version)
}

impl GoToolchain {
    pub fn goroot(&self) -> &Path {
        &self.root
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn stdlib_src(&self, pkg_path: &str) -> PathBuf {
        self.root.join("src").join(pkg_path)
    }

    pub fn stdlib_package_exists(&self, pkg_path: &str) -> bool {
        let src_dir = self.stdlib_src(pkg_path);
        if !src_dir.is_dir() {
            return false;
        }
        // Check that it contains at least one .go file
        std::fs::read_dir(&src_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().extension().is_some_and(|ext| ext == "go"))
            })
            .unwrap_or(false)
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_data_dir() {
        let dir = data_dir().unwrap();
        assert!(dir.ends_with("gors"));
    }

    #[test]
    fn test_toolchain_dir() {
        let dir = toolchain_dir("1.22.0").unwrap();
        assert!(dir.to_string_lossy().contains("go1.22.0"));
    }

    #[test]
    fn test_download_url() {
        let url = download_url("1.22.0").unwrap();
        assert!(url.starts_with("https://dl.google.com/go/go1.22.0."));
        assert!(url.ends_with(".tar.gz"));
    }

    #[test]
    fn test_verify_checksum() {
        let data = b"hello world";
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        verify_checksum(data, expected).unwrap();
    }

    #[test]
    fn test_verify_checksum_mismatch() {
        let data = b"hello world";
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_checksum(data, wrong).is_err());
    }
}
