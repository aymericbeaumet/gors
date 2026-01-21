//! Error types for WASM compilation.

use std::fmt;

/// Errors that can occur during WASM compilation.
#[derive(Debug)]
pub enum WasmError {
    /// Unsupported Rust construct
    Unsupported(String),
    /// Type error during compilation
    TypeError(String),
    /// Unknown identifier
    UnknownIdentifier(String),
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "Unsupported: {msg}"),
            Self::TypeError(msg) => write!(f, "Type error: {msg}"),
            Self::UnknownIdentifier(name) => write!(f, "Unknown identifier: {name}"),
        }
    }
}

impl std::error::Error for WasmError {}
