//! WebAssembly backend for gors.
//!
//! This module compiles `syn::File` (Rust AST) directly to WebAssembly bytecode
//! using the Walrus library. It works both in native CLI and when compiled to WASM
//! for the browser playground.
//!
//! # Supported Features
//!
//! - Basic types: i32, i64, f32, f64, bool
//! - Functions with parameters and return values
//! - Local variables
//! - Arithmetic and comparison operations
//! - Control flow: if/else, loops, return
//! - Function calls
//!
//! # Limitations
//!
//! - No heap allocation (stack only)
//! - No strings (only integer/float types)
//! - No standard library (imports for I/O)

mod compiler;
mod error;
mod expr;
mod types;

pub use compiler::WasmCompiler;
pub use error::WasmError;

/// Compile a `syn::File` to WebAssembly bytecode.
///
/// # Arguments
///
/// * `file` - The Rust AST to compile
///
/// # Returns
///
/// Returns the compiled WASM binary as a byte vector.
///
/// # Errors
///
/// Returns an error if compilation fails.
pub fn compile_to_wasm(file: &syn::File) -> Result<Vec<u8>, WasmError> {
    let mut compiler = WasmCompiler::new();
    compiler.compile(file)?;
    Ok(compiler.emit())
}
