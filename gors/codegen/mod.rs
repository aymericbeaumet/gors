//! Rust code generation.
//!
//! This module provides functions for formatting Rust `syn` AST into
//! pretty-printed source code using the `prettyplease` library.

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
