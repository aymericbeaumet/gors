#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use gors::compiler::db::{BuildConfig, FileAnalysis, QueryKind, RuntimeAbiId};
use gors::compiler::fingerprint::Fingerprint;
use gors::compiler::ids::{DefId, FileId};
use gors::compiler::input::{
    PackageInputManifest, PackageKey, ProgramInput, SourceFileInput, WorkspaceKey,
};
use gors::compiler::provenance::SourceRef;
use gors::compiler::{CompilerHost, CompilerSession, SchedulerTelemetry};

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

const PARALLEL_PROGRAM: &str = r#"package main

func a() int { return 1 }
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func f() int { return 6 }
func g() int { return 7 }
func main() { println(g()) }
"#;

const PARALLEL_PROGRAM_COMMENT_EDIT: &str = r#"package main

// a remains semantically identical.
func a() int { return 1 }
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func f() int {
    // Moving the return anchor must not dirty the Rust-IR root.
    return 6
}
func g() int { return 7 }
func main() { println(g()) }
"#;

const PARALLEL_PROGRAM_EDIT: &str = r#"package main

// a remains semantically identical.
func a() int { return 1 }
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func f() int {
    // Moving the return anchor must not dirty the Rust-IR root.
    return 60
}
func g() int { return 7 }
func main() { println(g()) }
"#;

const PARALLEL_PROGRAM_RENAMED: &str = r#"package main

func a() int { return 1 }
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func z() int { return 6 }
func g() int { return 7 }
func main() { println(g()) }
"#;

const PARALLEL_PROGRAM_API_EDIT: &str = r#"package main

func a() int { return 1 }
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func f() int { return 6 }
func g() string { return "seven" }
func main() { println(g()) }
"#;

const PARALLEL_STAGE_FAILURES: &str = r#"package main

func a() int {}
func b() int { return 2 }
func c() int { return 3 }
func d() int { return 4 }
func e() int { return 5 }
func f() string {}
func g() int { return 7 }
func main() { println(g()) }
"#;

fn program(path: &str, source: &str) -> ProgramInput {
    program_file("main.go", path, source)
}

fn program_file(logical_path: &str, diagnostic_path: &str, source: &str) -> ProgramInput {
    let package = PackageKey::command_line();
    let manifest = PackageInputManifest::new(
        package.clone(),
        [SourceFileInput::from_source(logical_path, diagnostic_path, source).unwrap()],
    )
    .unwrap();
    ProgramInput::new(
        WorkspaceKey::ad_hoc("compiler-session-integration-tests").unwrap(),
        package,
        [manifest],
    )
    .unwrap()
}

fn only_file(session: &CompilerSession) -> FileId {
    let files = session.database().active_files();
    assert_eq!(files.len(), 1);
    *files.first().unwrap()
}

fn functions(analysis: &FileAnalysis) -> BTreeMap<String, DefId> {
    analysis
        .functions()
        .iter()
        .map(|function| (function.name().to_string(), function.id()))
        .collect()
}

fn generate_single_source(compiled: gors::compiler::CompiledProgram) -> String {
    let mut generated = gors::printer::generate_single(compiled).unwrap();
    assert_eq!(generated.files.len(), 1);
    generated.files.remove("main.rs").unwrap()
}

