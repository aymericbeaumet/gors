use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::ids::SourceSpan;
use crate::compiler::input::SourceSnapshot;

use super::*;

fn test_workspace() -> WorkspaceKey {
    WorkspaceKey::ad_hoc("compiler-session-tests").unwrap()
}

fn raw_program(logical_path: &str, diagnostic_path: &str, source: &str) -> ProgramInput {
    let package = super::super::input::PackageKey::command_line();
    let file =
        super::super::input::SourceFileInput::from_source(logical_path, diagnostic_path, source)
            .unwrap();
    let manifest = PackageInputManifest::new(package.clone(), [file]).unwrap();
    ProgramInput::new(test_workspace(), package, [manifest]).unwrap()
}

#[test]
fn install_transaction_rolls_back_updated_inserted_and_stale_inputs() {
    let mut session = CompilerSession::default();
    let workspace = test_workspace();
    let package = super::super::input::PackageKey::command_line();
    let original = Arc::new(
        SourceSnapshot::from_source("/original/main.go", "package main\nfunc main() {}\n").unwrap(),
    );
    session
        .compile_program(raw_program(
            "main.go",
            "/original/main.go",
            "package main\nfunc main() {}\n",
        ))
        .unwrap();
    let main_file = *session.database.active_files().first().unwrap();
    let stale = Arc::new(
        SourceSnapshot::from_source(
            "/original/stale.go",
            "package main\nfunc stale() int { return 1 }\n",
        )
        .unwrap(),
    );
    let stale_file = session
        .database
        .set_source(&workspace, &package, "stale.go", Arc::clone(&stale))
        .unwrap()
        .file();
    let package_id = session.database.package_for_file(main_file).unwrap();
    let readiness = session.ready_rust_ir_roots.clone();
    assert!(!readiness.is_empty());

    let input = ProgramInput::new(
        workspace,
        package.clone(),
        [PackageInputManifest::new(
            package,
            [
                super::super::input::SourceFileInput::from_source(
                    "main.go",
                    "/failed/main.go",
                    "package main\nfunc main() { println(1) }\n",
                )
                .unwrap(),
                super::super::input::SourceFileInput::from_source(
                    "inserted.go",
                    "/failed/inserted.go",
                    "package main\nfunc inserted() {}\n",
                )
                .unwrap(),
            ],
        )
        .unwrap()],
    )
    .unwrap();

    let error = session
        .install_program_transaction(&input, |_| {
            Err(CompilerError::backend(
                "injected failure after stale-file deletion",
            ))
        })
        .err()
        .expect("the injected pre-commit failure must abort installation");

    assert!(error.to_string().contains("injected failure"));
    assert_eq!(
        session
            .database
            .active_files()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([main_file, stale_file])
    );
    assert_eq!(
        session
            .database
            .source_snapshot(main_file)
            .unwrap()
            .as_ref(),
        original.as_ref()
    );
    assert_eq!(
        session
            .database
            .source_snapshot(stale_file)
            .unwrap()
            .as_ref(),
        stale.as_ref()
    );
    assert_eq!(session.ready_rust_ir_roots, readiness);

    let restored = session.database.analyze_package(package_id).unwrap();
    assert!(restored.issues().is_empty());
    assert_eq!(
        restored
            .functions()
            .iter()
            .map(|function| function.name())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["main", "stale"])
    );
}

#[test]
fn install_rollback_restores_removed_package_and_discards_new_package() {
    let mut session = CompilerSession::default();
    let workspace = test_workspace();
    let old_package = super::super::input::PackageKey::import_path("example/old").unwrap();
    let old_snapshot = Arc::new(
        SourceSnapshot::from_source(
            "/original/old.go",
            "package old\nfunc Value() int { return 1 }\n",
        )
        .unwrap(),
    );
    let old_file = session
        .database
        .set_source(
            &workspace,
            &old_package,
            "old.go",
            Arc::clone(&old_snapshot),
        )
        .unwrap()
        .file();
    let old_package_id = session.database.package_for_file(old_file).unwrap();
    let new_package = super::super::input::PackageKey::command_line();
    let input = ProgramInput::new(
        workspace,
        new_package.clone(),
        [PackageInputManifest::new(
            new_package,
            [super::super::input::SourceFileInput::from_source(
                "main.go",
                "/failed/main.go",
                "package main\nfunc main() {}\n",
            )
            .unwrap()],
        )
        .unwrap()],
    )
    .unwrap();
    let new_package_id = std::cell::Cell::new(None);

    let error = session
        .install_program_transaction(&input, |database| {
            assert!(database.source_snapshot(old_file).is_err());
            let new_file = database
                .active_files()
                .into_iter()
                .find(|file| *file != old_file)
                .expect("the new package source must be active before commit");
            new_package_id.set(Some(database.package_for_file(new_file).unwrap()));
            Err(CompilerError::backend(
                "injected failure across package-map boundaries",
            ))
        })
        .err()
        .expect("the injected pre-commit failure must abort installation");

    assert!(error.to_string().contains("injected failure"));
    assert_eq!(session.database.active_files(), vec![old_file]);
    assert_eq!(
        session.database.source_snapshot(old_file).unwrap().as_ref(),
        old_snapshot.as_ref()
    );
    let restored = session.database.analyze_package(old_package_id).unwrap();
    assert!(restored.issues().is_empty());
    assert_eq!(
        restored.functions().first().map(|function| function.name()),
        Some("Value")
    );
    assert_eq!(restored.functions().len(), 1);
    assert!(
        session
            .database
            .analyze_package(new_package_id.get().unwrap())
            .is_err()
    );
}

