use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::compiler::input::{
    GoLanguageVersion, PackageCatalogError, PackageInputManifest, PackageKey,
    PackageManifestCatalog, SourceSnapshot, WorkspaceKey,
};
use crate::compiler::provenance::SourceRef;

use super::*;

fn test_workspace() -> WorkspaceKey {
    WorkspaceKey::ad_hoc("compiler-session-tests").unwrap()
}

#[derive(Debug)]
struct TestCatalog {
    manifests: BTreeMap<PackageKey, Arc<PackageInputManifest>>,
    requests: AtomicUsize,
}

impl TestCatalog {
    fn new(manifests: impl IntoIterator<Item = PackageInputManifest>) -> Self {
        Self {
            manifests: manifests
                .into_iter()
                .map(|manifest| (manifest.key().clone(), Arc::new(manifest)))
                .collect(),
            requests: AtomicUsize::new(0),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
}

impl PackageManifestCatalog for TestCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        Ok(self.manifests.get(package).map(Arc::clone))
    }
}

fn package_manifest(
    key: PackageKey,
    logical_path: &str,
    diagnostic_path: &str,
    source: &str,
) -> PackageInputManifest {
    PackageInputManifest::new(
        key,
        [
            super::super::input::SourceFileInput::from_source(
                logical_path,
                diagnostic_path,
                source,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn raw_program(logical_path: &str, diagnostic_path: &str, source: &str) -> ProgramInput {
    let package = super::super::input::PackageKey::command_line();
    let file =
        super::super::input::SourceFileInput::from_source(logical_path, diagnostic_path, source)
            .unwrap();
    let manifest = PackageInputManifest::new(package, [file]).unwrap();
    ProgramInput::standalone(test_workspace(), manifest).unwrap()
}

fn raw_program_at_version(
    logical_path: &str,
    diagnostic_path: &str,
    source: &str,
    language_version: GoLanguageVersion,
) -> ProgramInput {
    let package = super::super::input::PackageKey::command_line();
    let file =
        super::super::input::SourceFileInput::from_source(logical_path, diagnostic_path, source)
            .unwrap();
    let manifest =
        PackageInputManifest::new_with_language_version(package, language_version, [file]).unwrap();
    ProgramInput::standalone(test_workspace(), manifest).unwrap()
}

#[test]
fn unsupported_aggregate_payloads_fail_at_their_source_boundary() {
    for source in [
        "package main\nfunc main() { value := \"x\"; _ = &value }\n",
        "package main\ntype Value struct { Items []int }\nfunc main() { value := Value{}; _ = &value }\n",
        "package main\ntype Value struct { Items []string }\nfunc main() { value := Value{}; _ = &value }\n",
        "package main\ntype Node struct { Next *Node }\nfunc main() { value := Node{}; _ = &value }\n",
        "package main\ntype Node struct { Next *Node }\nfunc use(value *Node) {}\nfunc main() {}\n",
        "package main\nfunc main() { value := &struct { Name string }{}; _ = value }\n",
        "package main\nfunc main() { values := []struct { Name string }{{Name: \"x\"}}; _ = values }\n",
        "package main\nfunc main() { values := map[string]struct { X int }{\"a\": {X: 1}}; _ = values }\n",
    ] {
        let error = CompilerSession::default()
            .compile_program(raw_program("main.go", "/checkout/project/main.go", source))
            .err()
            .expect("an unsupported pointer payload must be rejected before MIR");
        let diagnostic = error.diagnostics().first().unwrap();
        assert_eq!(diagnostic.code, "GORS2001");
        assert_eq!(diagnostic.file, "/checkout/project/main.go");
        assert!(diagnostic.line > 0);
    }
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
    let resolved_inputs = session.database.active_resolved_import_files();
    assert!(!readiness.is_empty());

    let input = ProgramInput::standalone(
        workspace,
        PackageInputManifest::new(
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
        .unwrap(),
    )
    .unwrap();

    let error = session
        .install_program_transaction(&input, |database| {
            assert_eq!(database.active_resolved_import_files().len(), 2);
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
    assert_eq!(
        session.database.active_resolved_import_files(),
        resolved_inputs
    );

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
    let input = ProgramInput::standalone(
        workspace,
        PackageInputManifest::new(
            new_package,
            [super::super::input::SourceFileInput::from_source(
                "main.go",
                "/failed/main.go",
                "package main\nfunc main() {}\n",
            )
            .unwrap()],
        )
        .unwrap(),
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
            assert!(database.resolved_file_imports(new_file).is_ok());
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
    assert!(session.database.active_resolved_import_files().is_empty());
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
fn binary_literals_obey_package_and_file_language_versions() {
    let source = "package main\n\nfunc main() {\n\tx := 0b1011\n\tprintln(x)\n}\n";
    let old = raw_program_at_version(
        "main.go",
        "/checkout/project/main.go",
        source,
        GoLanguageVersion::new(1, 12),
    );
    let mut session = CompilerSession::default();
    let error = session
        .compile_program(old)
        .err()
        .expect("a binary literal must be rejected under Go 1.12");
    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(diagnostic.file, "/checkout/project/main.go");
    assert_eq!((diagnostic.line, diagnostic.column), (4, 7));
    assert_eq!(
        diagnostic.message,
        "binary literal requires go1.13 or later (-lang was set to go1.12; check go.mod)"
    );

    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 13),
        ))
        .expect("Go 1.13 accepts binary literals");

    let file_override = "//go:build go1.13\n\npackage main\n\nfunc main() { println(0b1011) }\n";
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            file_override,
            GoLanguageVersion::new(1, 12),
        ))
        .expect("a Go version build constraint selects the file language version");
}

#[test]
fn language_version_only_updates_reuse_projection_and_semantic_products() {
    let source = "package main\nfunc main() { println(0b1011) }\n";
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 13),
        ))
        .unwrap();
    session.database().reset_telemetry();

    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 14),
        ))
        .unwrap();

    let telemetry = session.database().telemetry();
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::FileProjection),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::LanguageVersionCheck),
        1
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::TypedHir),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::VerifiedGoMir),
        0
    );
    assert_eq!(
        telemetry.executions(crate::compiler::db::QueryKind::VerifiedRustIr),
        0
    );

    session.database().reset_telemetry();
    session
        .compile_program(raw_program_at_version(
            "main.go",
            "/checkout/project/main.go",
            source,
            GoLanguageVersion::new(1, 14),
        ))
        .unwrap();
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn runtime_int32_negation_diagnostic_is_incrementally_reused() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc negate(value int32) int32 { return -value }\nfunc main() {}\n",
    );
    let mut session = CompilerSession::default();

    let first = session
        .compile_program(program.clone())
        .err()
        .expect("runtime int32 negation must be rejected before representation lowering");
    let diagnostic = first.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2001");
    assert!(
        diagnostic
            .message
            .contains("runtime unary negation of int32/rune requires 32-bit wrapping semantics")
    );

    session.database().reset_telemetry();
    let second = session
        .compile_program(program)
        .err()
        .expect("the unchanged unsupported revision must remain invalid");

    assert_eq!(second, first);
    assert_eq!(session.database().telemetry().total_executions(), 0);
}