fn parallel_evidence(jobs: usize) -> (String, Vec<u8>, Fingerprint, SchedulerTelemetry) {
    let host = CompilerHost::new(NonZeroUsize::new(jobs).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    let (compiled, plan) = session
        .compile_program_with_source_map(program("main.go", PARALLEL_PROGRAM))
        .unwrap();
    let rust = generate_single_source(compiled);
    let mut source_map = Vec::new();
    plan.build(&rust).to_writer(&mut source_map).unwrap();
    let file = only_file(&session);
    let package = session.database().package_for_file(file).unwrap();
    let fingerprint = session
        .database()
        .verified_rust_ir_package(package)
        .unwrap()
        .fingerprint();
    (rust, source_map, fingerprint, host.telemetry())
}

#[test]
fn ordinary_sessions_are_inline_until_parallelism_is_explicit() {
    let session = CompilerSession::default();
    assert_eq!(session.job_budget(), NonZeroUsize::MIN);
    assert_eq!(session.scheduler_telemetry(), SchedulerTelemetry::default());

    let session = CompilerSession::new(BuildConfig::default()).unwrap();
    assert_eq!(session.job_budget(), NonZeroUsize::MIN);
    assert_eq!(session.scheduler_telemetry(), SchedulerTelemetry::default());
}

#[test]
fn worker_counts_preserve_output_source_map_and_stage_fingerprint() {
    let serial = parallel_evidence(1);
    let two_workers = parallel_evidence(2);
    let four_workers = parallel_evidence(4);

    assert_eq!(
        (&serial.0, &serial.1, serial.2),
        (&two_workers.0, &two_workers.1, two_workers.2)
    );
    assert_eq!(
        (&serial.0, &serial.1, serial.2),
        (&four_workers.0, &four_workers.1, four_workers.2)
    );
    assert_eq!(serial.3.serial_waves, 1);
    assert_eq!(serial.3.snapshots_created, 0);
    assert_eq!(serial.3.pool_starts, 0);
    assert_eq!(two_workers.3.parallel_waves, 1);
    assert_eq!(two_workers.3.snapshots_created, 2);
    assert_eq!(two_workers.3.peak_workers, 2);
    assert_eq!(four_workers.3.parallel_waves, 1);
    assert_eq!(four_workers.3.snapshots_created, 4);
    assert_eq!(four_workers.3.peak_workers, 4);
}

#[test]
fn warm_scheduler_skips_comments_and_fans_out_only_changed_root_inputs() {
    let host = CompilerHost::new(NonZeroUsize::new(4).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    session
        .compile_program(program("main.go", PARALLEL_PROGRAM))
        .unwrap();
    let cold = host.telemetry();
    assert_eq!(cold.scheduled_roots, 8);
    assert_eq!(cold.parallel_waves, 1);
    assert_eq!(cold.pool_starts, 1);

    session.database().reset_telemetry();
    session
        .compile_program(program("main.go", PARALLEL_PROGRAM))
        .unwrap();
    assert_eq!(session.database().telemetry().total_executions(), 0);
    assert_eq!(host.telemetry(), cold);

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM_COMMENT_EDIT))
        .unwrap();
    assert_eq!(host.telemetry(), cold);

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM_EDIT))
        .unwrap();
    let edited = host.telemetry();
    assert_eq!(edited.scheduled_roots, 9);
    assert_eq!(edited.parallel_waves, 1);
    assert_eq!(edited.serial_waves, 1);
    assert_eq!(edited.pool_starts, 1);

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM_EDIT))
        .unwrap();
    assert_eq!(host.telemetry(), edited);
}

#[test]
fn renamed_roots_are_pruned_instead_of_reusing_stale_readiness() {
    let host = CompilerHost::new(NonZeroUsize::new(4).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    session
        .compile_program(program("main.go", PARALLEL_PROGRAM))
        .unwrap();

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM_RENAMED))
        .unwrap();
    assert_eq!(host.telemetry().scheduled_roots, 9);

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM))
        .unwrap();
    let restored = host.telemetry();
    assert_eq!(restored.scheduled_roots, 10);
    assert_eq!(restored.serial_waves, 2);
}

