use gors::error::{Diagnostic, DiagnosticKind};
use wasm_bindgen::prelude::*;

/// Result of a build operation
#[wasm_bindgen]
pub struct BuildResult {
    success: bool,
    output: String,
    error_message: String,
    error_file: String,
    error_line: u32,
    error_column: u32,
    error_end_column: u32,
    error_kind: String,
    error_source_line: String,
}

#[wasm_bindgen]
impl BuildResult {
    #[wasm_bindgen(getter)]
    pub fn success(&self) -> bool {
        self.success
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> String {
        self.output.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_message(&self) -> String {
        self.error_message.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_file(&self) -> String {
        self.error_file.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_line(&self) -> u32 {
        self.error_line
    }

    #[wasm_bindgen(getter)]
    pub fn error_column(&self) -> u32 {
        self.error_column
    }

    #[wasm_bindgen(getter)]
    pub fn error_end_column(&self) -> u32 {
        self.error_end_column
    }

    #[wasm_bindgen(getter)]
    pub fn error_kind(&self) -> String {
        self.error_kind.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn error_source_line(&self) -> String {
        self.error_source_line.clone()
    }
}

impl BuildResult {
    fn success_result(output: String) -> Self {
        Self {
            success: true,
            output,
            error_message: String::new(),
            error_file: String::new(),
            error_line: 0,
            error_column: 0,
            error_end_column: 0,
            error_kind: String::new(),
            error_source_line: String::new(),
        }
    }

    fn error_result(diagnostic: Diagnostic) -> Self {
        Self {
            success: false,
            output: String::new(),
            error_message: diagnostic.message.clone(),
            error_file: diagnostic.file.clone(),
            error_line: diagnostic.line as u32,
            error_column: diagnostic.column as u32,
            error_end_column: diagnostic.end_column as u32,
            error_kind: match diagnostic.kind {
                DiagnosticKind::Scanner => "scanner".to_string(),
                DiagnosticKind::Parser => "parser".to_string(),
                DiagnosticKind::Compiler => "compiler".to_string(),
            },
            error_source_line: diagnostic.source_line.unwrap_or_default(),
        }
    }
}

/// Build Go source code and return Rust code with structured error information
#[wasm_bindgen]
pub fn build(input: String) -> BuildResult {
    console_error_panic_hook::set_once();

    // Parse
    let ast = match gors::parser::parse_file("main.go", &input) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, "main.go", &input);
            return BuildResult::error_result(diagnostic);
        }
    };

    // Compile
    let compiled = match gors::compiler::compile(ast) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                "main.go",
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            return BuildResult::error_result(diagnostic);
        }
    };

    // Codegen
    let mut w = vec![];
    if let Err(err) = gors::codegen::fprint(&mut w, compiled) {
        let diagnostic = Diagnostic::new(
            "main.go",
            0,
            0,
            err.to_string(),
            DiagnosticKind::Compiler,
        );
        return BuildResult::error_result(diagnostic);
    }

    let output = String::from_utf8(w).unwrap_or_else(|_| "Invalid UTF-8 output".to_string());
    BuildResult::success_result(output)
}
