use gors::error::{Diagnostic, DiagnosticKind};
use wasm_bindgen::prelude::*;

use crate::build_result::BuildResult;
use crate::comments;

/// Build Go source with the authoritative HIR, Go MIR, and Rust IR pipeline.
#[wasm_bindgen]
pub fn build_rust(input: String) -> BuildResult {
    console_error_panic_hook::set_once();

    let program = match gors::parser::parse_program_from_source("main.go", &input) {
        Ok(program) => program,
        Err(error) => {
            let diagnostic = match error {
                gors::parser::PathParseError::ParserError(ref error) => {
                    Diagnostic::from_parser_error(error, "main.go", &input)
                }
                _ => Diagnostic::new("main.go", 0, 0, error.to_string(), DiagnosticKind::Compiler),
            };
            return BuildResult::error_result(diagnostic);
        }
    };

    let comments = comments::collect(&program.main_package.ast, &input);
    let (compiled, source_map_plan) = match gors::compiler::compile_program_with_source_map(program)
    {
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
