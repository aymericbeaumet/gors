use gors::error::{Diagnostic, DiagnosticKind};

/// Print a formatted error with source context.
pub fn print_error(diagnostic: &Diagnostic) {
    let use_colors = atty::is(atty::Stream::Stderr);
    eprint!("{}", diagnostic.format_terminal(use_colors));
}

pub fn print_compiler_error(error: &gors::compiler::CompilerError, fallback_file: &str) {
    for diagnostic in error.diagnostics() {
        let file = if diagnostic.file.is_empty() {
            fallback_file
        } else {
            &diagnostic.file
        };
        print_error(&Diagnostic::new(
            file,
            diagnostic.line,
            diagnostic.column,
            format!("{}: {}", diagnostic.code, diagnostic.message),
            DiagnosticKind::Compiler,
        ));
    }
}
