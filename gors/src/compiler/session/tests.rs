use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::compiler::input::SourceSnapshot;
use crate::compiler::provenance::SourceRef;

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
fn definition_source_table_rebinds_stable_reference_to_current_anchor() {
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
        .unwrap()
        .id();
    let source_ref = SourceRef::definition(target);
    let before = session
        .database
        .definition_source_table(file, target)
        .unwrap();
    let before_range = before.resolve(source_ref).unwrap();
    assert_eq!(
        before_range.range().start().to_usize(),
        source.find("target").unwrap()
    );

    let edited =
        "package main\n\nfunc prefix() {}\n\n// relocated\nfunc target() {}\nfunc main() {}\n";
    session
        .compile_program(raw_program("main.go", "main.go", edited))
        .unwrap();
    let after = session
        .database
        .definition_source_table(file, target)
        .unwrap();
    let after_range = after.resolve(source_ref).unwrap();

    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(
        after_range.range().start().to_usize(),
        edited.find("target").unwrap()
    );
    assert!(after_range.range().start() > before_range.range().start());
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
fn syntax_diagnostic_path_moves_without_semantic_reexecution() {
    let source = "package main\nfunc main( {\n";
    let mut session = CompilerSession::default();
    let first = session
        .compile_program(raw_program("main.go", "/checkout/one/main.go", source))
        .err()
        .expect("invalid source must fail");
    assert_eq!(
        first.diagnostics().first().unwrap().file,
        "/checkout/one/main.go"
    );

    session.database().reset_telemetry();
    let moved = session
        .compile_program(raw_program("main.go", "/checkout/two/main.go", source))
        .err()
        .expect("moved invalid source must fail");
    assert_eq!(
        moved.diagnostics().first().unwrap().file,
        "/checkout/two/main.go"
    );
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn relative_line_directive_uses_the_current_diagnostic_directory() {
    let source = "package main\n//line generated.go:40\nfunc main( {\n";
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(raw_program("main.go", "/checkout/project/main.go", source))
        .err()
        .expect("invalid source must fail");
    let diagnostic = error.diagnostics().first().unwrap();

    assert_eq!(diagnostic.file, "/checkout/project/generated.go");
    assert_eq!((diagnostic.line, diagnostic.column), (40, 0));
}

#[test]
fn rooted_and_uri_line_directive_names_are_presentation_independent() {
    for (directive, expected) in [
        ("/virtual/generated.go", "/virtual/generated.go"),
        (r"C:\virtual\generated.go", r"C:\virtual\generated.go"),
        ("mem://generated/pkg/main.go", "mem://generated/pkg/main.go"),
    ] {
        let source = format!("package main\n//line {directive}:40\nfunc main( {{\n");
        let mut session = CompilerSession::default();
        let error = session
            .compile_program(raw_program("main.go", "/checkout/project/main.go", &source))
            .err()
            .expect("invalid source must fail");
        let diagnostic = error.diagnostics().first().unwrap();

        assert_eq!(diagnostic.file, expected);
        assert_eq!((diagnostic.line, diagnostic.column), (40, 0));
    }
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
fn catalog_dependency_is_not_materialized_before_import_rejection() {
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
    assert_eq!(session.database().active_files().len(), 1);
    let packages = session
        .database()
        .active_files()
        .into_iter()
        .map(|file| session.database().package_for_file(file).unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(packages.len(), 1);
}

#[test]
fn huge_valid_and_invalid_catalog_packages_cost_nothing_beyond_entry() {
    let entry = super::super::input::PackageKey::command_line();
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
    let baseline_input =
        ProgramInput::new(test_workspace(), entry.clone(), [entry_manifest.clone()]).unwrap();
    let payload = "x".repeat(1024 * 1024);
    let valid = super::super::input::PackageKey::import_path("example/valid").unwrap();
    let valid_manifest = PackageInputManifest::new(
        valid,
        [super::super::input::SourceFileInput::from_source(
            "huge.go",
            "/checkout/valid/huge.go",
            format!("package valid\nvar Payload = `{payload}`\n"),
        )
        .unwrap()],
    )
    .unwrap();
    let invalid = super::super::input::PackageKey::import_path("example/invalid").unwrap();
    let invalid_manifest = PackageInputManifest::new(
        invalid,
        [super::super::input::SourceFileInput::from_source(
            "broken.go",
            "/checkout/invalid/broken.go",
            format!("package invalid\nfunc broken( {{\n// {payload}"),
        )
        .unwrap()],
    )
    .unwrap();
    let input = ProgramInput::new(
        test_workspace(),
        entry,
        [invalid_manifest, valid_manifest, entry_manifest],
    )
    .unwrap();
    let mut baseline = CompilerSession::default();
    baseline.compile_program(baseline_input).unwrap();
    let baseline_files = baseline.database().active_files();
    let baseline_bytes = baseline.database().retained_source_bytes();
    let baseline_telemetry = baseline.database().telemetry();
    let mut session = CompilerSession::default();

    session.compile_program(input).unwrap();

    assert_eq!(session.database().active_files(), baseline_files);
    assert_eq!(session.database().retained_source_bytes(), baseline_bytes);
    assert_eq!(session.database().telemetry(), baseline_telemetry);
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
    let retained_bytes = session.database().retained_source_bytes();
    session.database().reset_telemetry();

    session
        .compile_program(input("package unrelated\nfunc Value() int { return 2 }\n"))
        .unwrap();

    assert_eq!(session.database().telemetry().total_executions(), 0);
    assert_eq!(host.telemetry(), scheduler);
    assert_eq!(session.database().active_files().len(), 1);
    assert_eq!(session.database().retained_source_bytes(), retained_bytes);
}

#[test]
fn previous_entry_is_removed_when_retained_only_as_catalog_package() {
    let workspace = test_workspace();
    let previous_key =
        super::super::input::PackageKey::import_path("example/previous-entry").unwrap();
    let previous_manifest = PackageInputManifest::new(
        previous_key.clone(),
        [super::super::input::SourceFileInput::from_source(
            "previous.go",
            "/checkout/previous.go",
            "package main\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let first =
        ProgramInput::new(workspace.clone(), previous_key, [previous_manifest.clone()]).unwrap();
    let mut session = CompilerSession::default();
    session.compile_program(first).unwrap();
    let previous_file = *session.database().active_files().first().unwrap();
    let previous_package = session.database().package_for_file(previous_file).unwrap();

    let entry = super::super::input::PackageKey::command_line();
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
    let second = ProgramInput::new(workspace, entry, [previous_manifest, entry_manifest]).unwrap();

    session.compile_program(second).unwrap();

    let active = session.database().active_files();
    assert_eq!(active.len(), 1);
    assert_ne!(active.first().copied(), Some(previous_file));
    assert!(session.database().source_snapshot(previous_file).is_err());
    assert!(
        session
            .database()
            .analyze_package(previous_package)
            .is_err()
    );
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
