use gors::error::Diagnostic;

/// Print a formatted error with source context.
pub fn print_error(diagnostic: &Diagnostic) {
    let use_colors = atty::is(atty::Stream::Stderr);
    eprint!("{}", diagnostic.format_terminal(use_colors));
}
