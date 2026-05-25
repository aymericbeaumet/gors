use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const GO_VERSION_FILE: &str = "../.go-version";
const STDLIB_PRELOAD_SCHEMA_SUFFIX: &str = "stdlib-static-preload-v1";

type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone)]
struct StdlibSourceFile {
    filename: String,
    content: String,
}

type StdlibPackages = BTreeMap<String, Vec<StdlibSourceFile>>;

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

fn stdlib_preload_schema(go_version: &str, target_goos: &str) -> String {
    format!(
        "{}-{target_goos}-gors-{STDLIB_PRELOAD_SCHEMA_SUFFIX}",
        stdlib_version(go_version),
    )
}

fn go_arch() -> BuildResult<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        arch => Err(build_error(format!("unsupported arch for Go SDK download: {arch}")).into()),
    }
}

fn go_os() -> BuildResult<&'static str> {
    match std::env::consts::OS {
        "macos" => Ok("darwin"),
        "linux" => Ok("linux"),
        os => Err(build_error(format!("unsupported OS for Go SDK download: {os}")).into()),
    }
}

fn download_url(go_version: &str) -> BuildResult<String> {
    Ok(format!(
        "https://dl.google.com/go/go{go_version}.{}-{}.tar.gz",
        go_os()?,
        go_arch()?
    ))
}

fn checksum_url(go_version: &str) -> BuildResult<String> {
    Ok(format!(
        "https://dl.google.com/go/go{go_version}.{}-{}.tar.gz.sha256",
        go_os()?,
        go_arch()?
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
    Ok(format!("go{go_version}.{}-{}", go_os()?, go_arch()?))
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

fn extract_stdlib_from_sdk(sdk_path: &Path, go_version: &str, target_goos: &str) -> StdlibPackages {
    let src_dir = sdk_path.join("src");
    let mut packages = BTreeMap::new();

    fn walk(
        dir: &Path,
        base: &Path,
        go_version: &str,
        target_goos: &str,
        packages: &mut StdlibPackages,
    ) {
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
                    || name_str.starts_with('.')
                {
                    continue;
                }
                walk(&path, base, go_version, target_goos, packages);
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
                    if should_compile_file(&filename, &content, go_version, target_goos) {
                        packages
                            .entry(rel_dir)
                            .or_default()
                            .push(StdlibSourceFile { filename, content });
                    }
                }
            }
        }
    }

    walk(&src_dir, &src_dir, go_version, target_goos, &mut packages);
    for files in packages.values_mut() {
        files.sort_by(|a, b| a.filename.cmp(&b.filename));
    }
    packages
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

    let known_packages: BTreeSet<_> = packages.keys().cloned().collect();
    let direct_imports = package_direct_imports(packages, &known_packages);

    let mut metadata = String::new();
    metadata.push_str("static EMBEDDED_PACKAGES: &[EmbeddedGoPackage] = &[\n");

    for (pkg_path, files) in packages {
        let pkg_dir = source_dir.join(pkg_path);
        std::fs::create_dir_all(&pkg_dir)?;

        metadata.push_str("    EmbeddedGoPackage {\n");
        metadata.push_str("        import_path: ");
        metadata.push_str(&rust_string(pkg_path));
        metadata.push_str(",\n        files: &[\n");

        for file in files {
            let file_path = pkg_dir.join(&file.filename);
            std::fs::write(&file_path, &file.content)?;
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
        if let Some(imports) = direct_imports.get(pkg_path) {
            for import_path in imports {
                metadata.push_str("            ");
                metadata.push_str(&rust_string(import_path));
                metadata.push_str(",\n");
            }
        }
        metadata.push_str("        ],\n    },\n");
    }
    metadata.push_str("];\n");

    std::fs::write(output_path, metadata)?;
    Ok(())
}

fn package_direct_imports(
    packages: &StdlibPackages,
    known_packages: &BTreeSet<String>,
) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for (pkg_path, files) in packages {
        let mut imports = BTreeSet::new();
        for file in files {
            for import_path in import_paths_from_source(&file.content) {
                if import_path != *pkg_path && known_packages.contains(&import_path) {
                    imports.insert(import_path);
                }
            }
        }
        result.insert(pkg_path.clone(), imports.into_iter().collect());
    }
    result
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn import_paths_from_source(content: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_import_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import (") {
            in_import_block = true;
            continue;
        }
        if in_import_block && trimmed == ")" {
            in_import_block = false;
            continue;
        }
        if trimmed.starts_with("import ") || in_import_block {
            if let Some(start) = trimmed.find('"')
                && let Some(end) = trimmed[start + 1..].find('"')
            {
                paths.push(trimmed[start + 1..start + 1 + end].to_string());
            }
        }
    }

    paths
}

fn should_compile_file(filename: &str, content: &str, go_version: &str, target_goos: &str) -> bool {
    file_name_matches_target(filename, target_goos)
        && build_constraint_matches(content, go_version, target_goos)
}

fn file_name_matches_target(filename: &str, target_goos: &str) -> bool {
    let Some(stem) = filename.strip_suffix(".go") else {
        return false;
    };
    let parts: Vec<&str> = stem.split('_').collect();
    let Some(last) = parts.last().copied() else {
        return true;
    };

    if is_go_arch(last) {
        if last != "gors" {
            return false;
        }
        if let Some(os_part) = parts.get(parts.len().saturating_sub(2))
            && is_go_os(os_part)
            && *os_part != target_goos
        {
            return false;
        }
        return true;
    }

    !is_go_os(last) || last == target_goos
}

