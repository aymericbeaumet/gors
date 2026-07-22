use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[path = "build/platform.rs"]
mod platform;
#[path = "build/sdk_index.rs"]
mod sdk_index;

use sdk_index::StdlibPackages;

const GO_VERSION_FILE: &str = "../.go-version";
const STDLIB_PRELOAD_SCHEMA_SUFFIX: &str = "stdlib-source-metadata-v4";
const COMPILER_FINGERPRINT_DOMAIN: &[u8] = b"gors-compiler-artifact-v1\0";

type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;

fn build_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

fn read_go_version() -> BuildResult<String> {
    let raw = std::fs::read_to_string(GO_VERSION_FILE)?;
    let version = raw.trim();
    if !is_go_version(version) {
        return Err(build_error(format!(
            "{GO_VERSION_FILE} must contain a Go version like 1.24.3"
        ))
        .into());
    }
    Ok(version.to_string())
}

fn compiler_source_fingerprint(
    sdk_fingerprint: &str,
    target_goos: &str,
    target_goarch: &str,
) -> BuildResult<String> {
    fn collect_rust_sources(root: &Path, files: &mut Vec<PathBuf>) -> BuildResult<()> {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_rust_sources(&path, files)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect_rust_sources(Path::new("src"), &mut files)?;
    collect_rust_sources(Path::new("../gors-runtime/src"), &mut files)?;
    files.extend(
        [
            "build.rs",
            "build/platform.rs",
            "build/sdk_index.rs",
            "Cargo.toml",
            "../Cargo.toml",
            "../Cargo.lock",
            GO_VERSION_FILE,
            "../gors-runtime/Cargo.toml",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    files.sort();

    let mut hasher = Sha256::new();
    hasher.update(COMPILER_FINGERPRINT_DOMAIN);
    hasher.update(b"embedded-go-sdk\0");
    hasher.update(sdk_fingerprint.as_bytes());
    hasher.update(b"\0target-goos\0");
    hasher.update(target_goos.as_bytes());
    hasher.update(b"\0target-goarch\0");
    hasher.update(target_goarch.as_bytes());
    hasher.update(b"\0");
    for key in ["TARGET", "PROFILE", "CARGO_PKG_VERSION"] {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(std::env::var(key)?.as_bytes());
        hasher.update(b"\0");
    }
    let mut enabled_features = std::env::vars()
        .filter_map(|(key, value)| key.starts_with("CARGO_FEATURE_").then_some((key, value)))
        .collect::<Vec<_>>();
    enabled_features.sort();
    for (key, value) in enabled_features {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    for path in files {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(std::fs::read(&path)?);
        hasher.update(b"\0");
    }
    if std::env::var("TARGET")?.starts_with("wasm32-") {
        for path in ["../www/wasm/Cargo.toml", "../www/wasm/Cargo.lock"] {
            hasher.update(path.as_bytes());
            hasher.update(b"\0");
            hasher.update(std::fs::read(path)?);
            hasher.update(b"\0");
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn stdlib_source_fingerprint(
    packages: &StdlibPackages,
    go_version: &str,
    target_goos: &str,
    target_goarch: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gors-embedded-go-sdk-v2\0");
    hasher.update(go_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(target_goos.as_bytes());
    hasher.update(b"\0");
    hasher.update(target_goarch.as_bytes());
    hasher.update(b"\0");
    for (import_path, package) in packages {
        hasher.update(import_path.as_bytes());
        hasher.update(b"\0");
        for file in &package.files {
            hasher.update(file.filename.as_bytes());
            hasher.update(b"\0");
            hasher.update(file.content.as_bytes());
            hasher.update(b"\0");
        }
        for dependency in &package.direct_imports {
            hasher.update(dependency.as_bytes());
            hasher.update(b"\0");
        }
        for pattern in &package.embed_patterns {
            hasher.update(pattern.as_bytes());
            hasher.update(b"\0");
        }
        for asset in &package.embed_files {
            hasher.update(asset.path.as_bytes());
            hasher.update(b"\0");
            hasher.update(asset.sha256.as_bytes());
            hasher.update(b"\0");
            hasher.update(&asset.content);
            hasher.update(b"\0");
        }
        for input in &package.unsupported_inputs {
            hasher.update(input.kind.rust_variant().as_bytes());
            hasher.update(b"\0");
            hasher.update(input.file.path.as_bytes());
            hasher.update(b"\0");
            hasher.update(input.file.sha256.as_bytes());
            hasher.update(b"\0");
            hasher.update(&input.file.content);
            hasher.update(b"\0");
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_go_version(version: &str) -> bool {
    let mut count = 0;
    for part in version.split('.') {
        count += 1;
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
    }
    count == 3
}

fn stdlib_version(go_version: &str) -> String {
    format!("gostdlib{go_version}")
}

fn stdlib_preload_schema(
    go_version: &str,
    target_goos: &str,
    target_goarch: &str,
    sdk_fingerprint: &str,
) -> String {
    format!(
        "{}-{target_goos}-gors-defs-{target_goarch}-{STDLIB_PRELOAD_SCHEMA_SUFFIX}-{sdk_fingerprint}",
        stdlib_version(go_version),
    )
}

fn host_sdk_platform() -> BuildResult<platform::GoPlatform> {
    platform::host_sdk_platform(std::env::consts::OS, std::env::consts::ARCH)
        .map_err(|message| build_error(message).into())
}

fn target_source_platform() -> BuildResult<platform::GoPlatform> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    platform::target_source_platform(&target_os, &target_arch)
        .map_err(|message| build_error(message).into())
}

fn download_url(go_version: &str) -> BuildResult<String> {
    let host = host_sdk_platform()?;
    Ok(format!(
        "https://dl.google.com/go/go{go_version}.{}-{}.tar.gz",
        host.os, host.arch
    ))
}

fn checksum_url(go_version: &str) -> BuildResult<String> {
    let host = host_sdk_platform()?;
    Ok(format!(
        "https://dl.google.com/go/go{go_version}.{}-{}.tar.gz.sha256",
        host.os, host.arch
    ))
}

fn download_bytes(url: &str) -> BuildResult<Vec<u8>> {
    let response = reqwest::blocking::get(url)?;
    if !response.status().is_success() {
        return Err(build_error(format!("HTTP {} for {url}", response.status())).into());
    }
    Ok(response.bytes()?.to_vec())
}

fn verify_checksum(data: &[u8], expected_hex: &str) -> BuildResult<()> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let actual_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    if actual_hex != expected_hex {
        return Err(build_error(format!(
            "checksum mismatch: expected {expected_hex}, got {actual_hex}"
        ))
        .into());
    }
    Ok(())
}

fn cache_dir() -> BuildResult<PathBuf> {
    let cargo_home = match std::env::var("CARGO_HOME") {
        Ok(path) => PathBuf::from(path),
        Err(_) => dirs::home_dir()
            .ok_or_else(|| build_error("cannot determine home directory"))?
            .join(".cargo"),
    };
    Ok(cargo_home.join("gors-cache"))
}

fn go_sdk_cache_key(go_version: &str) -> BuildResult<String> {
    let host = host_sdk_platform()?;
    Ok(format!("go{go_version}.{}-{}", host.os, host.arch))
}

fn ensure_go_sdk(go_version: &str) -> BuildResult<PathBuf> {
    if let Ok(sdk_path) = std::env::var("GORS_GO_SDK_PATH") {
        eprintln!("Using Go SDK from GORS_GO_SDK_PATH={sdk_path}");
        let sdk_path = PathBuf::from(sdk_path);
        validate_sdk_version(&sdk_path, go_version)?;
        return Ok(sdk_path);
    }

    let cache = cache_dir()?;
    let key = go_sdk_cache_key(go_version)?;
    let sdk_root = cache.join(&key);
    let sdk_path = sdk_root.join("go");
    if validate_sdk_version(&sdk_path, go_version).is_ok() {
        return Ok(sdk_path);
    }

    let tarball_path = ensure_go_tarball(&cache, go_version)?;
    let tmp_root = cache.join(format!("{key}.tmp-{}", std::process::id()));
    if tmp_root.exists() {
        std::fs::remove_dir_all(&tmp_root)?;
    }
    std::fs::create_dir_all(&tmp_root)?;

    let tarball = std::fs::File::open(&tarball_path)?;
    let decoder = GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(&tmp_root)?;
    validate_sdk_version(&tmp_root.join("go"), go_version)?;

    if sdk_root.exists() {
        std::fs::remove_dir_all(&sdk_root)?;
    }
    std::fs::rename(&tmp_root, &sdk_root)?;
    Ok(sdk_path)
}

fn ensure_go_tarball(cache: &Path, go_version: &str) -> BuildResult<PathBuf> {
    std::fs::create_dir_all(cache)?;
    let key = go_sdk_cache_key(go_version)?;
    let cached_tarball = cache.join(format!("{key}.tar.gz"));
    if cached_tarball.exists() {
        eprintln!("Using cached Go SDK tarball: {}", cached_tarball.display());
        return Ok(cached_tarball);
    }

    eprintln!("Downloading Go {go_version} SDK...");
    let url = download_url(go_version)?;
    let data = download_bytes(&url)?;

    eprintln!("Verifying checksum...");
    let checksum_bytes = download_bytes(&checksum_url(go_version)?)?;
    let expected = String::from_utf8_lossy(&checksum_bytes).trim().to_string();
    verify_checksum(&data, &expected)?;

    std::fs::write(&cached_tarball, &data)?;
    eprintln!("Cached Go SDK tarball at {}", cached_tarball.display());
    Ok(cached_tarball)
}

fn validate_sdk_version(sdk_path: &Path, go_version: &str) -> BuildResult<()> {
    let expected = format!("go{go_version}");
    let version_path = sdk_path.join("VERSION");
    let raw = std::fs::read_to_string(&version_path)?;
    let actual = raw.lines().next().unwrap_or_default().trim();
    if actual != expected {
        return Err(build_error(format!(
            "{} is {actual}, expected {expected}",
            version_path.display()
        ))
        .into());
    }
    Ok(())
}

fn create_stdlib_preload(
    packages: &StdlibPackages,
    output_path: &Path,
    source_dir: &Path,
) -> BuildResult<()> {
    if source_dir.exists() {
        std::fs::remove_dir_all(source_dir)?;
    }
    std::fs::create_dir_all(source_dir)?;

    let mut metadata = String::new();
    metadata.push_str("static EMBEDDED_PACKAGES: &[EmbeddedGoPackage] = &[\n");

    for (pkg_path, package) in packages {
        let pkg_dir = source_dir.join(pkg_path);
        std::fs::create_dir_all(&pkg_dir)?;

        metadata.push_str("    EmbeddedGoPackage {\n");
        metadata.push_str("        import_path: ");
        metadata.push_str(&rust_string(pkg_path));
        metadata.push_str(",\n        files: &[\n");

        for file in &package.files {
            materialize_package_file(&pkg_dir, &file.filename, file.content.as_bytes())?;
            metadata.push_str("            EmbeddedGoFile { filename: ");
            metadata.push_str(&rust_string(&file.filename));
            metadata.push_str(", content: include_str!(concat!(env!(\"OUT_DIR\"), ");
            metadata.push_str(&rust_string(&format!(
                "/go_stdlib_src/{pkg_path}/{}",
                file.filename
            )));
            metadata.push_str(")) },\n");
        }
        metadata.push_str("        ],\n        direct_imports: &[\n");
        for import_path in &package.direct_imports {
            metadata.push_str("            ");
            metadata.push_str(&rust_string(import_path));
            metadata.push_str(",\n");
        }
        metadata.push_str("        ],\n        embed_patterns: &[\n");
        for pattern in &package.embed_patterns {
            metadata.push_str("            ");
            metadata.push_str(&rust_string(pattern));
            metadata.push_str(",\n");
        }
        metadata.push_str("        ],\n        embed_files: &[\n");
        for asset in &package.embed_files {
            materialize_package_file(&pkg_dir, &asset.path, &asset.content)?;
            metadata.push_str("            EmbeddedGoAsset { path: ");
            metadata.push_str(&rust_string(&asset.path));
            metadata.push_str(", content: include_bytes!(concat!(env!(\"OUT_DIR\"), ");
            metadata.push_str(&rust_string(&format!(
                "/go_stdlib_src/{pkg_path}/{}",
                asset.path
            )));
            metadata.push_str(")), sha256: ");
            metadata.push_str(&rust_string(&asset.sha256));
            metadata.push_str(" },\n");
        }
        metadata.push_str("        ],\n        unsupported_inputs: &[\n");
        for input in &package.unsupported_inputs {
            materialize_package_file(&pkg_dir, &input.file.path, &input.file.content)?;
            metadata.push_str("            UnsupportedGoInput { kind: UnsupportedGoInputKind::");
            metadata.push_str(input.kind.rust_variant());
            metadata.push_str(", path: ");
            metadata.push_str(&rust_string(&input.file.path));
            metadata.push_str(", content: include_bytes!(concat!(env!(\"OUT_DIR\"), ");
            metadata.push_str(&rust_string(&format!(
                "/go_stdlib_src/{pkg_path}/{}",
                input.file.path
            )));
            metadata.push_str(")), sha256: ");
            metadata.push_str(&rust_string(&input.file.sha256));
            metadata.push_str(" },\n");
        }
        metadata.push_str("        ],\n    },\n");
    }
    metadata.push_str("];\n");

    std::fs::write(output_path, metadata)?;
    Ok(())
}

fn materialize_package_file(
    package_dir: &Path,
    relative_path: &str,
    content: &[u8],
) -> BuildResult<()> {
    let path = package_dir.join(relative_path);
    let parent = path.parent().ok_or_else(|| {
        build_error(format!(
            "generated SDK metadata path has no parent: {}",
            path.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;
    if path.exists() {
        let existing = std::fs::read(&path)?;
        if existing != content {
            return Err(build_error(format!(
                "conflicting build-selected SDK inputs map to {}",
                path.display()
            ))
            .into());
        }
        return Ok(());
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn main() -> BuildResult<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/platform.rs");
    println!("cargo:rerun-if-changed=build/sdk_index.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");
    println!("cargo:rerun-if-changed=../Cargo.lock");
    println!("cargo:rerun-if-changed=../gors-runtime/Cargo.toml");
    println!("cargo:rerun-if-changed={GO_VERSION_FILE}");
    println!("cargo:rerun-if-changed=../gors-runtime/src");
    println!("cargo:rerun-if-env-changed=GORS_GO_SDK_PATH");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    if std::env::var("TARGET")?.starts_with("wasm32-") {
        println!("cargo:rerun-if-changed=../www/wasm/Cargo.toml");
        println!("cargo:rerun-if-changed=../www/wasm/Cargo.lock");
    }

    let go_version = read_go_version()?;
    let stdlib_version = stdlib_version(&go_version);
    let sdk_path = ensure_go_sdk(&go_version)?;
    if std::env::var_os("GORS_GO_SDK_PATH").is_some() {
        println!(
            "cargo:rerun-if-changed={}",
            sdk_path.join("VERSION").display()
        );
        println!("cargo:rerun-if-changed={}", sdk_path.join("src").display());
    }
    let target = target_source_platform()?;
    let target_goos = target.os;
    let target_goarch = target.arch;
    let oracle_cache = cache_dir()?.join("go-source-oracle");
    let packages =
        sdk_index::load_stdlib_from_sdk(&sdk_path, &oracle_cache, target_goos, target_goarch)?;
    let sdk_fingerprint =
        stdlib_source_fingerprint(&packages, &go_version, target_goos, target_goarch);
    let compiler_fingerprint =
        compiler_source_fingerprint(&sdk_fingerprint, target_goos, target_goarch)?;
    println!("cargo:rustc-env=GORS_GO_VERSION={go_version}");
    println!("cargo:rustc-env=GORS_STDLIB_VERSION={stdlib_version}");
    println!("cargo:rustc-env=GORS_COMPILER_FINGERPRINT={compiler_fingerprint}");
    println!(
        "cargo:rustc-env=GORS_BUILT_GO_SDK_PATH={}",
        sdk_path.display()
    );

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let preload_path = out_dir.join("go_stdlib.rs");
    let source_dir = out_dir.join("go_stdlib_src");
    let marker_path = out_dir.join("go_stdlib.version");
    let preload_schema =
        stdlib_preload_schema(&go_version, target_goos, target_goarch, &sdk_fingerprint);

    if preload_path.exists()
        && source_dir.exists()
        && std::fs::read_to_string(&marker_path).is_ok_and(|s| s == preload_schema)
    {
        return Ok(());
    }

    eprintln!(
        "Preloading {} build-selected Go stdlib packages for target GOOS={target_goos} GOARCH={target_goarch} with gors overrides ({} Go files, {} embed assets, {} classified host inputs)",
        packages.len(),
        packages
            .values()
            .map(|package| package.files.len())
            .sum::<usize>(),
        packages
            .values()
            .map(|package| package.embed_files.len())
            .sum::<usize>(),
        packages
            .values()
            .map(|package| package.unsupported_inputs.len())
            .sum::<usize>()
    );

    create_stdlib_preload(&packages, &preload_path, &source_dir)?;
    std::fs::write(&marker_path, preload_schema)?;
    eprintln!("Created stdlib preload at {}", preload_path.display());
    Ok(())
}
