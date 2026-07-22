//! Embedded Go SDK source metadata.
//!
//! Resolution deliberately stops at source discovery. Semantic indexing,
//! reachability, lowering, and caching belong to the compiler's semantic
//! database; this module must never manufacture Rust syntax or partial packages.

#[derive(Clone, Copy)]
struct EmbeddedGoFile {
    filename: &'static str,
    content: &'static str,
}

#[derive(Clone, Copy)]
struct EmbeddedGoAsset {
    path: &'static str,
    content: &'static [u8],
    sha256: &'static str,
}

/// A build-selected package input that requires an unsupported host/runtime
/// integration path instead of ordinary Go semantic lowering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedGoInputKind {
    /// Go source selected only when cgo is enabled.
    Cgo,
    /// Go assembler source.
    GoAssembly,
    /// A precompiled system object.
    SystemObject,
}

/// Exact metadata for a selected package input that is not ordinary Go source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedGoPackageInput {
    pub kind: UnsupportedGoInputKind,
    pub path: &'static str,
    pub content: &'static [u8],
    pub sha256: &'static str,
}

#[derive(Clone, Copy)]
struct UnsupportedGoInput {
    kind: UnsupportedGoInputKind,
    path: &'static str,
    content: &'static [u8],
    sha256: &'static str,
}

#[derive(Clone, Copy)]
struct EmbeddedGoPackage {
    import_path: &'static str,
    files: &'static [EmbeddedGoFile],
    direct_imports: &'static [&'static str],
    embed_patterns: &'static [&'static str],
    embed_files: &'static [EmbeddedGoAsset],
    unsupported_inputs: &'static [UnsupportedGoInput],
}

include!(concat!(env!("OUT_DIR"), "/go_stdlib.rs"));

fn embedded_package(import_path: &str) -> Option<&'static EmbeddedGoPackage> {
    EMBEDDED_PACKAGES
        .binary_search_by(|package| package.import_path.cmp(import_path))
        .ok()
        .and_then(|index| EMBEDDED_PACKAGES.get(index))
}

/// Whether the pinned Go SDK contains this import path.
#[must_use]
pub fn is_known(import_path: &str) -> bool {
    embedded_package(import_path).is_some()
}

/// Whether the pinned Go SDK contains this import path.
#[must_use]
pub fn package_exists(import_path: &str) -> bool {
    is_known(import_path)
}

/// Return the exact build-selected Go sources embedded for a package.
#[must_use]
pub fn package_files(import_path: &str) -> Option<Vec<(&'static str, &'static str)>> {
    embedded_package(import_path).map(|package| {
        package
            .files
            .iter()
            .map(|file| (file.filename, file.content))
            .collect()
    })
}

/// Return the direct imports recorded for an embedded package.
#[must_use]
pub fn package_imports(import_path: &str) -> Option<&'static [&'static str]> {
    embedded_package(import_path).map(|package| package.direct_imports)
}

/// Return the exact `//go:embed` patterns selected for a package.
#[must_use]
pub fn package_embed_patterns(import_path: &str) -> Option<&'static [&'static str]> {
    embedded_package(import_path).map(|package| package.embed_patterns)
}

/// Return build-selected embedded assets as `(path, bytes, SHA-256)` tuples.
#[must_use]
pub fn package_embed_files(
    import_path: &str,
) -> Option<Vec<(&'static str, &'static [u8], &'static str)>> {
    embedded_package(import_path).map(|package| {
        package
            .embed_files
            .iter()
            .map(|asset| (asset.path, asset.content, asset.sha256))
            .collect()
    })
}

/// Return selected non-Go inputs that semantic package loading must reject or
/// route through an explicit host/runtime implementation.
#[must_use]
pub fn package_unsupported_inputs(import_path: &str) -> Option<Vec<UnsupportedGoPackageInput>> {
    embedded_package(import_path).map(|package| {
        package
            .unsupported_inputs
            .iter()
            .map(|input| UnsupportedGoPackageInput {
                kind: input.kind,
                path: input.path,
                content: input.content,
                sha256: input.sha256,
            })
            .collect()
    })
}

/// List all import paths in deterministic order.
#[must_use]
pub fn list_packages() -> Vec<String> {
    EMBEDDED_PACKAGES
        .iter()
        .map(|package| package.import_path.to_string())
        .collect()
}

/// Convert a Go import path into a valid, stable Rust module identifier.
#[must_use]
pub fn module_name(import_path: &str) -> String {
    let mut output = String::new();
    for character in import_path.chars() {
        match character {
            '/' => output.push_str("__"),
            character if character.is_ascii_alphanumeric() || character == '_' => {
                output.push(character);
            }
            _ => output.push('_'),
        }
    }

    if output.is_empty() {
        output.push('_');
    }
    if output.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        output.insert(0, '_');
    }
    if is_rust_keyword(&output) {
        output.push('_');
    }
    output
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn source_metadata_is_available_without_compiling_the_package() {
        let files = package_files("fmt").expect("fmt source");
        assert!(!files.is_empty());
        assert!(package_imports("fmt").is_some());
    }

    #[test]
    fn module_names_are_valid_and_stable() {
        assert_eq!(module_name("io/fs"), "io__fs");
        assert_eq!(module_name("type"), "type_");
    }

    #[test]
    fn embed_assets_keep_exact_bytes_and_hashes() {
        let assets =
            package_embed_files("internal/trace/traceviewer").expect("traceviewer embed metadata");
        let (path, content, expected_hash) = assets
            .into_iter()
            .find(|(path, _, _)| *path == "static/trace_viewer_full.html")
            .expect("trace viewer HTML asset");
        let actual_hash = Sha256::digest(content)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        assert_eq!(path, "static/trace_viewer_full.html");
        assert!(!content.is_empty());
        assert_eq!(actual_hash, expected_hash);
    }

    #[test]
    fn host_inputs_are_explicit_instead_of_silently_dropped() {
        let inputs = package_unsupported_inputs("math").expect("math package inputs");
        assert!(inputs.iter().any(|input| {
            input.kind == UnsupportedGoInputKind::GoAssembly
                && input.path.ends_with(".s")
                && !input.content.is_empty()
                && input.sha256.len() == 64
        }));
    }
}
