//! # Gors
//!
//! A Go toolchain written in Rust, featuring a scanner, parser, compiler, and
//! Rust source printer for transpiled Go programs.
//!
//! ## Overview
//!
//! This library provides the core components for parsing and compiling Go code:
//!
//! - [`scanner`] - Lexical analysis of Go source code into tokens
//! - [`parser`] - Parsing tokens into a Go Abstract Syntax Tree (AST)
//! - [`ast`] - Go AST data structures based on the Go language specification
//! - [`compiler`] - Lowers Go AST through typed HIR, Go MIR, and Rust IR into Rust syntax
//! - [`printer`] - Formats the Rust AST into source code
//! - [`error`] - Error types and diagnostic formatting
//! - [`token`] - Token types and source position tracking
//!
//! ## Example
//!
//! ```
//! use gors::{compiler, printer};
//! use gors::compiler::input::{
//!     PackageInputManifest, PackageKey, ProgramInput, SourceFileInput, WorkspaceKey,
//! };
//!
//! let go_source = r#"
//!     package main
//!
//!     func main() {
//!         x := 1 + 2
//!     }
//! "#;
//!
//! let package = PackageKey::command_line();
//! let file = SourceFileInput::from_source("example.go", "example.go", go_source).unwrap();
//! let manifest = PackageInputManifest::new(package.clone(), [file]).unwrap();
//! let input = ProgramInput::new(
//!     WorkspaceKey::ad_hoc("example").unwrap(),
//!     package,
//!     [manifest],
//! ).unwrap();
//! let compiled = compiler::compile_program(input).unwrap();
//! let rust_source = printer::generate_single(compiled).unwrap();
//! ```

// Lints are configured at workspace level in the root Cargo.toml.

mod artifact;

/// Go SDK version pinned by the repository-level `.go-version` file.
pub const GO_VERSION: &str = env!("GORS_GO_VERSION");

/// Version label for the embedded Go stdlib archive compiled into gors.
pub const STDLIB_VERSION: &str = env!("GORS_STDLIB_VERSION");

/// Content fingerprint for compiler and generated-runtime sources.
///
/// Persistent compiler artifacts include this value so they are rejected
/// automatically when lowering or runtime semantics change.
pub const COMPILER_FINGERPRINT: &str = env!("GORS_COMPILER_FINGERPRINT");

#[cfg(any(
    feature = "test_integration_go_repositories",
    feature = "test_integration_go_spec",
    feature = "test_integration_go_stdlib",
    feature = "test_integration_go_programs"
))]
/// Extracted Go SDK path used by integration tests for Go oracle/runtime checks.
pub const GO_SDK_PATH: &str = env!("GORS_BUILT_GO_SDK_PATH");

/// Go Abstract Syntax Tree data structures.
///
/// This module contains the AST node types based on the
/// [Go language specification](https://go.dev/ref/spec).
pub mod ast;

/// Rust source printing from syn AST.
///
/// Provides formatting of `syn::File` into pretty-printed Rust source code.
pub mod printer;

/// Go-to-Rust compiler.
///
/// Uses authoritative typed HIR, explicit-order Go MIR, and mandatory verified
/// Rust IR. Rust `syn` syntax is only the terminal emission format.
pub mod compiler;

/// Error types and diagnostic formatting.
///
/// Provides structured error reporting with source context.
pub mod error;

pub(crate) mod profile;

/// Go source code parser.
///
/// Parses Go source code into an Abstract Syntax Tree following
/// the Go language specification grammar.
pub mod parser;

/// Go source code scanner (lexer).
///
/// Performs lexical analysis of Go source code, producing tokens
/// with position information.
pub mod scanner;

/// Physical and Go-adjusted source coordinates shared by frontend layers.
///
/// Byte offsets and ranges are independent of compiler semantic identities;
/// compiler-owned file provenance lives in [`compiler::provenance`].
pub mod source;

/// Source mapping between Go and Rust code.
///
/// Provides data structures for tracking correspondence between
/// positions in Go source code and generated Rust output.
pub mod sourcemap;

/// Go package resolution.
///
/// Exposes build-selected source metadata for packages in the embedded Go SDK.
pub mod resolve;

/// Go token definitions and source positions.
///
/// Contains token types matching the Go specification and
/// position tracking for source locations.
pub mod token;

/// Filesystem discovery for syntax-unvalidated compiler inputs.
pub mod workspace;

// The build-script platform mapping is pure and shared here only so ordinary
// library unit tests exercise host/target separation. It is not a runtime API.
#[cfg(test)]
#[path = "../build/platform.rs"]
mod build_platform_tests;

// Keep cross-target Go source-oracle behavior in the ordinary unit-test gate.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../build/sdk_index.rs"]
mod build_sdk_index_tests;
