#![allow(clippy::panic)]

use std::path::{Path, PathBuf};

fn corpus_files(target: &str) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(target);
    let mut files = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("cannot read corpus entry: {error}"))
                .path()
        })
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "empty {target} corpus");
    files
}

fn replay(target: &str, exercise: fn(&[u8])) {
    for path in corpus_files(target) {
        let data = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        exercise(&data);
    }
}

#[test]
fn scanner_corpus_does_not_panic() {
    replay("scanner", fuzz::exercise_scanner);
}

#[test]
fn parser_corpus_does_not_panic() {
    replay("parser", fuzz::exercise_parser);
}

#[test]
fn ast_snapshot_corpus_is_deterministic() {
    replay("roundtrip", fuzz::exercise_ast_snapshot);
}

#[test]
fn compiler_corpus_either_reports_diagnostics_or_prints_rust() {
    for path in corpus_files("compiler") {
        let data = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let source = String::from_utf8_lossy(&data);
        let package = gors::compiler::input::PackageKey::command_line();
        let file = gors::compiler::input::SourceFileInput::from_source(
            "fuzz.go",
            "fuzz.go",
            source.as_ref(),
        )
        .unwrap_or_else(|error| panic!("cannot construct compiler corpus source input: {error}"));
        let manifest = gors::compiler::input::PackageInputManifest::new(package.clone(), [file])
            .unwrap_or_else(|error| panic!("cannot construct compiler corpus package: {error}"));
        let workspace = gors::compiler::input::WorkspaceKey::ad_hoc("fuzz-corpus")
            .unwrap_or_else(|error| panic!("cannot construct compiler corpus workspace: {error}"));
        let program = gors::compiler::input::ProgramInput::new(workspace, package, [manifest])
            .unwrap_or_else(|error| panic!("cannot construct compiler corpus program: {error}"));
        let compiled = match gors::compiler::compile_program(program) {
            Ok(compiled) => compiled,
            Err(error) => {
                assert!(
                    !error.diagnostics().is_empty(),
                    "compiler corpus seed {} failed without a diagnostic",
                    path.display()
                );
                continue;
            }
        };
        let generated = gors::printer::generate_single(compiled).unwrap_or_else(|error| {
            panic!(
                "compiler corpus seed {} does not print: {error}",
                path.display()
            );
        });
        let rust_source = generated.files.get("main.rs").unwrap_or_else(|| {
            panic!("compiler corpus seed {} printed no main.rs", path.display())
        });
        assert!(
            !rust_source.is_empty(),
            "compiler corpus seed {} printed empty Rust source",
            path.display()
        );
    }
}
