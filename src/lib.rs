//! # Gors
//!
//! A Go toolchain written in Rust, featuring a scanner, parser, compiler, and
//! code generator that transpiles Go source code to Rust.
//!
//! ## Overview
//!
//! This library provides the core components for parsing and compiling Go code:
//!
//! - [`scanner`] - Lexical analysis of Go source code into tokens
//! - [`parser`] - Parsing tokens into a Go Abstract Syntax Tree (AST)
//! - [`ast`] - Go AST data structures based on the Go language specification
//! - [`compiler`] - Transforms Go AST into Rust `syn` AST
//! - [`backend_rust`] - Formats the Rust AST into source code
//! - [`backend_wasm`] - Compiles Rust AST directly to WebAssembly
//! - [`error`] - Error types and diagnostic formatting
//! - [`token`] - Token types and source position tracking
//!
//! ## Example
//!
//! ```
//! use gors::{parser, compiler, backend_rust};
//!
//! let go_source = r#"
//!     package main
//!
//!     func main() {
//!         x := 1 + 2
//!     }
//! "#;
//!
//! // Parse Go source into AST
//! let ast = parser::parse_file("example.go", go_source).unwrap();
//!
//! // Compile Go AST to Rust AST
//! let rust_ast = compiler::compile(ast).unwrap();
//!
//! // Generate Rust source code
//! let rust_source = backend_rust::generate(rust_ast).unwrap();
//! ```

// Clippy lints are configured at workspace level in the root Cargo.toml
#![allow(dead_code)] // Some code is intentionally unused for future features

/// Go Abstract Syntax Tree data structures.
///
/// This module contains the AST node types based on the
/// [Go language specification](https://go.dev/ref/spec).
pub mod ast;

/// Rust code generation from syn AST.
///
/// Provides formatting of `syn::File` into pretty-printed Rust source code.
pub mod backend_rust;

/// Go to Rust compiler.
///
/// Transforms a Go AST into a Rust `syn` AST, applying various
/// transformation passes to produce idiomatic Rust code.
pub mod compiler;

/// Error types and diagnostic formatting.
///
/// Provides structured error reporting with source context.
pub mod error;

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

/// Source mapping between Go and Rust code.
///
/// Provides data structures for tracking correspondence between
/// positions in Go source code and generated Rust output.
pub mod mapping;

/// Go token definitions and source positions.
///
/// Contains token types matching the Go specification and
/// position tracking for source locations.
pub mod token;

/// WebAssembly backend for direct WASM compilation.
///
/// Compiles Rust AST (syn::File) directly to WebAssembly bytecode
/// using the Walrus library. Works in both native CLI and browser.
pub mod backend_wasm;
