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
fn install_rollback_restores_updated_inputs_and_removes_orphans() {
    let mut session = CompilerSession::default();
    let workspace = test_workspace();
    let package = super::super::input::PackageKey::command_line();
    let original = Arc::new(
        SourceSnapshot::from_source("/original/main.go", "package main\nfunc main() {}\n").unwrap(),
    );
    let file = session
        .database
        .set_source(&workspace, &package, "main.go", Arc::clone(&original))
        .unwrap()
        .file();
    let previous = BTreeMap::from([(file, Arc::clone(&original))]);

    session
        .database
        .set_source(
            &workspace,
            &package,
            "main.go",
            Arc::new(
                SourceSnapshot::from_source(
                    "/failed/main.go",
                    "package main\nfunc main() { println(1) }\n",
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let orphan = session
        .database
        .set_source(
            &workspace,
            &package,
            "orphan.go",
            Arc::new(
                SourceSnapshot::from_source(
                    "/failed/orphan.go",
                    "package main\nfunc orphan() {}\n",
                )
                .unwrap(),
            ),
        )
        .unwrap()
        .file();

    session.rollback_install(&previous).unwrap();

    assert_eq!(session.database.active_files(), vec![file]);
    assert_eq!(
        session.database.source_snapshot(file).unwrap().as_ref(),
        original.as_ref()
    );
    assert!(session.database.source_snapshot(orphan).is_err());
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