#[test]
fn goto_scope_diagnostic_is_incrementally_reused() {
    let program = raw_program(
        "main.go",
        "/checkout/main.go",
        "package main\nfunc main() {\n\tgoto Nested\n\t{\n\tNested:\n\t}\n}\n",
    );
    let mut session = CompilerSession::default();

    let first = session
        .compile_program(program.clone())
        .err()
        .expect("goto into a nested block must fail semantic analysis");
    let diagnostic = first.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!(diagnostic.message, "goto Nested jumps into block");

    session.database().reset_telemetry();
    let second = session
        .compile_program(program)
        .err()
        .expect("the unchanged invalid goto must remain invalid");

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
fn reachable_catalog_dependency_compiles_through_its_declared_package_name() {
    let entry = super::super::input::PackageKey::command_line();
    let dependency = super::super::input::PackageKey::import_path("example/dependency").unwrap();
    let entry_manifest = PackageInputManifest::new(
        entry,
        [super::super::input::SourceFileInput::from_source(
            "main.go",
            "/checkout/main.go",
            "package main\nimport \"example/dependency\"\nfunc main() { if actualname.Value() != 1 { panic(\"dependency\") } }\n",
        )
        .unwrap()],
    )
    .unwrap();
    let dependency_manifest = PackageInputManifest::new(
        dependency,
        [super::super::input::SourceFileInput::from_source(
            "dependency.go",
            "/checkout/dependency/dependency.go",
            "package actualname\nfunc Value() int { return 1 }\n",
        )
        .unwrap()],
    )
    .unwrap();
    let catalog = Arc::new(TestCatalog::new([dependency_manifest]));
    let input = ProgramInput::new(test_workspace(), entry_manifest, catalog.clone()).unwrap();
    let mut session = CompilerSession::default();

    let compiled = session.compile_program(input).unwrap();

    assert_eq!(compiled.modules.len(), 1);
    assert!(compiled.modules.contains_key("example__dependency"));
    assert_eq!(catalog.request_count(), 1);
    assert_eq!(session.database().active_files().len(), 2);
    let packages = session
        .database()
        .active_files()
        .into_iter()
        .map(|file| session.database().package_for_file(file).unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(packages.len(), 2);
    let entry_file = session
        .database()
        .active_files()
        .into_iter()
        .find(|file| {
            session.database().package_for_file(*file).unwrap()
                == session.admitted_package_dag().unwrap().entry()
        })
        .unwrap();
    let resolved = session
        .database()
        .resolved_file_imports(entry_file)
        .unwrap();
    assert_eq!(resolved.imports().len(), 1);
    assert_eq!(
        resolved.imports().first().unwrap().binding().local_name(),
        Some("actualname")
    );
    let dag = session
        .admitted_package_dag()
        .expect("successful admission publishes its canonical package graph");
    assert_eq!(dag.nodes().len(), 2);
    assert_eq!(dag.edges().len(), 1);
    assert_eq!(dag.topological_order().last(), Some(&dag.entry()));
}

#[test]
fn recursively_admits_only_reachable_packages_in_dependency_first_layers() {
    let entry_key = PackageKey::command_line();
    let first_key = PackageKey::import_path("example/first").unwrap();
    let leaf_key = PackageKey::import_path("example/leaf").unwrap();
    let unrelated_key = PackageKey::import_path("example/unrelated").unwrap();
    let entry = package_manifest(
        entry_key.clone(),
        "main.go",
        "/checkout/main.go",
        "package main\nimport \"example/first\"\nfunc main() { println(first.Value()) }\n",
    );
    let first = package_manifest(
        first_key.clone(),
        "first.go",
        "/checkout/first/first.go",
        "package first\nimport \"example/leaf\"\nfunc Value() int { return leaf.Value() }\n",
    );
    let leaf = package_manifest(
        leaf_key.clone(),
        "leaf.go",
        "/checkout/leaf/leaf.go",
        "package leaf\nfunc Value() int { return 1 }\n",
    );
    let unrelated = package_manifest(
        unrelated_key,
        "unrelated.go",
        "/checkout/unrelated/unrelated.go",
        "package unrelated\n",
    );
    let catalog = Arc::new(TestCatalog::new([unrelated, leaf, first]));
    let input = ProgramInput::new(test_workspace(), entry, catalog.clone()).unwrap();
    let mut session = CompilerSession::default();

    let compiled = session.compile_program(input).unwrap();

    assert_eq!(compiled.modules.len(), 2);
    assert_eq!(catalog.request_count(), 2);
    let dag = session.admitted_package_dag().unwrap();
    assert_eq!(dag.nodes().len(), 3);
    assert_eq!(dag.edges().len(), 2);
    assert_eq!(dag.layers().len(), 3);
    let id_for = |key: &PackageKey| {
        dag.nodes()
            .iter()
            .find(|node| node.key() == key)
            .unwrap()
            .package()
    };
    assert_eq!(
        dag.topological_order(),
        [id_for(&leaf_key), id_for(&first_key), id_for(&entry_key)]
    );
}

#[test]
fn missing_reachable_package_rolls_back_sources_and_preserves_prior_dag() {
    let mut session = CompilerSession::default();
    session
        .compile_program(raw_program(
            "main.go",
            "/checkout/previous.go",
            "package main\nfunc main() {}\n",
        ))
        .unwrap();
    let previous_files = session.database().active_files();
    let previous_resolved_files = session.database().active_resolved_import_files();
    let previous_dag = session.admitted_package_dag().unwrap().clone();
    let entry = package_manifest(
        PackageKey::command_line(),
        "main.go",
        "/checkout/next.go",
        "package main\nimport \"example/missing\"\nfunc main() {}\n",
    );
    let input = ProgramInput::standalone(test_workspace(), entry).unwrap();

    let error = session
        .compile_program(input)
        .err()
        .expect("a missing reachable package must reject admission");

    assert_eq!(error.diagnostics().first().unwrap().code, "GORS2004");
    assert!(error.to_string().contains("unresolved import"));
    assert_eq!(session.database().active_files(), previous_files);
    assert_eq!(
        session.database().active_resolved_import_files(),
        previous_resolved_files
    );
    assert_eq!(session.admitted_package_dag(), Some(&previous_dag));
}

#[test]
fn import_cycle_is_source_anchored_and_never_commits_partial_packages() {
    let entry = package_manifest(
        PackageKey::command_line(),
        "main.go",
        "/checkout/main.go",
        "package main\nimport \"example/a\"\nfunc main() {}\n",
    );
    let a = package_manifest(
        PackageKey::import_path("example/a").unwrap(),
        "a.go",
        "/checkout/a/a.go",
        "package a\nimport \"example/b\"\n",
    );
    let b = package_manifest(
        PackageKey::import_path("example/b").unwrap(),
        "b.go",
        "/checkout/b/b.go",
        "package b\nimport \"example/a\"\n",
    );
    let catalog = Arc::new(TestCatalog::new([a, b]));
    let input = ProgramInput::new(test_workspace(), entry, catalog.clone()).unwrap();
    let mut session = CompilerSession::default();

    let error = session
        .compile_program(input)
        .err()
        .expect("an import cycle must reject admission");

    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2004");
    assert!(diagnostic.message.contains("import cycle"));
    assert!(!diagnostic.file.is_empty());
    assert!(diagnostic.line > 0);
    assert_eq!(catalog.request_count(), 2);
    assert!(session.database().active_files().is_empty());
    assert!(session.admitted_package_dag().is_none());
}

#[test]
fn huge_valid_and_invalid_catalog_packages_cost_nothing_beyond_entry() {
    let entry = super::super::input::PackageKey::command_line();
    let entry_manifest = PackageInputManifest::new(
        entry,
        [super::super::input::SourceFileInput::from_source(
            "main.go",
            "/checkout/main.go",
            "package main\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let baseline_input =
        ProgramInput::standalone(test_workspace(), entry_manifest.clone()).unwrap();
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
    let catalog = Arc::new(TestCatalog::new([invalid_manifest, valid_manifest]));
    let input = ProgramInput::new(test_workspace(), entry_manifest, catalog.clone()).unwrap();
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
    assert_eq!(catalog.request_count(), 0);
}

#[test]
fn unrelated_package_edit_preserves_entry_queries_and_scheduler_readiness() {
    fn input(unrelated_body: &str) -> (ProgramInput, Arc<TestCatalog>) {
        let entry = super::super::input::PackageKey::command_line();
        let unrelated = super::super::input::PackageKey::import_path("example/unrelated").unwrap();
        let entry_manifest = PackageInputManifest::new(
            entry,
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
        let catalog = Arc::new(TestCatalog::new([unrelated_manifest]));
        let input = ProgramInput::new(test_workspace(), entry_manifest, catalog.clone()).unwrap();
        (input, catalog)
    }

    let host = CompilerHost::new(NonZeroUsize::new(2).unwrap()).unwrap();
    let mut session = host.session(BuildConfig::default()).unwrap();
    let (first_input, first_catalog) = input("package unrelated\nfunc Value() int { return 1 }\n");
    session.compile_program(first_input).unwrap();
    assert_eq!(first_catalog.request_count(), 0);
    let scheduler = host.telemetry();
    let retained_bytes = session.database().retained_source_bytes();
    session.database().reset_telemetry();

    let (second_input, second_catalog) =
        input("package unrelated\nfunc Value() int { return 2 }\n");
    session.compile_program(second_input).unwrap();

    assert_eq!(second_catalog.request_count(), 0);
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
        previous_key,
        [super::super::input::SourceFileInput::from_source(
            "previous.go",
            "/checkout/previous.go",
            "package main\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let first = ProgramInput::standalone(workspace.clone(), previous_manifest.clone()).unwrap();
    let mut session = CompilerSession::default();
    session.compile_program(first).unwrap();
    let previous_file = *session.database().active_files().first().unwrap();
    let previous_package = session.database().package_for_file(previous_file).unwrap();

    let entry = super::super::input::PackageKey::command_line();
    let entry_manifest = PackageInputManifest::new(
        entry,
        [super::super::input::SourceFileInput::from_source(
            "main.go",
            "/checkout/main.go",
            "package main\nfunc main() {}\n",
        )
        .unwrap()],
    )
    .unwrap();
    let catalog = Arc::new(TestCatalog::new([previous_manifest]));
    let second = ProgramInput::new(workspace, entry_manifest, catalog.clone()).unwrap();

    session.compile_program(second).unwrap();

    assert_eq!(catalog.request_count(), 0);
    let active = session.database().active_files();
    assert_eq!(active.len(), 1);
    assert_ne!(active.first().copied(), Some(previous_file));
    assert!(session.database().source_snapshot(previous_file).is_err());
    assert!(
        session
            .database()
            .resolved_file_imports(previous_file)
            .is_err()
    );
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
