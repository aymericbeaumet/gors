//! WebAssembly code generation backend.
//!
//! This module generates WebAssembly (WASM/WAT) from a `syn::File` AST.
//!
//! The output is WAT (WebAssembly Text Format) with inline comments
//! for debugging and source mapping.

mod compiler;
mod error;
mod types;

pub use compiler::WasmCompiler;
pub use error::WasmError;
pub use types::{TypeContext, WasmType};

use super::{CodegenError, SourceMap};

/// Generate WAT (WebAssembly Text) from a syn::File.
///
/// # Arguments
///
/// * `file` - The Rust AST to compile to WASM
///
/// # Returns
///
/// Returns `Ok(String)` containing the WAT text, or an error if compilation fails.
pub fn generate(file: syn::File) -> Result<String, CodegenError> {
    let mut compiler = WasmCompiler::new();
    compiler.compile(file)
}

/// Generate WAT with source file and content for better error messages.
///
/// # Arguments
///
/// * `file` - The Rust AST to compile to WASM
/// * `source_file` - Name of the source file for error messages
/// * `source_content` - Source code content for error context
///
/// # Returns
///
/// Returns `Ok(String)` containing the WAT text, or an error if compilation fails.
pub fn generate_with_source(
    file: syn::File,
    source_file: &str,
    source_content: &str,
) -> Result<String, CodegenError> {
    let mut compiler = WasmCompiler::new();
    compiler.set_source_file(source_file);
    compiler.set_source_content(source_content);
    compiler.compile(file)
}

/// Generate WAT with source map tracking.
///
/// # Arguments
///
/// * `file` - The Rust AST to compile to WASM
/// * `source_file` - Name of the source file for the source map
///
/// # Returns
///
/// Returns `Ok((String, SourceMap))` containing the WAT text and source map.
pub fn generate_with_sourcemap(
    file: syn::File,
    source_file: &str,
) -> Result<(String, SourceMap), CodegenError> {
    let mut compiler = WasmCompiler::new();
    compiler.set_source_file(source_file);
    let wat = compiler.compile(file)?;
    let source_map = compiler.into_source_map();
    Ok((wat, source_map))
}

/// Generate WAT with source map tracking and source content.
///
/// # Arguments
///
/// * `file` - The Rust AST to compile to WASM
/// * `source_file` - Name of the source file for the source map
/// * `source_content` - Source code content for error context
///
/// # Returns
///
/// Returns `Ok((String, SourceMap))` containing the WAT text and source map.
pub fn generate_with_sourcemap_and_source(
    file: syn::File,
    source_file: &str,
    source_content: &str,
) -> Result<(String, SourceMap), CodegenError> {
    let mut compiler = WasmCompiler::new();
    compiler.set_source_file(source_file);
    compiler.set_source_content(source_content);
    let wat = compiler.compile(file)?;
    let source_map = compiler.into_source_map();
    Ok((wat, source_map))
}
