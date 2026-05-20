#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const GO_VERSION: &str = "1.24.3";

fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        arch => panic!("unsupported arch for Go SDK download: {arch}"),
    }
}

fn go_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        os => panic!("unsupported OS for Go SDK download: {os}"),
    }
}

fn download_url() -> String {
    format!(
        "https://dl.google.com/go/go{GO_VERSION}.{}-{}.tar.gz",
        go_os(),
        go_arch()
    )
}

fn checksum_url() -> String {
    format!(
        "https://dl.google.com/go/go{GO_VERSION}.{}-{}.tar.gz.sha256",
        go_os(),
        go_arch()
    )
}

fn download_bytes(url: &str) -> Vec<u8> {
    let response =
        reqwest::blocking::get(url).unwrap_or_else(|e| panic!("failed to download {url}: {e}"));
    if !response.status().is_success() {
        panic!("HTTP {} for {url}", response.status());
    }
    response
        .bytes()
        .unwrap_or_else(|e| panic!("failed to read response body from {url}: {e}"))
        .to_vec()
}

fn verify_checksum(data: &[u8], expected_hex: &str) {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let actual_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        actual_hex, expected_hex,
        "checksum mismatch: expected {expected_hex}, got {actual_hex}"
    );
}

fn cache_dir() -> PathBuf {
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("cannot determine home directory")
                .join(".cargo")
        });
    cargo_home.join("gors-cache")
}

fn should_include_file(path: &str) -> bool {
    let rel = path.strip_prefix("go/src/").unwrap_or(path);

    if !rel.ends_with(".go") {
        return false;
    }

    if rel.ends_with("_test.go") {
        return false;
    }

    let parts: Vec<&str> = rel.split('/').collect();
    for part in &parts {
        if *part == "testdata" || *part == "vendor" || *part == "cmd" {
            return false;
        }
        if part.starts_with('.') {
            return false;
        }
    }

    true
}

fn extract_stdlib_from_sdk(sdk_path: &Path) -> BTreeMap<String, Vec<(String, String)>> {
    let src_dir = sdk_path.join("src");
    let mut packages: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    fn walk(dir: &Path, base: &Path, packages: &mut BTreeMap<String, Vec<(String, String)>>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "testdata"
                    || name_str == "vendor"
                    || name_str == "cmd"
                    || name_str == "internal"
                    || name_str.starts_with('.')
                {
                    continue;
                }
                walk(&path, base, packages);
            } else if path.extension().is_some_and(|e| e == "go") {
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.ends_with("_test.go") {
                    continue;
                }
                let rel_dir = path
                    .parent()
                    .and_then(|p| p.strip_prefix(base).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if rel_dir.is_empty() {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    packages
                        .entry(rel_dir)
                        .or_default()
                        .push((filename, content));
                }
            }
        }
    }

    walk(&src_dir, &src_dir, &mut packages);
    packages
}

fn extract_stdlib_from_tarball(tarball: &[u8]) -> BTreeMap<String, Vec<(String, String)>> {
    let decoder = GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    let mut packages: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for entry in archive.entries().expect("failed to read tar entries") {
        let mut entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = match entry.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        if !path.starts_with("go/src/") || !should_include_file(&path) {
            continue;
        }

        let rel = path.strip_prefix("go/src/").unwrap_or(&path);
        let (pkg_path, filename) = match rel.rsplit_once('/') {
            Some((dir, file)) => (dir.to_string(), file.to_string()),
            None => continue,
        };

        if pkg_path.contains("/internal") || pkg_path.starts_with("internal") {
            continue;
        }

        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            packages
                .entry(pkg_path)
                .or_default()
                .push((filename, content));
        }
    }

    packages
}

fn create_stdlib_archive(packages: &BTreeMap<String, Vec<(String, String)>>, output_path: &Path) {
    let file = std::fs::File::create(output_path)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", output_path.display()));
    let encoder = GzEncoder::new(file, Compression::best());
    let mut builder = tar::Builder::new(encoder);

    for (pkg_path, files) in packages {
        for (filename, content) in files {
            let entry_path = format!("{pkg_path}/{filename}");
            let data = content.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, &entry_path, data)
                .unwrap_or_else(|e| panic!("failed to add {entry_path} to archive: {e}"));
        }
    }

    builder
        .finish()
        .unwrap_or_else(|e| panic!("failed to finish archive: {e}"));
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GORS_GO_SDK_PATH");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let archive_path = out_dir.join("go_stdlib.tar.gz");

    if archive_path.exists() {
        return;
    }

    let packages = if let Ok(sdk_path) = std::env::var("GORS_GO_SDK_PATH") {
        eprintln!("Using Go SDK from GORS_GO_SDK_PATH={sdk_path}");
        extract_stdlib_from_sdk(Path::new(&sdk_path))
    } else {
        let cache = cache_dir();
        let cached_tarball = cache.join(format!("go{GO_VERSION}.tar.gz"));

        let tarball_data = if cached_tarball.exists() {
            eprintln!("Using cached Go SDK tarball: {}", cached_tarball.display());
            std::fs::read(&cached_tarball)
                .unwrap_or_else(|e| panic!("failed to read cached tarball: {e}"))
        } else {
            eprintln!("Downloading Go {GO_VERSION} SDK...");
            let url = download_url();
            let data = download_bytes(&url);

            eprintln!("Verifying checksum...");
            let checksum_bytes = download_bytes(&checksum_url());
            let expected = String::from_utf8_lossy(&checksum_bytes).trim().to_string();
            verify_checksum(&data, &expected);

            std::fs::create_dir_all(&cache)
                .unwrap_or_else(|e| panic!("failed to create cache dir: {e}"));
            std::fs::write(&cached_tarball, &data)
                .unwrap_or_else(|e| panic!("failed to cache tarball: {e}"));
            eprintln!("Cached Go SDK tarball at {}", cached_tarball.display());
            data
        };

        extract_stdlib_from_tarball(&tarball_data)
    };

    eprintln!(
        "Embedding {} Go stdlib packages ({} total files)",
        packages.len(),
        packages.values().map(|v| v.len()).sum::<usize>()
    );

    create_stdlib_archive(&packages, &archive_path);
    eprintln!("Created stdlib archive at {}", archive_path.display());
}
