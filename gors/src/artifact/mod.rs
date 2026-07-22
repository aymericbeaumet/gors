//! Rust artifact packaging inputs that are not compiler semantics.

/// Reserved module name used by the bootstrap standalone Rust artifact.
pub const RUNTIME_MODULE_NAME: &str = "__gors_runtime";

/// Exact source of the versioned runtime ABI bundled by the bootstrap Rust
/// artifact path.
///
/// Packaging copies this source directly. It must never be parsed back into a
/// semantic compiler stage or patched for a particular Go package.
pub const RUNTIME_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../gors-runtime/src/lib.rs"
));