#[test]
fn direct_callee_api_edits_schedule_the_callee_and_its_caller() {
    let host = CompilerHost::new(NonZeroUsize::new(4).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    session
        .compile_program(program("main.go", PARALLEL_PROGRAM))
        .unwrap();

    session
        .compile_program(program("main.go", PARALLEL_PROGRAM_API_EDIT))
        .unwrap();

    let edited = host.telemetry();
    assert_eq!(edited.scheduled_roots, 10);
    assert_eq!(edited.parallel_waves, 1);
    assert_eq!(edited.serial_waves, 1);
}

#[test]
fn checkout_root_move_reuses_semantics_and_republishes_source_location() {
    let host = CompilerHost::new(NonZeroUsize::new(4).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    let (first_compiled, first_plan) = session
        .compile_program_with_source_map(program("/checkout/one/main.go", PARALLEL_PROGRAM))
        .unwrap();
    let first_rust = generate_single_source(first_compiled);
    let first_scheduler = host.telemetry();
    session.database().reset_telemetry();

    let (second_compiled, second_plan) = session
        .compile_program_with_source_map(program("/checkout/two/main.go", PARALLEL_PROGRAM))
        .unwrap();
    let second_rust = generate_single_source(second_compiled);
    let first_map = first_plan.build(&first_rust);
    let second_map = second_plan.build(&second_rust);

    assert_eq!(second_rust, first_rust);
    assert_eq!(first_map.get_source(0), Some("/checkout/one/main.go"));
    assert_eq!(second_map.get_source(0), Some("/checkout/two/main.go"));
    assert_eq!(session.database().telemetry().total_executions(), 0);
    assert_eq!(session.database().telemetry().engine().will_execute, 0);
    assert_eq!(
        session
            .database()
            .telemetry()
            .engine()
            .cancellation_requests,
        0
    );
    assert_eq!(host.telemetry(), first_scheduler);
    let file = only_file(&session);
    assert_eq!(
        session
            .database()
            .source_snapshot(file)
            .unwrap()
            .diagnostic_path(),
        "/checkout/two/main.go"
    );
}

#[test]
fn checkout_root_move_reuses_failed_roots_but_repaints_the_error_path() {
    let host = CompilerHost::new(NonZeroUsize::new(4).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    let first_error = session
        .compile_program(program("/checkout/one/main.go", PARALLEL_STAGE_FAILURES))
        .err()
        .expect("missing returns must fail MIR construction");
    let first_scheduler = host.telemetry();
    session.database().reset_telemetry();

    let second_error = session
        .compile_program(program("/checkout/two/main.go", PARALLEL_STAGE_FAILURES))
        .err()
        .expect("the unchanged invalid program must still fail");

    assert_eq!(
        second_error.diagnostics().len(),
        first_error.diagnostics().len()
    );
    for (first, second) in first_error
        .diagnostics()
        .iter()
        .zip(second_error.diagnostics())
    {
        assert_eq!(second.code, first.code);
        assert_eq!(second.message, first.message);
        assert_eq!(second.line, first.line);
        assert_eq!(second.column, first.column);
        assert_eq!(first.file, "/checkout/one/main.go");
        assert_eq!(second.file, "/checkout/two/main.go");
    }
    assert_eq!(session.database().telemetry().total_executions(), 0);
    assert_eq!(session.database().telemetry().engine().will_execute, 0);
    assert_eq!(
        session
            .database()
            .telemetry()
            .engine()
            .cancellation_requests,
        0
    );
    assert_eq!(host.telemetry(), first_scheduler);
}

#[test]
fn canonical_package_root_selects_the_same_error_for_every_worker_count() {
    let compile = |jobs| {
        let host = CompilerHost::new(NonZeroUsize::new(jobs).unwrap()).unwrap();
        let mut session = host.session(BuildConfig::default()).unwrap();
        let error = session
            .compile_program(program("main.go", PARALLEL_STAGE_FAILURES))
            .err()
            .expect("functions missing returns must fail MIR construction");
        (error, host.telemetry())
    };

    let serial = compile(1);
    let two_workers = compile(2);
    let four_workers = compile(4);
    assert_eq!(two_workers.0, serial.0);
    assert_eq!(four_workers.0, serial.0);
    assert_eq!(serial.1.serial_waves, 1, "{}", serial.0);
    assert_eq!(two_workers.1.parallel_waves, 1);
    assert_eq!(four_workers.1.parallel_waves, 1);
    assert!(serial.0.to_string().contains("without returning"));
}

#[test]
fn production_session_rejects_an_unsupported_runtime_contract() {
    let unsupported = BuildConfig::new(
        gors::GO_VERSION,
        RuntimeAbiId::from_contract_hash([0x99; 32]),
    );
    let error = CompilerSession::new(unsupported.clone())
        .err()
        .expect("a production session must reject an unsupported runtime contract");
    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2003");
    assert!(error.to_string().contains(&"99".repeat(32)));
    assert!(
        error
            .to_string()
            .contains(&RuntimeAbiId::current().to_string())
    );

    let mut session = CompilerSession::default();
    let error = session
        .set_build_config(unsupported)
        .expect_err("reconfiguration must enforce the same packaging invariant");
    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2003");
    assert_eq!(
        session.database().build_config().unwrap().runtime_abi(),
        RuntimeAbiId::current()
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
    let g = *ids.get("g").unwrap();
    let before_package = session.database().analyze_package(package).unwrap();
    let before_public = session.database().public_api(file).unwrap();
    let before_hir = session.database().typed_hir(file, g).unwrap();
    let source_ref = SourceRef::definition(g);
    let before_source_table = session.database().definition_source_table(file, g).unwrap();
    let before_range = before_source_table.resolve(source_ref).unwrap();
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
    let after_source_table = session.database().definition_source_table(file, g).unwrap();
    let after_range = after_source_table.resolve(source_ref).unwrap();
    let after_mir = session.database().verified_mir(file, g).unwrap();
    let after_normalized = session.database().normalized_mir(file, g).unwrap();
    let after_rust = session.database().verified_rust_ir(file, g).unwrap();
    assert!(Arc::ptr_eq(&before_package, &after_package));
    assert!(Arc::ptr_eq(&before_public, &after_public));
    assert!(Arc::ptr_eq(&before_hir, &after_hir));
    assert!(Arc::ptr_eq(&before_mir, &after_mir));
    assert!(Arc::ptr_eq(&before_normalized, &after_normalized));
    assert!(Arc::ptr_eq(&before_rust, &after_rust));
    assert!(!Arc::ptr_eq(&before_source_table, &after_source_table));
    assert!(after_range.range().start() > before_range.range().start());

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
    let g = *functions(&session.database().analyze_file(file).unwrap())
        .get("g")
        .unwrap();
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
        generate_single_source(first_output),
        generate_single_source(second_output)
    );
}

#[test]
fn production_package_index_rejects_cross_file_issues_before_codegen() {
    let package = PackageKey::command_line();
    let manifest = PackageInputManifest::new(
        package.clone(),
        [
            SourceFileInput::from_source(
                "a.go",
                "/checkout/a.go",
                "package main\nfunc duplicate() {}\nfunc main() {}\n",
            )
            .unwrap(),
            SourceFileInput::from_source(
                "b.go",
                "/checkout/b.go",
                "package main\nfunc duplicate() {}\n",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let input = ProgramInput::new(
        WorkspaceKey::ad_hoc("compiler-session-integration-tests").unwrap(),
        package,
        [manifest],
    )
    .unwrap();
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(input)
        .err()
        .expect("duplicate package definitions should fail compilation");
    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2002");
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
        .compile_program(program_file(
            "old.go",
            "/first/old.go",
            "package main\nfunc main() {}\n",
        ))
        .unwrap();
    assert_eq!(session.database().active_files().len(), 1);

    let invalid = program_file(
        "bad.go",
        "/failed/bad.go",
        "package main\nfunc broken() { switch {} }\nfunc main() {}\n",
    );
    let error = session
        .compile_program(invalid)
        .err()
        .expect("unsupported revision must fail");
    assert_eq!(error.diagnostics().first().unwrap().file, "/failed/bad.go");
    assert_eq!(session.database().active_files().len(), 1);

    session
        .compile_program(program_file(
            "current.go",
            "/recovered/current.go",
            "package main\nfunc main() { println(1) }\n",
        ))
        .unwrap();
    let active = session.database().active_files();
    assert_eq!(active.len(), 1);
    let active_file = *active.first().unwrap();
    assert_eq!(
        session
            .database()
            .source_snapshot(active_file)
            .unwrap()
            .diagnostic_path(),
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
    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.file, "/checkout/current/main.go");
    assert_eq!(diagnostic.line, 8);
    assert_eq!(diagnostic.column, 5);
}

#[test]
fn semantic_diagnostics_project_line_directives_only_at_publication() {
    let source =
        "package main\n//line generated.go:40\nfunc (value int) method() {}\nfunc main() {}\n";
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(program("/checkout/current/main.go", source))
        .err()
        .expect("methods are outside the bootstrap frontier");
    let diagnostic = error
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.message.contains("methods are not implemented"))
        .expect("method diagnostic");

    assert_eq!(diagnostic.file, "/checkout/current/generated.go");
    assert_eq!(diagnostic.line, 40);
    assert_eq!(diagnostic.column, 0);
}

#[test]
fn source_map_tracks_non_main_function_through_its_generated_symbol() {
    let mut session = CompilerSession::default();
    let (compiled, plan) = session
        .compile_program_with_source_map(program("main.go", ORIGINAL))
        .unwrap();
    let rust = generate_single_source(compiled);
    let source_map = plan.build(&rust);
    let token = source_map
        .tokens()
        .find(|token| token.get_name() == Some("f"))
        .expect("non-main generated symbol must retain the Go display name");
    assert_eq!(token.get_src_line(), 2);
    assert_eq!(token.get_src_col(), 5);
    assert!(rust.lines().nth(token.get_dst_line() as usize).is_some());
}

#[test]
fn source_maps_use_physical_sources_even_with_line_directives() {
    let source = "package main\n//line virtual.go:400\nfunc helper() int { return 1 }\nfunc main() { println(helper()) }\n";
    let mut session = CompilerSession::default();
    let (compiled, plan) = session
        .compile_program_with_source_map(program("/checkout/main.go", source))
        .unwrap();
    let rust = generate_single_source(compiled);
    let source_map = plan.build(&rust);
    let token = source_map
        .tokens()
        .find(|token| token.get_name() == Some("helper"))
        .expect("helper mapping");

    assert_eq!(source_map.get_source(0), Some("/checkout/main.go"));
    assert_eq!(token.get_src_line(), 2);
    assert_eq!(token.get_src_col(), 5);
}
