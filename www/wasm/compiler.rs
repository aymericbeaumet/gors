use std::sync::Arc;

use gors::error::{Diagnostic, DiagnosticKind};
use wasm_bindgen::prelude::*;

use crate::build_result::BuildResult;
use crate::comments;
use crate::runtime_dependency::RuntimeDependencyProtocol;

/// Explicitly owned browser compiler state.
///
/// Retaining this value across edits retains the compiler query database. Raw
/// browser source enters that database without a presentation-layer parse;
/// syntax validation and owned comment projection are query results.
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
    let input: Arc<str> = input.into();
    let program = match browser_program_input(Arc::clone(&input)) {
        Ok(program) => program,
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
    let (compiled, source_map_plan) = match session.compile_program_with_source_map(program) {
        Ok(result) => result,
        Err(error) => {
            return BuildResult::error_result(compiler_diagnostic(&error, "main.go", &input));
        }
    };
    let comments = comments::collect(
        source_map_plan.entry_comments(),
        source_map_plan.entry_source_name(),
        &input,
    );
    let mut generated = match gors::printer::generate_single(compiled) {
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
    let runtime_dependency = match RuntimeDependencyProtocol::from_generated_output(&generated) {
        Ok(dependency) => dependency,
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
    let Some(rust_source) = generated.files.remove("main.rs") else {
        return BuildResult::error_result(Diagnostic::new(
            "main.go",
            0,
            0,
            "single-file Rust generation omitted main.rs",
            DiagnosticKind::Compiler,
        ));
    };
    if !generated.files.is_empty() {
        return BuildResult::error_result(Diagnostic::new(
            "main.go",
            0,
            0,
            "single-file Rust generation produced unexpected additional files",
            DiagnosticKind::Compiler,
        ));
    }

    let initial_source_map = source_map_plan.build(&rust_source);
    let (output, source_map) =
        comments::insert_and_remap(&rust_source, &comments, &initial_source_map);
    BuildResult::success_rust(output, source_map, runtime_dependency)
}

fn browser_program_input(
    source: Arc<str>,
) -> Result<gors::compiler::input::ProgramInput, gors::compiler::input::InputError> {
    use gors::compiler::input::{
        PackageInputManifest, PackageKey, ProgramInput, SourceFileInput, WorkspaceKey,
    };

    let package_key = PackageKey::command_line();
    let file = SourceFileInput::from_source("main.go", "main.go", source)?;
    let package = PackageInputManifest::new(package_key.clone(), [file])?;
    ProgramInput::new(
        WorkspaceKey::ad_hoc("browser-worker")?,
        package_key,
        [package],
    )
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