#[test]
fn exact_noop_install_has_no_salsa_mutation_or_readiness_change() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc main() {}\n",
    );
    let mut session = CompilerSession::default();
    session.compile_program(program.clone()).unwrap();
    let active = session.database.active_files();
    let retained_bytes = session.database.retained_source_bytes();
    let readiness = session.ready_rust_ir_roots.clone();
    session.database.reset_telemetry();

    session.install_program(&program).unwrap();

    assert_eq!(session.database.active_files(), active);
    assert_eq!(session.database.retained_source_bytes(), retained_bytes);
    assert_eq!(session.ready_rust_ir_roots, readiness);
    assert_eq!(session.database.telemetry().total_executions(), 0);
    assert_eq!(
        session.database.telemetry().engine().cancellation_requests,
        0
    );
}

#[test]
fn function_relative_diagnostic_rebases_to_current_anchor() {
    let source = "package main\n\nfunc prefix() {}\n\nfunc target() {}\nfunc main() {}\n";
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program("main.go", "main.go", source))
        .unwrap();
    let file = session.database.active_files().first().copied().unwrap();
    let analysis = session.database.analyze_file(file).unwrap();
    let target = analysis
        .functions()
        .iter()
        .find(|function| function.name() == "target")
        .unwrap();
    let provenance = session
        .database
        .function_provenance(file, target.id())
        .unwrap();
    let mut diagnostic = super::super::Diagnostic {
        code: "GORS2003",
        message: "test diagnostic".to_string(),
        span: SourceSpan {
            file: "main.go".to_string(),
            start: 2,
            end: 4,
            line: 2,
            column: 3,
        },
    };

    rebase_function_diagnostic(&mut diagnostic, &provenance);

    assert_eq!(diagnostic.span.start, provenance.byte_offset() + 2);
    assert_eq!(diagnostic.span.end, provenance.byte_offset() + 4);
    assert_eq!(diagnostic.span.line, provenance.line() + 1);
    assert_eq!(diagnostic.span.column, 3);
}

#[test]
fn syntax_invalid_input_is_query_owned_and_repeated_revision_is_green() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc main( {\n",
    );
    let mut session = CompilerSession::default();

    let first = session
        .compile_program(program.clone())
        .err()
        .expect("syntax-invalid raw source must fail in the parse query");
    assert_eq!(first.diagnostics().first().unwrap().code, "GORS2002");
    assert_eq!(
        first.diagnostics().first().unwrap().file,
        "/checkout/main.go"
    );
    assert_eq!(session.database().active_files().len(), 1);

    session.database().reset_telemetry();
    let second = session
        .compile_program(program)
        .err()
        .expect("the unchanged invalid revision must remain invalid");
    assert_eq!(second, first);
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn source_map_plan_owns_entry_comments_across_session_revisions() {
    let mut session = CompilerSession::default();
    let (_, first_plan) = session
        .compile_program_with_source_map(raw_program(
            "main.go",
            "/checkout/first/main.go",
            "package main\nfunc main() {\n// first revision\n}\n",
        ))
        .unwrap();

    session
        .compile_program(raw_program(
            "main.go",
            "/checkout/second/main.go",
            "package main\nfunc main() {\n// second revision\n}\n",
        ))
        .unwrap();

    assert_eq!(first_plan.entry_source_name(), "/checkout/first/main.go");
    let comments = first_plan.entry_comments().comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments.first().unwrap().text(), "// first revision");
}

