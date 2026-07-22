#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use gors::compiler::CompilerSession;
use gors::compiler::db::{BuildConfig, FileAnalysis, QueryKind};
use gors::compiler::ids::{DefId, FileId};

const ORIGINAL: &str = r#"package main

func f() string { return "a" }
func g() int { return 7 }
func main() { println(g()) }
"#;

const DIFFERENT_LENGTH_BODY: &str = r#"package main

func f() string { return "a much longer replacement" }
func g() int { return 7 }
func main() { println(g()) }
"#;

fn program(path: &str, source: &str) -> gors::parser::ParsedProgram {
    gors::parser::parse_program_from_source(path, source).unwrap()
}

fn only_file(session: &CompilerSession) -> FileId {
    let files = session.database().active_files();
    assert_eq!(files.len(), 1);
    files[0]
}

fn functions(analysis: &FileAnalysis) -> BTreeMap<String, DefId> {
    analysis
        .functions()
        .iter()
        .map(|function| (function.name().to_string(), function.id()))
        .collect()
}

#[test]
fn production_executes_the_query_spine_and_noop_reuses_every_stage() {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    let first = session.database().telemetry();
    for stage in [
        QueryKind::FileProjection,
        QueryKind::SemanticFile,
        QueryKind::TypedHir,
        QueryKind::VerifiedGoMir,
        QueryKind::NormalizedGoMir,
        QueryKind::VerifiedRustIr,
        QueryKind::RustIrPackage,
    ] {
        assert!(
            first.executions(stage) > 0,
            "missing production stage {stage:?}"
        );
    }

    session.database().reset_telemetry();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn different_length_private_body_edit_keeps_unrelated_products_green() {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    let file = only_file(&session);
    let package = session.database().package_for_file(file).unwrap();
    let ids = functions(&session.database().analyze_file(file).unwrap());
    let g = ids["g"];
    let before_package = session.database().analyze_package(package).unwrap();
    let before_public = session.database().public_api(file).unwrap();
    let before_hir = session.database().typed_hir(file, g).unwrap();
    let before_mir = session.database().verified_mir(file, g).unwrap();
    let before_normalized = session.database().normalized_mir(file, g).unwrap();
    let before_rust = session.database().verified_rust_ir(file, g).unwrap();

    session.database().reset_telemetry();
    session
        .compile_program(program("main.go", DIFFERENT_LENGTH_BODY))
        .unwrap();

    let after_package = session.database().analyze_package(package).unwrap();
    let after_public = session.database().public_api(file).unwrap();
    let after_hir = session.database().typed_hir(file, g).unwrap();
    let after_mir = session.database().verified_mir(file, g).unwrap();
    let after_normalized = session.database().normalized_mir(file, g).unwrap();
    let after_rust = session.database().verified_rust_ir(file, g).unwrap();
    assert!(Arc::ptr_eq(&before_package, &after_package));
    assert!(Arc::ptr_eq(&before_public, &after_public));
    assert!(Arc::ptr_eq(&before_hir, &after_hir));
    assert!(Arc::ptr_eq(&before_mir, &after_mir));
    assert!(Arc::ptr_eq(&before_normalized, &after_normalized));
    assert!(Arc::ptr_eq(&before_rust, &after_rust));

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::SemanticFile), 1);
    assert_eq!(telemetry.executions(QueryKind::PackageAnalysis), 0);
    assert_eq!(telemetry.executions(QueryKind::PackageSignatures), 0);
}

#[test]
fn target_change_invalidates_representation_but_not_go_semantics() {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    let file = only_file(&session);
    let ids = functions(&session.database().analyze_file(file).unwrap());
    let g = ids["g"];
    let before_hir = session.database().typed_hir(file, g).unwrap();
    let before_mir = session.database().verified_mir(file, g).unwrap();
    let before_normalized = session.database().normalized_mir(file, g).unwrap();
    let before_rust = session.database().verified_rust_ir(file, g).unwrap();

    session
        .set_build_config(BuildConfig::new(
            "rust-source-test-target",
            gors::GO_VERSION,
            gors::RUNTIME_ABI_ID,
        ))
        .unwrap();
    session.database().reset_telemetry();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();

    let after_hir = session.database().typed_hir(file, g).unwrap();
    let after_mir = session.database().verified_mir(file, g).unwrap();
    let after_normalized = session.database().normalized_mir(file, g).unwrap();
    let after_rust = session.database().verified_rust_ir(file, g).unwrap();
    assert!(Arc::ptr_eq(&before_hir, &after_hir));
    assert!(Arc::ptr_eq(&before_mir, &after_mir));
    assert!(Arc::ptr_eq(&before_normalized, &after_normalized));
    assert!(!Arc::ptr_eq(&before_rust, &after_rust));

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 0);
    assert_eq!(telemetry.executions(QueryKind::SemanticFile), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert!(telemetry.executions(QueryKind::VerifiedRustIr) > 0);
}

#[test]
fn checkout_path_does_not_change_ids_or_generated_output() {
    let mut first = CompilerSession::default();
    let mut second = CompilerSession::default();
    let first_output = first
        .compile_program(program("/checkout/one/main.go", ORIGINAL))
        .unwrap();
    let second_output = second
        .compile_program(program("/other/root/main.go", ORIGINAL))
        .unwrap();
    let first_file = only_file(&first);
    let second_file = only_file(&second);
    assert_eq!(first_file, second_file);
    assert_eq!(
        functions(&first.database().analyze_file(first_file).unwrap()),
        functions(&second.database().analyze_file(second_file).unwrap())
    );
    assert_eq!(
        gors::printer::generate_single(first_output).unwrap(),
        gors::printer::generate_single(second_output).unwrap()
    );
}

#[test]
fn stateful_snapshots_query_verified_products_in_parallel() {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    let file = only_file(&session);
    let g = functions(&session.database().analyze_file(file).unwrap())["g"];
    let first = session.database().snapshot();
    let second = session.database().snapshot();
    let first_worker = std::thread::spawn(move || first.verified_rust_ir(file, g).unwrap());
    let second_worker = std::thread::spawn(move || second.verified_rust_ir(file, g).unwrap());
    let first = first_worker.join().unwrap();
    let second = second_worker.join().unwrap();
    assert_eq!(first, second);
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn production_package_index_rejects_cross_file_issues_before_codegen() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("a.go"),
        "package main\nfunc duplicate() {}\nfunc main() {}\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("b.go"),
        "package main\nfunc duplicate() {}\n",
    )
    .unwrap();
    let parsed = gors::parser::parse_program(&directory.path().to_string_lossy()).unwrap();
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(parsed)
        .err()
        .expect("duplicate package definitions should fail compilation");
    assert_eq!(error.diagnostics()[0].code, "GORS2002");
    assert!(error.to_string().contains("duplicate package definition"));
    assert_eq!(
        session
            .database()
            .telemetry()
            .executions(QueryKind::PackageAnalysis),
        1
    );
    assert_eq!(
        session
            .database()
            .telemetry()
            .executions(QueryKind::VerifiedRustIr),
        0
    );
}
