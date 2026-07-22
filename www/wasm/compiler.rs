use gors::error::{Diagnostic, DiagnosticKind};
use wasm_bindgen::prelude::*;

use crate::build_result::BuildResult;
use crate::comments;

/// Explicitly owned browser compiler state.
///
/// Retaining this value across edits retains the compiler query database. The
/// browser presentation layer still parses once to validate the complete
/// program and once to collect comments before the tracked compiler parse; the
/// session removes neither of those temporary frontend parses yet.
#[wasm_bindgen]
pub struct GorsCompiler {
    session: gors::compiler::CompilerSession,
}

#[wasm_bindgen]
impl GorsCompiler {
    /// Construct one compiler session for the lifetime of a browser worker.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self {
            session: gors::compiler::CompilerSession::default(),
        }
    }

    /// Build Go source while preserving query products from earlier edits.
    pub fn build_rust(&mut self, input: String) -> BuildResult {
        build_rust_with_session(&mut self.session, input)
    }
}

impl Default for GorsCompiler {
    fn default() -> Self {
        Self::new()
    }
}

fn build_rust_with_session(
    session: &mut gors::compiler::CompilerSession,
    input: String,
) -> BuildResult {
    let program = match gors::parser::parse_program_from_source("main.go", &input) {
        Ok(program) => program,
        Err(error) => {
            let diagnostic = match error {
                gors::parser::PathParseError::ParserError(ref error) => {
                    Diagnostic::from_file_parse_error(error)
                }
                gors::parser::PathParseError::InvalidImportPath(ref error) => {
                    Diagnostic::from_invalid_import_path(error)
                }
                _ => Diagnostic::new("main.go", 0, 0, error.to_string(), DiagnosticKind::Compiler),
            };
            return BuildResult::error_result(diagnostic);
        }
    };

    let comments = {
        let Some(file) = program.main_package().files().first() else {
            return BuildResult::error_result(Diagnostic::new(
                "main.go",
                0,
                0,
                "parsed program contains no entry source file",
                DiagnosticKind::Compiler,
            ));
        };
        let ast = match file.parse() {
            Ok(ast) => ast,
            Err(error) => {
                return BuildResult::error_result(Diagnostic::from_parser_error(
                    &error,
                    file.path(),
                    file.source(),
                ));
            }
        };
        comments::collect(&ast, file.source())
    };
    let (compiled, source_map_plan) = match session.compile_program_with_source_map(program) {
        Ok(result) => result,
        Err(error) => {
            return BuildResult::error_result(compiler_diagnostic(&error, "main.go", &input));
        }
    };
    let rust_source = match gors::printer::generate_single(compiled) {
        Ok(output) => output,
        Err(error) => {
            return BuildResult::error_result(Diagnostic::new(
                "main.go",
                0,
                0,
                error.to_string(),
                DiagnosticKind::Compiler,
            ));
        }
    };

    let initial_source_map = source_map_plan.build(&rust_source);
    let (output, source_map) =
        comments::insert_and_remap(&rust_source, &comments, &initial_source_map);
    BuildResult::success_rust(output, source_map)
}

#[cfg(test)]
impl GorsCompiler {
    pub(crate) fn session(&self) -> &gors::compiler::CompilerSession {
        &self.session
    }
}

fn compiler_diagnostic(
    error: &gors::compiler::CompilerError,
    fallback_file: &str,
    source: &str,
) -> Diagnostic {
    let Some(diagnostic) = error.diagnostics().first() else {
        return Diagnostic::new(
            fallback_file,
            0,
            0,
            error.to_string(),
            DiagnosticKind::Compiler,
        );
    };
    let file = if diagnostic.file.is_empty() {
        fallback_file
    } else {
        &diagnostic.file
    };
    Diagnostic::new(
        file,
        diagnostic.line,
        diagnostic.column,
        format!("{}: {}", diagnostic.code, diagnostic.message),
        DiagnosticKind::Compiler,
    )
    .with_source(source)
}
