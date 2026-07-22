//! Authoritative Go SDK package and source selection.
//!
//! The pinned Go tool owns Go build-constraint, architecture-feature,
//! experiment-tag, and vendoring semantics. Reimplementing those rules in the
//! Rust build script caused target files and canonical vendor dependencies to
//! disappear silently, so this module consumes `go list` as the source oracle.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

type IndexResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdlibSourceFile {
    pub filename: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdlibPackage {
    pub files: Vec<StdlibSourceFile>,
    pub direct_imports: Vec<String>,
    pub embed_patterns: Vec<String>,
    pub embed_files: Vec<StdlibBinaryFile>,
    pub unsupported_inputs: Vec<StdlibUnsupportedInput>,
}

pub type StdlibPackages = BTreeMap<String, StdlibPackage>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdlibBinaryFile {
    pub path: String,
    pub content: Vec<u8>,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UnsupportedInputKind {
    Cgo,
    GoAssembly,
    SystemObject,
}

impl UnsupportedInputKind {
    pub const fn rust_variant(self) -> &'static str {
        match self {
            Self::Cgo => "Cgo",
            Self::GoAssembly => "GoAssembly",
            Self::SystemObject => "SystemObject",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdlibUnsupportedInput {
    pub kind: UnsupportedInputKind,
    pub file: StdlibBinaryFile,
}

#[derive(Debug, Deserialize)]
struct GoListPackage {
    #[serde(rename = "Dir")]
    dir: PathBuf,
    #[serde(rename = "ImportPath")]
    import_path: String,
    #[serde(rename = "GoFiles", default)]
    go_files: Vec<String>,
    #[serde(rename = "Imports", default)]
    imports: Vec<String>,
    #[serde(rename = "ImportMap", default)]
    import_map: BTreeMap<String, String>,
    #[serde(rename = "EmbedPatterns", default)]
    embed_patterns: Vec<String>,
    #[serde(rename = "EmbedFiles", default)]
    embed_files: Vec<String>,
    #[serde(rename = "CgoFiles", default)]
    cgo_files: Vec<String>,
    #[serde(rename = "SFiles", default)]
    assembly_files: Vec<String>,
    #[serde(rename = "SysoFiles", default)]
    system_object_files: Vec<String>,
    #[serde(rename = "Goroot", default)]
    goroot: bool,
    #[serde(rename = "Standard", default)]
    standard: bool,
}

fn index_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

/// Load the complete build-selected standard-library source index.
pub fn load_stdlib_from_sdk(
    sdk_path: &Path,
    cache_root: &Path,
    target_goos: &str,
    target_goarch: &str,
) -> IndexResult<StdlibPackages> {
    load_packages_from_sdk(sdk_path, cache_root, target_goos, target_goarch, &["std"])
}

fn load_packages_from_sdk(
    sdk_path: &Path,
    cache_root: &Path,
    target_goos: &str,
    target_goarch: &str,
    patterns: &[&str],
) -> IndexResult<StdlibPackages> {
    let go_cache = cache_root.join("build-cache");
    let go_tmp = cache_root.join("tmp");
    let go_mod_cache = cache_root.join("module-cache");
    std::fs::create_dir_all(&go_cache)?;
    std::fs::create_dir_all(&go_tmp)?;
    std::fs::create_dir_all(&go_mod_cache)?;

    let go_binary = sdk_path.join("bin/go");
    if !go_binary.is_file() {
        return Err(index_error(format!(
            "pinned Go SDK has no executable at {}",
            go_binary.display()
        ))
        .into());
    }

    let mut command = Command::new(&go_binary);
    command
        .env_clear()
        .env("GOROOT", sdk_path)
        .env("GOOS", target_goos)
        .env("GOARCH", target_goarch)
        .env("CGO_ENABLED", "0")
        .env("GO111MODULE", "off")
        .env("GOENV", "off")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOTELEMETRY", "off")
        .env("GOCACHE", &go_cache)
        .env("GOMODCACHE", &go_mod_cache)
        .env("GOTMPDIR", &go_tmp)
        .arg("list")
        .arg("-json")
        .arg("-deps")
        .arg("-tags=gors")
        .args(patterns);

    let output = command.output()?;
    if !output.status.success() {
        return Err(index_error(format!(
            "pinned Go source oracle failed for GOOS={target_goos} GOARCH={target_goarch}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into());
    }

    let sdk_source = sdk_path.join("src");
    let mut packages = StdlibPackages::new();
    let stream = serde_json::Deserializer::from_slice(&output.stdout).into_iter::<GoListPackage>();
    for decoded in stream {
        let package = decoded.map_err(|error| {
            index_error(format!(
                "invalid JSON from pinned Go source oracle for GOOS={target_goos} GOARCH={target_goarch}: {error}"
            ))
        })?;
        validate_package_identity(&package, &sdk_source)?;

        let GoListPackage {
            dir,
            import_path,
            go_files,
            imports,
            import_map,
            mut embed_patterns,
            embed_files,
            cgo_files,
            assembly_files,
            system_object_files,
            ..
        } = package;
        let canonical_dir = std::fs::canonicalize(&dir).map_err(|error| {
            index_error(format!(
                "failed to canonicalize Go source oracle package directory {} for {import_path}: {error}",
                dir.display()
            ))
        })?;

        let mut files = Vec::with_capacity(go_files.len());
        for filename in go_files {
            validate_filename(&filename, &import_path)?;
            let bytes = read_package_file(&dir, &canonical_dir, &filename, &import_path)?;
            let content = String::from_utf8(bytes).map_err(|error| {
                index_error(format!(
                    "Go source oracle file {filename:?} for {import_path} is not UTF-8: {error}"
                ))
            })?;
            files.push(StdlibSourceFile { filename, content });
        }
        files.sort_by(|left, right| left.filename.cmp(&right.filename));

        let mut direct_imports = imports
            .into_iter()
            .map(|import_path| import_map.get(&import_path).cloned().unwrap_or(import_path))
            .collect::<Vec<_>>();
        direct_imports.sort();
        direct_imports.dedup();

        embed_patterns.sort();
        embed_patterns.dedup();
        let mut embed_files = embed_files
            .into_iter()
            .map(|path| read_binary_file(&dir, &canonical_dir, path, &import_path))
            .collect::<IndexResult<Vec<_>>>()?;
        embed_files.sort_by(|left, right| left.path.cmp(&right.path));
        ensure_unique_paths(&embed_files, "embed asset", &import_path)?;

        let mut unsupported_inputs = Vec::new();
        for (kind, paths) in [
            (UnsupportedInputKind::Cgo, cgo_files),
            (UnsupportedInputKind::GoAssembly, assembly_files),
            (UnsupportedInputKind::SystemObject, system_object_files),
        ] {
            for path in paths {
                unsupported_inputs.push(StdlibUnsupportedInput {
                    kind,
                    file: read_binary_file(&dir, &canonical_dir, path, &import_path)?,
                });
            }
        }
        unsupported_inputs.sort_by(|left, right| {
            (left.kind, left.file.path.as_str()).cmp(&(right.kind, right.file.path.as_str()))
        });

        if packages
            .insert(
                import_path.clone(),
                StdlibPackage {
                    files,
                    direct_imports,
                    embed_patterns,
                    embed_files,
                    unsupported_inputs,
                },
            )
            .is_some()
        {
            return Err(index_error(format!(
                "pinned Go source oracle returned duplicate package {import_path}"
            ))
            .into());
        }
    }

    if packages.is_empty() {
        return Err(index_error(format!(
            "pinned Go source oracle returned no packages for GOOS={target_goos} GOARCH={target_goarch}"
        ))
        .into());
    }

    let known_packages = packages.keys().cloned().collect::<BTreeSet<_>>();
    for (import_path, package) in &packages {
        for dependency in &package.direct_imports {
            if !known_packages.contains(dependency) {
                return Err(index_error(format!(
                    "pinned Go source oracle returned missing direct dependency {dependency} for {import_path}"
                ))
                .into());
            }
        }
    }

    Ok(packages)
}

fn validate_package_identity(package: &GoListPackage, sdk_source: &Path) -> IndexResult<()> {
    if !package.goroot || !package.standard {
        return Err(index_error(format!(
            "Go source oracle returned non-standard package {}",
            package.import_path
        ))
        .into());
    }
    let relative_dir = package.dir.strip_prefix(sdk_source).map_err(|_| {
        index_error(format!(
            "Go source oracle package {} escaped pinned SDK source root: {}",
            package.import_path,
            package.dir.display()
        ))
    })?;
    let expected = Path::new(&package.import_path);
    if relative_dir != expected {
        return Err(index_error(format!(
            "Go source oracle package identity mismatch: {} resolved to {}",
            package.import_path,
            relative_dir.display()
        ))
        .into());
    }
    Ok(())
}

fn read_binary_file(
    package_dir: &Path,
    canonical_package_dir: &Path,
    path: String,
    import_path: &str,
) -> IndexResult<StdlibBinaryFile> {
    let content = read_package_file(package_dir, canonical_package_dir, &path, import_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let sha256 = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(StdlibBinaryFile {
        path,
        content,
        sha256,
    })
}

fn read_package_file(
    package_dir: &Path,
    canonical_package_dir: &Path,
    relative_path: &str,
    import_path: &str,
) -> IndexResult<Vec<u8>> {
    validate_relative_path(relative_path, import_path)?;
    let path = package_dir.join(relative_path);
    let canonical_path = std::fs::canonicalize(&path).map_err(|error| {
        index_error(format!(
            "failed to resolve Go source oracle file {} for {import_path}: {error}",
            path.display()
        ))
    })?;
    if canonical_path.strip_prefix(canonical_package_dir).is_err() {
        return Err(index_error(format!(
            "Go source oracle file {relative_path:?} for {import_path} escaped its package directory"
        ))
        .into());
    }
    if !canonical_path.is_file() {
        return Err(index_error(format!(
            "Go source oracle input {relative_path:?} for {import_path} is not a file"
        ))
        .into());
    }
    std::fs::read(&canonical_path).map_err(|error| {
        index_error(format!(
            "failed to read Go source oracle file {} for {import_path}: {error}",
            canonical_path.display()
        ))
        .into()
    })
}

fn validate_relative_path(relative_path: &str, import_path: &str) -> IndexResult<()> {
    if relative_path.is_empty() || relative_path.contains('\\') {
        return Err(index_error(format!(
            "Go source oracle returned invalid relative path {relative_path:?} for {import_path}"
        ))
        .into());
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(index_error(format!(
            "Go source oracle returned unsafe relative path {relative_path:?} for {import_path}"
        ))
        .into());
    }
    Ok(())
}

fn ensure_unique_paths(
    files: &[StdlibBinaryFile],
    kind: &str,
    import_path: &str,
) -> IndexResult<()> {
    for pair in files.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        if left.path == right.path {
            return Err(index_error(format!(
                "Go source oracle returned duplicate {kind} {:?} for {import_path}",
                left.path
            ))
            .into());
        }
    }
    Ok(())
}

fn validate_filename(filename: &str, import_path: &str) -> IndexResult<()> {
    let mut components = Path::new(filename).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(index_error(format!(
            "Go source oracle returned invalid filename {filename:?} for {import_path}"
        ))
        .into());
    }
    if !filename.ends_with(".go") {
        return Err(index_error(format!(
            "Go source oracle returned non-Go source {filename:?} for {import_path}"
        ))
        .into());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn pinned_sdk() -> PathBuf {
        PathBuf::from(env!("GORS_BUILT_GO_SDK_PATH"))
    }

    fn file_names<'a>(packages: &'a StdlibPackages, import_path: &str) -> BTreeSet<&'a str> {
        packages
            .get(import_path)
            .expect("oracle package")
            .files
            .iter()
            .map(|file| file.filename.as_str())
            .collect()
    }

    #[test]
    fn oracle_selects_exact_cross_architecture_sources() {
        let cache = tempfile::tempdir().expect("temporary Go list cache");
        let amd64 = load_packages_from_sdk(
            &pinned_sdk(),
            cache.path(),
            "linux",
            "amd64",
            &["math", "syscall"],
        )
        .expect("linux/amd64 source selection");
        let arm64 = load_packages_from_sdk(
            &pinned_sdk(),
            cache.path(),
            "linux",
            "arm64",
            &["math", "syscall"],
        )
        .expect("linux/arm64 source selection");

        assert!(file_names(&amd64, "math").contains("exp_amd64.go"));
        assert!(!file_names(&arm64, "math").contains("exp_amd64.go"));
        assert!(file_names(&amd64, "syscall").contains("zsyscall_linux_amd64.go"));
        assert!(file_names(&arm64, "syscall").contains("zsyscall_linux_arm64.go"));
        assert!(!file_names(&arm64, "syscall").contains("zsyscall_linux_amd64.go"));
    }

    #[test]
    fn oracle_selects_js_wasm_sources() {
        let cache = tempfile::tempdir().expect("temporary Go list cache");
        let packages = load_packages_from_sdk(
            &pinned_sdk(),
            cache.path(),
            "js",
            "wasm",
            &["os", "syscall"],
        )
        .expect("js/wasm source selection");

        assert!(file_names(&packages, "os").contains("stat_js.go"));
        assert!(file_names(&packages, "os").contains("pipe_wasm.go"));
        assert!(file_names(&packages, "syscall").contains("syscall_js.go"));
        assert!(!file_names(&packages, "syscall").contains("syscall_linux.go"));
    }

    #[test]
    fn oracle_preserves_canonical_vendor_import_identity() {
        let cache = tempfile::tempdir().expect("temporary Go list cache");
        let packages = load_packages_from_sdk(
            &pinned_sdk(),
            cache.path(),
            "linux",
            "amd64",
            &["net/http", "vendor/golang.org/x/net/http/httpguts"],
        )
        .expect("vendor-aware source selection");

        assert!(packages.contains_key("vendor/golang.org/x/net/http/httpguts"));
        let net_http = packages.get("net/http").expect("net/http package");
        assert!(
            net_http
                .direct_imports
                .contains(&"vendor/golang.org/x/net/http/httpguts".to_string())
        );
        assert!(
            !net_http
                .direct_imports
                .contains(&"golang.org/x/net/http/httpguts".to_string())
        );
    }

    #[test]
    fn oracle_carries_embed_assets_and_classifies_host_inputs() {
        let cache = tempfile::tempdir().expect("temporary Go list cache");
        let packages = load_packages_from_sdk(
            &pinned_sdk(),
            cache.path(),
            "linux",
            "amd64",
            &["internal/trace/traceviewer", "math"],
        )
        .expect("asset and host-input source selection");

        let traceviewer = packages
            .get("internal/trace/traceviewer")
            .expect("traceviewer package");
        let asset = traceviewer
            .embed_files
            .iter()
            .find(|asset| asset.path == "static/trace_viewer_full.html")
            .expect("trace viewer HTML embed asset");
        assert!(!asset.content.is_empty());
        assert_eq!(asset.sha256.len(), 64);
        assert!(
            traceviewer
                .embed_patterns
                .contains(&"static/trace_viewer_full.html".to_string())
        );

        let math = packages.get("math").expect("math package");
        assert!(math.unsupported_inputs.iter().any(|input| {
            input.kind == UnsupportedInputKind::GoAssembly && input.file.path.ends_with(".s")
        }));
    }
}
