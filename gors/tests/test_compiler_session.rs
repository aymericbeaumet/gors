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

func f() string {
    // A new line before every unchanged sibling must not poison its semantic key.
    return "a much longer replacement"
}
func g() int { return 7 }
func main() { println(g()) }
"#;

const UNRELATED_DECLARATION_INSERTION: &str = r#"package main

func f() string { return "a" }
func h() bool { return true }
func g() int { return 7 }
func main() { println(g()) }
"#;

const UNRELATED_SIGNATURE_EDIT: &str = r#"package main

func f(unused int) string { return "a" }
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
fn production_session_rejects_runtime_abi_without_packaging_support() {
    let unsupported = BuildConfig::new("rust-source", gors::GO_VERSION, "synthetic-runtime-v99");
    let error = CompilerSession::new(unsupported.clone())
        .err()
        .expect("a production session must reject an unpackaged runtime ABI");
    assert_eq!(error.diagnostics()[0].code, "GORS2003");
    assert!(error.to_string().contains("synthetic-runtime-v99"));
    assert!(error.to_string().contains(gors::RUNTIME_ABI_ID));

    let mut session = CompilerSession::default();
    let error = session
        .set_build_config(unsupported)
        .expect_err("reconfiguration must enforce the same packaging invariant");
    assert_eq!(error.diagnostics()[0].code, "GORS2003");
    assert_eq!(
        session.database().build_config().unwrap().runtime_abi(),
        gors::RUNTIME_ABI_ID
    );
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
    let before_provenance = session.database().function_provenance(file, g).unwrap();
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
    let after_provenance = session.database().function_provenance(file, g).unwrap();
    let after_mir = session.database().verified_mir(file, g).unwrap();
    let after_normalized = session.database().normalized_mir(file, g).unwrap();
    let after_rust = session.database().verified_rust_ir(file, g).unwrap();
    assert!(Arc::ptr_eq(&before_package, &after_package));
    assert!(Arc::ptr_eq(&before_public, &after_public));
    assert!(Arc::ptr_eq(&before_hir, &after_hir));
    assert!(Arc::ptr_eq(&before_mir, &after_mir));
    assert!(Arc::ptr_eq(&before_normalized, &after_normalized));
    assert!(Arc::ptr_eq(&before_rust, &after_rust));
    assert!(!Arc::ptr_eq(&before_provenance, &after_provenance));
    assert!(after_provenance.byte_offset() > before_provenance.byte_offset());
    assert!(after_provenance.line() > before_provenance.line());

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::SemanticFile), 1);
    assert_eq!(telemetry.executions(QueryKind::PackageAnalysis), 0);
    assert_eq!(telemetry.executions(QueryKind::PackageFunctionLookup), 0);
}

#[test]
fn unrelated_declaration_insertion_keeps_existing_leaf_products_green() {
    assert_unrelated_edit_keeps_g_green(UNRELATED_DECLARATION_INSERTION, true);
}

#[test]
fn unrelated_signature_edit_keeps_existing_leaf_products_green() {
    assert_unrelated_edit_keeps_g_green(UNRELATED_SIGNATURE_EDIT, false);
}

fn assert_unrelated_edit_keeps_g_green(edited: &str, inserts_declaration: bool) {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("main.go", ORIGINAL))
        .unwrap();
    let file = only_file(&session);
    let g = functions(&session.database().analyze_file(file).unwrap())["g"];
    let before_signature = session.database().typed_signature(file, g).unwrap();
    let before_hir = session.database().typed_hir(file, g).unwrap();
    let before_mir = session.database().verified_mir(file, g).unwrap();
    let before_normalized = session.database().normalized_mir(file, g).unwrap();
    let before_rust = session.database().verified_rust_ir(file, g).unwrap();

    session.database().reset_telemetry();
    session.compile_program(program("main.go", edited)).unwrap();
    let after_signature = session.database().typed_signature(file, g).unwrap();
    let after_hir = session.database().typed_hir(file, g).unwrap();
    let after_mir = session.database().verified_mir(file, g).unwrap();
    let after_normalized = session.database().normalized_mir(file, g).unwrap();
    let after_rust = session.database().verified_rust_ir(file, g).unwrap();
    assert!(Arc::ptr_eq(&before_signature, &after_signature));
    assert!(Arc::ptr_eq(&before_hir, &after_hir));
    assert!(Arc::ptr_eq(&before_mir, &after_mir));
    assert!(Arc::ptr_eq(&before_normalized, &after_normalized));
    assert!(Arc::ptr_eq(&before_rust, &after_rust));

    let telemetry = session.database().telemetry();
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
    if inserts_declaration {
        assert!(telemetry.executions(QueryKind::PackageFunctionLookup) > 0);
    }
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

#[test]
fn failed_revision_does_not_leave_orphan_inputs_and_next_revision_recovers() {
    let mut session = CompilerSession::default();
    session
        .compile_program(program("/first/old.go", "package main\nfunc main() {}\n"))
        .unwrap();
    assert_eq!(session.database().active_files().len(), 1);

    let invalid = program(
        "/failed/bad.go",
        "package main\nfunc broken() { switch {} }\nfunc main() {}\n",
    );
    let error = session
        .compile_program(invalid)
        .err()
        .expect("unsupported revision must fail");
    assert_eq!(error.diagnostics()[0].file, "/failed/bad.go");
    assert_eq!(session.database().active_files().len(), 1);

    session
        .compile_program(program(
            "/recovered/current.go",
            "package main\nfunc main() { println(1) }\n",
        ))
        .unwrap();
    let active = session.database().active_files();
    assert_eq!(active.len(), 1);
    assert_eq!(
        session
            .database()
            .source_snapshot(active[0])
            .unwrap()
            .path(),
        "/recovered/current.go"
    );
}

#[test]
fn semantic_diagnostics_publish_current_original_path_and_position() {
    let source = r#"package main

func prefix() {
    println(1)
}

func broken() {
    switch {}
}

func main() {}
"#;
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(program("/checkout/current/main.go", source))
        .err()
        .expect("switch is outside the bootstrap frontier");
    let diagnostic = &error.diagnostics()[0];
    assert_eq!(diagnostic.file, "/checkout/current/main.go");
    assert_eq!(diagnostic.line, 8);
    assert_eq!(diagnostic.column, 5);
}

#[test]
fn source_map_tracks_non_main_function_through_its_generated_symbol() {
    let mut session = CompilerSession::default();
    let (compiled, plan) = session
        .compile_program_with_source_map(program("main.go", ORIGINAL))
        .unwrap();
    let rust = gors::printer::generate_single(compiled).unwrap();
    let source_map = plan.build(&rust);
    let token = source_map
        .tokens()
        .find(|token| token.get_name() == Some("f"))
        .expect("non-main generated symbol must retain the Go display name");
    assert_eq!(token.get_src_line(), 2);
    assert_eq!(token.get_src_col(), 5);
    assert!(rust.lines().nth(token.get_dst_line() as usize).is_some());
}