fn build_constraint_matches(content: &str, go_version: &str, target_goos: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("//go:build ") {
            return BuildExprParser::new(expr, go_version, target_goos).parse();
        }
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        break;
    }
    true
}

struct BuildExprParser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
    go_version: &'a str,
    target_goos: &'a str,
}

impl<'a> BuildExprParser<'a> {
    fn new(expr: &'a str, go_version: &'a str, target_goos: &'a str) -> Self {
        Self {
            tokens: tokenize_build_expr(expr),
            pos: 0,
            go_version,
            target_goos,
        }
    }

    fn parse(&mut self) -> bool {
        self.parse_or()
    }

    fn parse_or(&mut self) -> bool {
        let mut value = self.parse_and();
        while self.peek() == Some("||") {
            self.pos += 1;
            value = self.parse_and() || value;
        }
        value
    }

    fn parse_and(&mut self) -> bool {
        let mut value = self.parse_unary();
        while self.peek() == Some("&&") {
            self.pos += 1;
            value = self.parse_unary() && value;
        }
        value
    }

    fn parse_unary(&mut self) -> bool {
        if self.peek() == Some("!") {
            self.pos += 1;
            return !self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> bool {
        match self.next() {
            Some("(") => {
                let value = self.parse_or();
                if self.peek() == Some(")") {
                    self.pos += 1;
                }
                value
            }
            Some(tag) => build_tag_matches(tag, self.go_version, self.target_goos),
            None => true,
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.pos += 1;
        Some(token)
    }
}

fn tokenize_build_expr(expr: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;

    for (idx, ch) in expr.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }

        if let Some(s) = start.take() {
            tokens.push(&expr[s..idx]);
        }

        match ch {
            '!' | '(' | ')' => tokens.push(&expr[idx..idx + ch.len_utf8()]),
            '&' | '|' => {
                let end = idx + 2;
                if expr.get(idx..end) == Some("&&") || expr.get(idx..end) == Some("||") {
                    tokens.push(&expr[idx..end]);
                }
            }
            _ => {}
        }
    }

    if let Some(s) = start {
        tokens.push(&expr[s..]);
    }

    tokens
}

fn build_tag_matches(tag: &str, go_version: &str, target_goos: &str) -> bool {
    if tag == target_goos || tag == "gors" {
        return true;
    }
    if tag == "unix" {
        return is_unix_goos(target_goos);
    }
    if let Some(version) = tag.strip_prefix("go1.") {
        return version
            .parse::<u32>()
            .is_ok_and(|minor| go_version_minor(go_version).is_some_and(|max| minor <= max));
    }
    matches!(tag, "gc")
}

fn go_version_minor(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    if parts.next()? != "1" {
        return None;
    }
    parts.next()?.parse::<u32>().ok()
}

fn target_go_os() -> String {
    let os =
        std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| std::env::consts::OS.to_string());
    match os.as_str() {
        "macos" => "darwin".to_string(),
        other => other.to_string(),
    }
}

fn is_go_os(value: &str) -> bool {
    matches!(
        value,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "js"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "plan9"
            | "solaris"
            | "wasip1"
            | "windows"
    )
}

fn is_go_arch(value: &str) -> bool {
    matches!(
        value,
        "386"
            | "amd64"
            | "arm"
            | "arm64"
            | "loong64"
            | "mips"
            | "mips64"
            | "mips64le"
            | "mipsle"
            | "ppc64"
            | "ppc64le"
            | "riscv64"
            | "s390x"
            | "wasm"
            | "gors"
    )
}

fn is_unix_goos(value: &str) -> bool {
    matches!(
        value,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "solaris"
    )
}

fn main() -> BuildResult<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={GO_VERSION_FILE}");
    println!("cargo:rerun-if-env-changed=GORS_GO_SDK_PATH");

    let go_version = read_go_version()?;
    let stdlib_version = stdlib_version(&go_version);
    let sdk_path = ensure_go_sdk(&go_version)?;
    println!("cargo:rustc-env=GORS_GO_VERSION={go_version}");
    println!("cargo:rustc-env=GORS_STDLIB_VERSION={stdlib_version}");
    println!(
        "cargo:rustc-env=GORS_BUILT_GO_SDK_PATH={}",
        sdk_path.display()
    );

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let preload_path = out_dir.join("go_stdlib.rs");
    let source_dir = out_dir.join("go_stdlib_src");
    let marker_path = out_dir.join("go_stdlib.version");
    let target_goos = target_go_os();
    let preload_schema = stdlib_preload_schema(&go_version, &target_goos);

    if preload_path.exists()
        && source_dir.exists()
        && std::fs::read_to_string(&marker_path).is_ok_and(|s| s == preload_schema)
    {
        return Ok(());
    }

    let packages = extract_stdlib_from_sdk(&sdk_path, &go_version, &target_goos);

    eprintln!(
        "Preloading {} Go stdlib packages for GOOS={target_goos} GOARCH=gors ({} total files)",
        packages.len(),
        packages.values().map(|v| v.len()).sum::<usize>()
    );

    create_stdlib_preload(&packages, &preload_path, &source_dir)?;
    std::fs::write(&marker_path, preload_schema)?;
    eprintln!("Created stdlib preload at {}", preload_path.display());
    Ok(())
}
