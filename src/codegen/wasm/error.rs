//! WASM code generation errors.

use crate::codegen::{CodegenError, CodegenErrorKind};

/// WASM-specific error type.
#[derive(Debug, Clone)]
pub enum WasmError {
    /// Unsupported function call
    UnsupportedFunction {
        name: String,
        line: u32,
        column: u32,
        suggestion: Option<String>,
    },
    /// Unsupported expression type
    UnsupportedExpression {
        kind: String,
        line: u32,
        column: u32,
    },
    /// Unsupported statement type
    UnsupportedStatement {
        kind: String,
        line: u32,
        column: u32,
    },
    /// Type inference failed
    TypeInference {
        message: String,
        line: u32,
        column: u32,
    },
    /// Validation error
    Validation {
        message: String,
    },
    /// General error
    General {
        message: String,
    },
}

impl std::fmt::Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFunction { name, line, column, suggestion } => {
                write!(f, "{}:{}: unsupported function '{}'", line, column, name)?;
                if let Some(s) = suggestion {
                    write!(f, "\nhint: {}", s)?;
                }
                Ok(())
            }
            Self::UnsupportedExpression { kind, line, column } => {
                write!(f, "{}:{}: unsupported expression type '{}'", line, column, kind)
            }
            Self::UnsupportedStatement { kind, line, column } => {
                write!(f, "{}:{}: unsupported statement type '{}'", line, column, kind)
            }
            Self::TypeInference { message, line, column } => {
                write!(f, "{}:{}: type inference error: {}", line, column, message)
            }
            Self::Validation { message } => {
                write!(f, "validation error: {}", message)
            }
            Self::General { message } => {
                write!(f, "{}", message)
            }
        }
    }
}

impl std::error::Error for WasmError {}

impl From<WasmError> for CodegenError {
    fn from(err: WasmError) -> Self {
        match err {
            WasmError::UnsupportedFunction { name, line, column, suggestion } => {
                let msg = if let Some(s) = suggestion {
                    format!("unsupported function '{}'. {}", name, s)
                } else {
                    format!("unsupported function '{}'", name)
                };
                CodegenError {
                    message: msg,
                    location: Some((line, column)),
                    kind: CodegenErrorKind::Unsupported,
                }
            }
            WasmError::UnsupportedExpression { kind, line, column } => {
                CodegenError {
                    message: format!("unsupported expression type '{}'", kind),
                    location: Some((line, column)),
                    kind: CodegenErrorKind::Unsupported,
                }
            }
            WasmError::UnsupportedStatement { kind, line, column } => {
                CodegenError {
                    message: format!("unsupported statement type '{}'", kind),
                    location: Some((line, column)),
                    kind: CodegenErrorKind::Unsupported,
                }
            }
            WasmError::TypeInference { message, line, column } => {
                CodegenError {
                    message,
                    location: Some((line, column)),
                    kind: CodegenErrorKind::TypeInference,
                }
            }
            WasmError::Validation { message } => {
                CodegenError {
                    message,
                    location: None,
                    kind: CodegenErrorKind::Validation,
                }
            }
            WasmError::General { message } => {
                CodegenError {
                    message,
                    location: None,
                    kind: CodegenErrorKind::Generation,
                }
            }
        }
    }
}

/// List of supported functions in the WASM backend.
pub const SUPPORTED_FUNCTIONS: &[&str] = &[
    "println",
    "print",
];

/// Suggest an alternative for an unsupported function.
pub fn suggest_alternative(func_name: &str) -> Option<String> {
    match func_name {
        "fmt.Sprintf" | "fmt.Printf" | "fmt.Print" => {
            Some("WASM backend only supports: println, print".to_string())
        }
        name if name.starts_with("fmt.") => {
            Some("WASM backend only supports: println (via fmt.Println)".to_string())
        }
        _ => Some(format!("WASM backend only supports: {}", SUPPORTED_FUNCTIONS.join(", ")))
    }
}
