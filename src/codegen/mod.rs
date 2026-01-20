//! Code generation backend.
//!
//! This module generates Rust source code from a `syn::File` AST.

mod tracked;

pub use tracked::{
    generate_with_comments, generate_with_comments_and_blanks, BlankLineInfo, CommentToInsert,
};

/// Source mapping between input and output positions.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Individual position mappings
    pub mappings: Vec<Mapping>,
    /// Source file name
    pub source_file: String,
    /// Optional source content
    pub source_content: Option<String>,
}

/// A single position mapping from input to output.
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Input line number (1-based)
    pub input_line: u32,
    /// Input column number (1-based)
    pub input_column: u32,
    /// Output line number (1-based)
    pub output_line: u32,
    /// Output column number (1-based)
    pub output_column: u32,
    /// Optional name/identifier at this position
    pub name: Option<String>,
}

impl SourceMap {
    /// Create a new empty source map.
    pub fn new(source_file: &str) -> Self {
        Self {
            mappings: Vec::new(),
            source_file: source_file.to_string(),
            source_content: None,
        }
    }

    /// Add a mapping.
    pub fn add_mapping(
        &mut self,
        input_line: u32,
        input_column: u32,
        output_line: u32,
        output_column: u32,
        name: Option<String>,
    ) {
        self.mappings.push(Mapping {
            input_line,
            input_column,
            output_line,
            output_column,
            name,
        });
    }

    /// Look up output position for a given input position.
    /// Returns (output_line, output_column, end_line, end_column) if found.
    pub fn input_to_output(&self, line: u32, column: u32) -> Option<(u32, u32, u32, u32)> {
        // Find the closest mapping at or before the given position
        let mut best: Option<&Mapping> = None;
        for mapping in &self.mappings {
            if mapping.input_line == line {
                if mapping.input_column <= column {
                    match best {
                        None => best = Some(mapping),
                        Some(b) if mapping.input_column > b.input_column => best = Some(mapping),
                        _ => {}
                    }
                }
            }
        }
        best.map(|m| {
            // Return a span (single token for now)
            (m.output_line, m.output_column, m.output_line, m.output_column + 1)
        })
    }

    /// Look up input position for a given output position.
    /// Returns (input_line, input_column, end_line, end_column) if found.
    pub fn output_to_input(&self, line: u32, column: u32) -> Option<(u32, u32, u32, u32)> {
        // Find the closest mapping at or before the given position
        let mut best: Option<&Mapping> = None;
        for mapping in &self.mappings {
            if mapping.output_line == line {
                if mapping.output_column <= column {
                    match best {
                        None => best = Some(mapping),
                        Some(b) if mapping.output_column > b.output_column => best = Some(mapping),
                        _ => {}
                    }
                }
            }
        }
        best.map(|m| {
            // Return a span (single token for now)
            (m.input_line, m.input_column, m.input_line, m.input_column + 1)
        })
    }
}

/// Error type for code generation.
#[derive(Debug, Clone)]
pub struct CodegenError {
    /// Error message
    pub message: String,
    /// Optional source location (line, column)
    pub location: Option<(u32, u32)>,
    /// Error kind
    pub kind: CodegenErrorKind,
}

/// Kind of code generation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenErrorKind {
    /// General generation error
    Generation,
    /// Unsupported construct
    Unsupported,
    /// Type inference error
    TypeInference,
    /// Validation error
    Validation,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some((line, col)) = self.location {
            write!(f, "{}:{}: {}", line, col, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for CodegenError {}

impl CodegenError {
    /// Create a new generation error.
    pub fn generation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::Generation,
        }
    }

    /// Create a new unsupported construct error.
    pub fn unsupported(message: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            message: message.into(),
            location: Some((line, column)),
            kind: CodegenErrorKind::Unsupported,
        }
    }

    /// Create a new type inference error.
    pub fn type_inference(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::TypeInference,
        }
    }

    /// Create a new validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::Validation,
        }
    }

    /// Add location information.
    pub fn with_location(mut self, line: u32, column: u32) -> Self {
        self.location = Some((line, column));
        self
    }
}

/// Write formatted Rust source code to a writer.
///
/// # Arguments
///
/// * `w` - The writer to output the formatted code to
/// * `file` - The Rust AST to format
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if writing fails.
pub fn fprint<W: std::io::Write>(
    mut w: W,
    file: syn::File,
) -> Result<(), Box<dyn std::error::Error>> {
    let formatted = prettyplease::unparse(&file);

    for (i, line) in formatted.lines().enumerate() {
        if i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")) {
            w.write_all(b"\n")?;
        }
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
    }

    Ok(())
}

/// Generate formatted Rust source code as a String.
///
/// # Arguments
///
/// * `file` - The Rust AST to format
///
/// # Returns
///
/// Returns `Ok(String)` containing the formatted source code, or an error
/// if formatting fails.
///
/// # Example
///
/// ```
/// use gors::{parser, compiler, codegen};
///
/// let go_source = "package main\n\nfunc main() {}";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let rust_ast = compiler::compile(go_ast).unwrap();
/// let rust_source = codegen::generate(rust_ast).unwrap();
/// ```
pub fn generate(file: syn::File) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    fprint(&mut output, file)?;
    Ok(String::from_utf8(output)?)
}

/// Generate Rust source code with source map tracking.
///
/// # Arguments
///
/// * `file` - The Rust AST to format
/// * `source_file` - Name of the source file for the source map
///
/// # Returns
///
/// Returns `Ok((String, SourceMap))` containing the formatted source code and source map.
pub fn generate_with_sourcemap(
    file: syn::File,
    source_file: &str,
) -> Result<(String, SourceMap), CodegenError> {
    let output = generate(file).map_err(|e| CodegenError::generation(e.to_string()))?;

    // For now, create an empty source map
    // TODO: Integrate with the compiler's source map tracking
    let source_map = SourceMap::new(source_file);

    Ok((output, source_map))
}
