//! Shared fuzzing operations.
//!
//! Keeping the operations outside the engine-specific target binaries lets the
//! stable test suite replay every checked-in corpus input on pull requests.

#![allow(clippy::panic)]

const MAX_INPUT_BYTES: usize = 256 * 1024;

fn source_from_bytes(data: &[u8]) -> Option<std::borrow::Cow<'_, str>> {
    if data.len() > MAX_INPUT_BYTES {
        return None;
    }
    Some(String::from_utf8_lossy(data))
}

/// Exercise the scanner over arbitrary bytes without requiring valid UTF-8.
pub fn exercise_scanner(data: &[u8]) {
    let Some(source) = source_from_bytes(data) else {
        return;
    };
    for token in gors::scanner::Scanner::new("fuzz.go", &source) {
        let _ = token;
    }
}

/// Exercise the parser over arbitrary bytes without treating syntax errors as
/// fuzzing failures.
pub fn exercise_parser(data: &[u8]) {
    let Some(source) = source_from_bytes(data) else {
        return;
    };
    let _ = gors::parser::parse_file("fuzz.go", &source);
}

/// Verify that independently parsing and printing the same source produces a
/// deterministic AST snapshot.
///
/// `ast::fprint` emits an AST dump rather than Go source, so its output cannot
/// be parsed as Go. Comparing two independent parses retains the useful
/// determinism property without asserting an invalid source round trip.
pub fn exercise_ast_snapshot(data: &[u8]) {
    let Some(source) = source_from_bytes(data) else {
        return;
    };
    let Ok(first_ast) = gors::parser::parse_file("fuzz.go", &source) else {
        return;
    };
    let Ok(second_ast) = gors::parser::parse_file("fuzz.go", &source) else {
        panic!("the parser accepted identical input only once");
    };

    let mut first = Vec::new();
    let mut second = Vec::new();
    if gors::ast::fprint(&mut first, first_ast).is_err()
        || gors::ast::fprint(&mut second, second_ast).is_err()
    {
        panic!("AST printing failed for an accepted Go source");
    }
    assert_eq!(first, second, "AST printing is not deterministic");
}

/// Exercise generic Go AST to Rust AST lowering and Rust source printing.
///
/// Compiler errors are valid outcomes for generated programs. Panics, aborts,
/// and memory-safety failures remain visible to the fuzzing engine.
pub fn exercise_compiler(data: &[u8]) {
    let Some(source) = source_from_bytes(data) else {
        return;
    };
    let package = gors::compiler::input::PackageKey::command_line();
    let Ok(file) =
        gors::compiler::input::SourceFileInput::from_source("fuzz.go", "fuzz.go", source.as_ref())
    else {
        return;
    };
    let Ok(manifest) = gors::compiler::input::PackageInputManifest::new(package.clone(), [file])
    else {
        return;
    };
    let Ok(program) = gors::compiler::input::ProgramInput::new(
        gors::compiler::input::WorkspaceKey::ad_hoc("fuzz-compiler").expect("stable fuzz key"),
        package,
        [manifest],
    ) else {
        return;
    };
    let Ok(compiled) = gors::compiler::compile_program(program) else {
        return;
    };
    if let Err(error) = gors::printer::generate_single(compiled) {
        panic!("Rust source printing failed after successful lowering: {error}");
    }
}