#[test]
fn every_manifest_package_is_installed_before_query_owned_import_rejection() {
    let entry = super::super::input::PackageKey::command_line();
    let dependency = super::super::input::PackageKey::import_path("example/dependency").unwrap();
    let entry_manifest = PackageInputManifest::new(
        entry.clone(),
        [super::super::input::SourceFileInput::from_source(
            "main.go",
            "/checkout/main.go",
            "package main\nimport \"example/dependency\"\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let dependency_manifest = PackageInputManifest::new(
        dependency,
        [super::super::input::SourceFileInput::from_source(
            "dependency.go",
            "/checkout/dependency/dependency.go",
            "package dependency\nfunc Value() int { return 1 }\n",
        )
        .unwrap()],
    )
    .unwrap();
    let input = ProgramInput::new(
        test_workspace(),
        entry,
        [dependency_manifest, entry_manifest],
    )
    .unwrap();
    let mut session = CompilerSession::default();

    let error = session
        .compile_program(input)
        .err()
        .expect("the bootstrap boundary must reject direct imports");

    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2001");
    assert!(error.to_string().contains("imports are not implemented"));
    assert_eq!(session.database().active_files().len(), 2);
    let packages = session
        .database()
        .active_files()
        .into_iter()
        .map(|file| session.database().package_for_file(file).unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(packages.len(), 2);
}

#[test]
fn unrelated_invalid_manifest_package_is_not_analyzed_eagerly() {
    let entry = super::super::input::PackageKey::command_line();
    let unrelated = super::super::input::PackageKey::import_path("example/unrelated").unwrap();
    let entry_manifest = PackageInputManifest::new(
        entry.clone(),
        [super::super::input::SourceFileInput::from_source(
            "main.go",
            "/checkout/main.go",
            "package main\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let unrelated_manifest = PackageInputManifest::new(
        unrelated,
        [super::super::input::SourceFileInput::from_source(
            "broken.go",
            "/checkout/unrelated/broken.go",
            "package unrelated\nfunc broken( {\n",
        )
        .unwrap()],
    )
    .unwrap();
    let input = ProgramInput::new(
        test_workspace(),
        entry,
        [unrelated_manifest, entry_manifest],
    )
    .unwrap();
    let mut session = CompilerSession::default();

    session.compile_program(input).unwrap();

    assert_eq!(session.database().active_files().len(), 2);
    let telemetry = session.database().telemetry();
    assert_eq!(
        telemetry.executions(super::super::db::QueryKind::PackageAnalysis),
        1
    );
    assert_eq!(
        telemetry.executions(super::super::db::QueryKind::FileProjection),
        1
    );
}

#[test]
fn unrelated_package_edit_preserves_entry_queries_and_scheduler_readiness() {
    fn input(unrelated_body: &str) -> ProgramInput {
        let entry = super::super::input::PackageKey::command_line();
        let unrelated = super::super::input::PackageKey::import_path("example/unrelated").unwrap();
        let entry_manifest = PackageInputManifest::new(
            entry.clone(),
            [super::super::input::SourceFileInput::from_source(
                "main.go",
                "/checkout/main.go",
                "package main\nfunc main() {}\n",
            )
            .unwrap()],
        )
        .unwrap();
        let unrelated_manifest = PackageInputManifest::new(
            unrelated,
            [super::super::input::SourceFileInput::from_source(
                "unrelated.go",
                "/checkout/unrelated/unrelated.go",
                unrelated_body,
            )
            .unwrap()],
        )
        .unwrap();
        ProgramInput::new(
            test_workspace(),
            entry,
            [unrelated_manifest, entry_manifest],
        )
        .unwrap()
    }

    let host = CompilerHost::new(NonZeroUsize::new(2).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    session
        .compile_program(input("package unrelated\nfunc Value() int { return 1 }\n"))
        .unwrap();
    let scheduler = host.telemetry();
    session.database().reset_telemetry();

    session
        .compile_program(input("package unrelated\nfunc Value() int { return 2 }\n"))
        .unwrap();

    assert_eq!(session.database().telemetry().total_executions(), 0);
    assert_eq!(host.telemetry(), scheduler);
}

#[test]
fn package_clause_edits_preserve_manifest_owned_file_package_and_definition_ids() {
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program(
            "main.go",
            "/checkout/main.go",
            "package main\nfunc helper() {}\nfunc main() {}\n",
        ))
        .unwrap();
    let first_file = session
        .database()
        .active_files()
        .first()
        .copied()
        .expect("compiled program must install its entry file");
    let first_package = session.database().package_for_file(first_file).unwrap();
    let first_functions = session
        .database()
        .analyze_package(first_package)
        .unwrap()
        .functions()
        .iter()
        .map(|function| (function.name().to_string(), function.id()))
        .collect::<BTreeMap<_, _>>();

    let error = session
        .compile_program(raw_program(
            "main.go",
            "/checkout/main.go",
            "package renamed\nfunc helper() {}\nfunc main() {}\n",
        ))
        .err()
        .expect("a non-main package remains outside the executable boundary");
    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2001");
    let second_file = session
        .database()
        .active_files()
        .first()
        .copied()
        .expect("edited program must retain its entry file");
    let second_package = session.database().package_for_file(second_file).unwrap();
    let second_functions = session
        .database()
        .analyze_package(second_package)
        .unwrap()
        .functions()
        .iter()
        .map(|function| (function.name().to_string(), function.id()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(second_file, first_file);
    assert_eq!(second_package, first_package);
    assert_eq!(second_functions, first_functions);
}
