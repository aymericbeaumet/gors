#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use gors::compiler::db::{
    BuildConfig, CompilerDatabase, FileAnalysis, PackageIssue, QueryError, QueryKind,
};
use gors::compiler::ids::{DefId, FileId};
use gors::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};
use gors::compiler::source::{LogicalColumn, TextSize};
use gors::parser::ImportPathIssue;

const ORIGINAL: &str = r#"package main

func f(x int) int { return x + 1 }
func g(x int) int { return x + 2 }
"#;

const BODY_EDIT: &str = r#"package main

func f(x int) int { return x + 10 }
func g(x int) int { return x + 2 }
"#;

const SIGNATURE_EDIT: &str = r#"package main

func f(x bool) int { return 1 }
func g(x int) int { return x + 2 }
"#;

const REORDERED: &str = r#"package main

func g(x int) int { return x + 2 }
func f(x int) int { return x + 1 }
"#;

const COMPLETE_PROGRAM: &str = r#"package main

func f(x int) int { return x + 1 }
func main() { println(f(1)) }
"#;

const COMMENTED_COMPLETE_PROGRAM: &str = r#"package main

// f documents f.
func f(x int) int { return x + 1 }
func main() { println(f(1)) }
"#;

fn source(text: &str) -> Arc<SourceSnapshot> {
    Arc::new(SourceSnapshot::from_source("main.go", text).unwrap())
}

fn workspace() -> WorkspaceKey {
    WorkspaceKey::AdHoc("workspace".into())
}

fn package_key(import_path: &str) -> PackageKey {
    PackageKey::ImportPath(import_path.into())
}

fn insert(db: &mut CompilerDatabase, text: &str) -> FileId {
    db.set_source(
        &workspace(),
        &package_key("example/main"),
        "main.go",
        source(text),
    )
    .unwrap()
    .file()
}

fn insert_file(
    db: &mut CompilerDatabase,
    import_path: &str,
    logical_path: &str,
    text: &str,
) -> FileId {
    db.set_source(
        &workspace(),
        &package_key(import_path),
        logical_path,
        Arc::new(SourceSnapshot::from_source(logical_path, text).unwrap()),
    )
    .unwrap()
    .file()
}

fn functions(analysis: &FileAnalysis) -> BTreeMap<String, DefId> {
    analysis
        .functions()
        .iter()
        .map(|function| (function.name().to_string(), function.id()))
        .collect()
}

fn prime_all(db: &CompilerDatabase, file: FileId) -> BTreeMap<String, DefId> {
    let analysis = db.analyze_file(file).unwrap();
    let functions = functions(&analysis);
    let _ = db.public_api(file).unwrap();
    for id in functions.values().copied() {
        let _ = db.function_signature(file, id).unwrap();
        let _ = db.function_body(file, id).unwrap();
    }
    functions
}

fn function_id(functions: &BTreeMap<String, DefId>, name: &str) -> DefId {
    functions.get(name).copied().expect("function should exist")
}

#[test]
fn default_build_config_uses_the_packaged_runtime_abi() {
    assert_eq!(BuildConfig::default().runtime_abi(), gors::RUNTIME_ABI_ID);
    assert_eq!(gors::RUNTIME_ABI_ID, "gors-runtime-abi-v3");
}

#[test]
fn equal_source_update_executes_no_query_bodies() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, ORIGINAL);
    let functions = prime_all(&db, file);
    db.reset_telemetry();

    let same_file = insert(&mut db, ORIGINAL);
    assert_eq!(same_file, file);
    let _ = db.analyze_file(file).unwrap();
    let _ = db.public_api(file).unwrap();
    for id in functions.values().copied() {
        let _ = db.function_signature(file, id).unwrap();
        let _ = db.function_body(file, id).unwrap();
    }

    let telemetry = db.telemetry();
    assert_eq!(telemetry.total_executions(), 0);
    assert_eq!(telemetry.engine().will_execute, 0);
    assert_eq!(telemetry.engine().cancellation_requests, 0);
}

#[test]
fn diagnostic_path_update_reuses_the_complete_semantic_pipeline() {
    let mut db = CompilerDatabase::default();
    let file = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            Arc::new(
                SourceSnapshot::from_source("/checkout/one/main.go", COMPLETE_PROGRAM).unwrap(),
            ),
        )
        .unwrap()
        .file();
    let analysis = db.analyze_file(file).unwrap();
    let functions = functions(&analysis);
    let function = function_id(&functions, "f");
    let package = db.package_for_file(file).unwrap();
    let public_api = db.public_api(file).unwrap();
    let provenance = db.function_provenance(file, function).unwrap();
    let typed_hir = db.typed_hir(file, function).unwrap();
    let typed_signature = db.typed_signature(file, function).unwrap();
    let verified_mir = db.verified_mir(file, function).unwrap();
    let normalized_mir = db.normalized_mir(file, function).unwrap();
    let verified_rust_ir = db.verified_rust_ir(file, function).unwrap();
    let verified_package = db.verified_rust_ir_package(package).unwrap();
    let canonical_content = db.source_snapshot(file).unwrap().content();
    db.reset_telemetry();

    let moved_snapshot =
        Arc::new(SourceSnapshot::from_source("/checkout/two/main.go", COMPLETE_PROGRAM).unwrap());
    let duplicate_content = moved_snapshot.content();
    let duplicate_content_weak = Arc::downgrade(&duplicate_content);
    let update = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            moved_snapshot,
        )
        .unwrap();
    drop(duplicate_content);
    assert_eq!(update.file(), file);
    assert!(!update.semantic_changed());
    assert!(update.diagnostic_path_changed());
    assert!(!update.inserted());
    let current_snapshot = db.source_snapshot(file).unwrap();
    assert_eq!(current_snapshot.diagnostic_path(), "/checkout/two/main.go");
    assert!(Arc::ptr_eq(&canonical_content, &current_snapshot.content()));
    assert!(duplicate_content_weak.upgrade().is_none());
    assert!(Arc::ptr_eq(&analysis, &db.analyze_file(file).unwrap()));
    assert!(Arc::ptr_eq(&public_api, &db.public_api(file).unwrap()));
    assert!(Arc::ptr_eq(
        &provenance,
        &db.function_provenance(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &typed_hir,
        &db.typed_hir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &typed_signature,
        &db.typed_signature(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &verified_mir,
        &db.verified_mir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &normalized_mir,
        &db.normalized_mir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &verified_rust_ir,
        &db.verified_rust_ir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &verified_package,
        &db.verified_rust_ir_package(package).unwrap()
    ));
    let telemetry = db.telemetry();
    assert_eq!(telemetry.total_executions(), 0);
    assert_eq!(telemetry.engine().will_execute, 0);
    assert_eq!(telemetry.engine().cancellation_requests, 0);
}

#[test]
fn diagnostic_path_update_reuses_a_cached_parse_failure() {
    let invalid = "package main\nfunc broken(";
    let mut db = CompilerDatabase::default();
    let file = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            Arc::new(SourceSnapshot::from_source("/old/main.go", invalid).unwrap()),
        )
        .unwrap()
        .file();
    let failure = db.analyze_file(file).unwrap();
    assert!(failure.failure().is_some());
    db.reset_telemetry();

    let update = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            Arc::new(SourceSnapshot::from_source("/new/main.go", invalid).unwrap()),
        )
        .unwrap();
    assert!(!update.semantic_changed());
    assert!(update.diagnostic_path_changed());

    assert!(Arc::ptr_eq(&failure, &db.analyze_file(file).unwrap()));
    assert_eq!(
        db.source_snapshot(file).unwrap().diagnostic_path(),
        "/new/main.go"
    );
    assert_eq!(db.telemetry().total_executions(), 0);
    assert_eq!(db.telemetry().engine().will_execute, 0);
    assert_eq!(db.telemetry().engine().cancellation_requests, 0);
}

#[test]
fn comment_only_edit_preserves_function_semantics_and_rust_ir() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, COMPLETE_PROGRAM);
    let functions = functions(&db.analyze_file(file).unwrap());
    let function = function_id(&functions, "f");
    let package = db.package_for_file(file).unwrap();
    let old_comments = db.file_comments(file).unwrap();
    let old_provenance = db.function_provenance(file, function).unwrap();
    let old_hir = db.typed_hir(file, function).unwrap();
    let old_mir = db.verified_mir(file, function).unwrap();
    let old_normalized = db.normalized_mir(file, function).unwrap();
    let old_rust_ir = db.verified_rust_ir(file, function).unwrap();
    let old_package = db.verified_rust_ir_package(package).unwrap();
    assert!(old_comments.comments().is_empty());

    insert(&mut db, COMMENTED_COMPLETE_PROGRAM);
    db.reset_telemetry();

    let comments = db.file_comments(file).unwrap();
    assert!(!Arc::ptr_eq(&old_comments, &comments));
    let comment = comments.comments().first().unwrap();
    assert_eq!(comment.file(), file);
    assert_eq!(comment.text(), "// f documents f.");
    assert_eq!(comment.line(), 3);
    assert_eq!(comment.column(), 1);
    assert!(comment.is_doc());
    assert_eq!(
        comment.byte_start(),
        COMMENTED_COMPLETE_PROGRAM.find(comment.text()).unwrap()
    );
    assert_eq!(
        comment.byte_end(),
        comment.byte_start() + comment.text().len()
    );
    assert!(!Arc::ptr_eq(
        &old_provenance,
        &db.function_provenance(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_hir,
        &db.typed_hir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_mir,
        &db.verified_mir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_normalized,
        &db.normalized_mir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_rust_ir,
        &db.verified_rust_ir(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_package,
        &db.verified_rust_ir_package(package).unwrap()
    ));

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::SemanticFile), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
    assert_eq!(telemetry.executions(QueryKind::RustIrPackage), 0);
}

#[test]
fn line_directives_do_not_replace_physical_comment_coordinates() {
    let source = r#"package main
//line /virtual/generated.go:200
// physical comment
func f() {}
"#;
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, source);
    let comments = db.file_comments(file).unwrap();

    assert_eq!(comments.comments().len(), 2);
    let directive = comments.comments().first().unwrap();
    let physical = comments.comments().get(1).unwrap();
    assert_eq!((directive.line(), directive.column()), (2, 1));
    assert_eq!((physical.line(), physical.column()), (3, 1));
    assert_eq!(
        physical.byte_start(),
        source.find("// physical comment").unwrap()
    );
    assert!(physical.is_doc());
}

#[test]
fn import_projection_preserves_occurrences_and_structured_failures() {
    let source = r#"package sample
import (
    "fmt"
    "encoding/json"
    "fmt"
    "\xff"
)
func f() {}
"#;
    let mut db = CompilerDatabase::default();
    let file = insert_file(&mut db, "example/sample", "imports.go", source);
    let imports = db.file_imports(file).unwrap();

    assert_eq!(
        imports
            .direct()
            .iter()
            .map(|import| import.path())
            .collect::<Vec<_>>(),
        ["fmt", "encoding/json", "fmt"]
    );
    assert_eq!(imports.invalid().len(), 1);
    let invalid = imports.invalid().first().unwrap();
    assert_eq!(invalid.file(), file);
    assert_eq!(invalid.literal(), r#""\xff""#);
    assert_eq!(
        invalid.byte_offset(),
        source.find(invalid.literal()).unwrap()
    );
    assert_eq!((invalid.line(), invalid.column()), (6, 5));
    assert_eq!(invalid.virtual_file(), None);
    assert_eq!(invalid.issue(), &ImportPathIssue::InvalidUtf8);

    let package = db.package_for_file(file).unwrap();
    let analysis = db.analyze_package(package).unwrap();
    assert_eq!(
        analysis
            .direct_imports()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["encoding/json", "fmt"]
    );
    assert!(analysis.issues().iter().any(|issue| matches!(
        issue,
        PackageIssue::InvalidImportPath {
            file: issue_file,
            literal,
            line: 6,
            column: 5,
            virtual_file: None,
            issue: ImportPathIssue::InvalidUtf8,
        } if *issue_file == file && literal.as_ref() == r#""\xff""#
    )));
}

#[test]
fn import_line_directive_origin_is_retained_without_a_display_path() {
    let source = "package sample\n//line /virtual/imports.go:40\nimport \"\\xff\"\n";
    let mut first = CompilerDatabase::default();
    let first_file = first
        .set_source(
            &workspace(),
            &package_key("example/sample"),
            "src/main.go",
            Arc::new(SourceSnapshot::from_source("/checkout/one/main.go", source).unwrap()),
        )
        .unwrap()
        .file();
    let mut second = CompilerDatabase::default();
    let second_file = second
        .set_source(
            &workspace(),
            &package_key("example/sample"),
            "src/main.go",
            Arc::new(SourceSnapshot::from_source("/checkout/two/main.go", source).unwrap()),
        )
        .unwrap()
        .file();

    assert_eq!(first_file, second_file);
    let first_imports = first.file_imports(first_file).unwrap();
    let second_imports = second.file_imports(second_file).unwrap();
    assert_eq!(first_imports, second_imports);
    let invalid = first_imports.invalid().first().unwrap();
    assert_eq!(invalid.virtual_file(), Some("/virtual/imports.go"));
    assert_eq!((invalid.line(), invalid.column()), (40, 0));

    let package = first.package_for_file(first_file).unwrap();
    assert!(
        first
            .analyze_package(package)
            .unwrap()
            .issues()
            .iter()
            .any(|issue| matches!(
                issue,
                PackageIssue::InvalidImportPath {
                virtual_file: Some(file),
                line: 40,
                column: 0,
                    ..
                } if file.as_ref() == "/virtual/imports.go"
            ))
    );
}

#[test]
fn import_edit_invalidates_import_facts_without_rebuilding_function_products() {
    let original = "package main\nimport \"example/one\"\nfunc f(x int) int { return x + 1 }\n";
    let changed = "package main\nimport \"example/two\"\nfunc f(x int) int { return x + 1 }\n";
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, original);
    let functions = functions(&db.analyze_file(file).unwrap());
    let function = function_id(&functions, "f");
    let package = db.package_for_file(file).unwrap();
    let old_imports = db.file_imports(file).unwrap();
    let old_analysis = db.analyze_file(file).unwrap();
    let old_package = db.analyze_package(package).unwrap();
    let old_signature = db.function_signature(file, function).unwrap();
    let old_body = db.function_body(file, function).unwrap();
    let old_hir_failure = match db.typed_hir(file, function).unwrap_err() {
        QueryError::StageFailure(failure) => failure,
        error => panic!("unexpected HIR query error: {error}"),
    };

    insert(&mut db, changed);
    db.reset_telemetry();

    let imports = db.file_imports(file).unwrap();
    assert!(!Arc::ptr_eq(&old_imports, &imports));
    assert_ne!(old_imports.fingerprint(), imports.fingerprint());
    assert_eq!(imports.direct().first().unwrap().path(), "example/two");
    assert!(Arc::ptr_eq(&old_analysis, &db.analyze_file(file).unwrap()));
    let new_package = db.analyze_package(package).unwrap();
    assert!(!Arc::ptr_eq(&old_package, &new_package));
    assert_eq!(
        new_package.direct_imports().first().unwrap().as_ref(),
        "example/two"
    );
    assert!(Arc::ptr_eq(
        &old_signature,
        &db.function_signature(file, function).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &old_body,
        &db.function_body(file, function).unwrap()
    ));
    let new_hir_failure = match db.typed_hir(file, function).unwrap_err() {
        QueryError::StageFailure(failure) => failure,
        error => panic!("unexpected HIR query error: {error}"),
    };
    assert!(Arc::ptr_eq(&old_hir_failure, &new_hir_failure));

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::SemanticFile), 1);
    assert_eq!(telemetry.executions(QueryKind::FileAnalysis), 0);
    assert_eq!(telemetry.executions(QueryKind::PackageAnalysis), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 0);
    assert_eq!(telemetry.executions(QueryKind::FunctionBody), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
}

#[test]
fn body_edit_reuses_public_headers_and_the_unrelated_body() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, ORIGINAL);
    let ids = prime_all(&db, file);
    let f = function_id(&ids, "f");
    let g = function_id(&ids, "g");
    let old_analysis = db.analyze_file(file).unwrap();
    let old_public = db.public_api(file).unwrap();
    let old_f_signature = db.function_signature(file, f).unwrap();
    let old_g_signature = db.function_signature(file, g).unwrap();
    let old_f_body = db.function_body(file, f).unwrap();
    let old_g_body = db.function_body(file, g).unwrap();

    insert(&mut db, BODY_EDIT);
    db.reset_telemetry();
    let analysis = db.analyze_file(file).unwrap();
    let public = db.public_api(file).unwrap();
    let f_signature = db.function_signature(file, f).unwrap();
    let g_signature = db.function_signature(file, g).unwrap();
    let f_body = db.function_body(file, f).unwrap();
    let g_body = db.function_body(file, g).unwrap();

    assert!(Arc::ptr_eq(&old_analysis, &analysis));
    assert!(Arc::ptr_eq(&old_public, &public));
    assert!(Arc::ptr_eq(&old_f_signature, &f_signature));
    assert!(Arc::ptr_eq(&old_g_signature, &g_signature));
    assert!(!Arc::ptr_eq(&old_f_body, &f_body));
    assert!(Arc::ptr_eq(&old_g_body, &g_body));
    assert_ne!(old_f_body.fingerprint(), f_body.fingerprint());

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::FileAnalysis), 0);
    assert_eq!(telemetry.executions(QueryKind::PublicApi), 0);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 0);
    assert_eq!(telemetry.executions(QueryKind::FunctionBody), 1);
}

#[test]
fn signature_edit_invalidates_the_public_product() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, ORIGINAL);
    let ids = prime_all(&db, file);
    let old_public = db.public_api(file).unwrap();

    insert(&mut db, SIGNATURE_EDIT);
    db.reset_telemetry();
    let public = db.public_api(file).unwrap();

    assert!(!Arc::ptr_eq(&old_public, &public));
    assert_ne!(old_public.fingerprint(), public.fingerprint());
    assert_eq!(functions(&db.analyze_file(file).unwrap()), ids);
    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::PublicApi), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 1);
}

#[test]
fn declaration_reorder_preserves_ids_and_every_projection() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, ORIGINAL);
    let original_analysis = db.analyze_file(file).unwrap();
    let ids = prime_all(&db, file);
    let original_public = db.public_api(file).unwrap();
    let original_f_body = db.function_body(file, function_id(&ids, "f")).unwrap();
    let original_g_body = db.function_body(file, function_id(&ids, "g")).unwrap();

    insert(&mut db, REORDERED);
    db.reset_telemetry();
    let reordered_analysis = db.analyze_file(file).unwrap();
    let reordered_public = db.public_api(file).unwrap();
    let reordered_f_body = db.function_body(file, function_id(&ids, "f")).unwrap();
    let reordered_g_body = db.function_body(file, function_id(&ids, "g")).unwrap();

    assert_eq!(functions(&reordered_analysis), ids);
    assert!(Arc::ptr_eq(&original_analysis, &reordered_analysis));
    assert!(Arc::ptr_eq(&original_public, &reordered_public));
    assert!(Arc::ptr_eq(&original_f_body, &reordered_f_body));
    assert!(Arc::ptr_eq(&original_g_body, &reordered_g_body));
    assert_eq!(db.telemetry().executions(QueryKind::FileProjection), 1);
    assert_eq!(db.telemetry().executions(QueryKind::SemanticFile), 1);
    assert_eq!(db.telemetry().total_executions(), 2);
}

#[test]
fn parse_failure_is_recoverable_input_state() {
    let mut db = CompilerDatabase::default();
    let invalid = "package main\nfunc broken(";
    let file = insert(&mut db, invalid);
    let failed = db.analyze_file(file).unwrap();
    let failure = failed.failure().unwrap();
    assert_eq!(failure.physical_range().start().to_usize(), invalid.len());
    assert_eq!(failure.physical_range().end().to_usize(), invalid.len());
    assert!(failed.functions().is_empty());

    insert(&mut db, ORIGINAL);
    db.reset_telemetry();
    let recovered = db.analyze_file(file).unwrap();
    assert!(recovered.failure().is_none());
    assert_eq!(recovered.functions().len(), 2);
    assert_eq!(db.telemetry().executions(QueryKind::FileProjection), 1);
    assert_eq!(db.telemetry().executions(QueryKind::FileAnalysis), 1);
}

#[test]
fn parse_failure_separates_physical_anchor_from_line_directive_coordinates() {
    let invalid = "package main\n//line virtual.go:40\nfunc broken( {";
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, invalid);
    let failed = db.analyze_file(file).unwrap();
    let failure = failed.failure().unwrap();

    assert_eq!(
        failure.physical_range().start().to_usize(),
        invalid.find('{').unwrap()
    );
    let map = db.source_coordinate_map(file).unwrap();
    let adjusted = map
        .adjusted_coordinate_for(
            failure.physical_range().start(),
            "/checkout/project/main.go",
        )
        .unwrap()
        .unwrap();
    assert_eq!(adjusted.filename(), "/checkout/project/virtual.go");
    assert_eq!(adjusted.position().line().get(), 40);
    assert_eq!(adjusted.position().column(), LogicalColumn::Hidden);
}

#[test]
fn successful_file_query_retains_the_complete_parser_coordinate_map() {
    let source = "package main\n//line virtual.go:40\nfunc main() {}\n";
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, source);
    let map = db.source_coordinate_map(file).unwrap();
    let eof = TextSize::try_from(source.len()).unwrap();

    assert_eq!(map.text_len(), eof);
    let adjusted = map.adjusted_coordinate(eof).unwrap().unwrap();
    assert_eq!(adjusted.filename(), "virtual.go");
    assert_eq!(adjusted.position().column(), LogicalColumn::Hidden);
    assert_eq!(db.telemetry().executions(QueryKind::FileProjection), 1);
}

#[test]
fn scanner_failure_keeps_hidden_column_and_exact_physical_anchor() {
    let invalid = "package main\n//line virtual.go:40\n@";
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, invalid);
    let failed = db.analyze_file(file).unwrap();
    let failure = failed.failure().unwrap();

    assert_eq!(
        failure.physical_range().start().to_usize(),
        invalid.find('@').unwrap()
    );
    let map = db.source_coordinate_map(file).unwrap();
    let adjusted = map
        .adjusted_coordinate_for(
            failure.physical_range().start(),
            "/checkout/project/main.go",
        )
        .unwrap()
        .unwrap();
    assert_eq!(adjusted.filename(), "/checkout/project/virtual.go");
    assert_eq!(adjusted.position().line().get(), 40);
    assert_eq!(adjusted.position().column(), LogicalColumn::Hidden);
}

#[test]
fn parse_failure_products_ignore_checkout_paths() {
    let invalid = "package main\nfunc broken(";
    let mut first = CompilerDatabase::default();
    let mut second = CompilerDatabase::default();
    let first_file = first
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            Arc::new(SourceSnapshot::from_source("/checkout/one/main.go", invalid).unwrap()),
        )
        .unwrap()
        .file();
    let second_file = second
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "main.go",
            Arc::new(SourceSnapshot::from_source("/checkout/two/main.go", invalid).unwrap()),
        )
        .unwrap()
        .file();

    assert_eq!(first_file, second_file);
    assert_eq!(
        first.analyze_file(first_file).unwrap(),
        second.analyze_file(second_file).unwrap()
    );
}

#[test]
fn identical_databases_produce_identical_results() {
    let mut first = CompilerDatabase::default();
    let mut second = CompilerDatabase::default();
    let first_file = insert(&mut first, ORIGINAL);
    let second_file = insert(&mut second, ORIGINAL);

    assert_eq!(first_file, second_file);
    assert_eq!(
        first.analyze_file(first_file).unwrap(),
        second.analyze_file(second_file).unwrap()
    );
    assert_eq!(
        first.public_api(first_file).unwrap(),
        second.public_api(second_file).unwrap()
    );
}

#[test]
fn source_payloads_are_costed_and_released_per_file() {
    let mut db = CompilerDatabase::default();
    let first_source = source(ORIGINAL);
    let first_weak = Arc::downgrade(&first_source);
    let first_file = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "first.go",
            Arc::clone(&first_source),
        )
        .unwrap()
        .file();
    let first_bytes = db.retained_source_bytes();
    let second_source =
        Arc::new(SourceSnapshot::from_source("second.go", "package main\nfunc h() {}\n").unwrap());
    let second_file = db
        .set_source(
            &workspace(),
            &package_key("example/main"),
            "second.go",
            Arc::clone(&second_source),
        )
        .unwrap()
        .file();
    let both_bytes = db.retained_source_bytes();
    let second_bytes = both_bytes.saturating_sub(first_bytes);
    let _ = db.analyze_file(first_file).unwrap();
    let _ = db.analyze_file(second_file).unwrap();
    assert_eq!(db.retained_source_bytes(), both_bytes);

    drop(first_source);
    db.remove_source(first_file).unwrap();
    assert!(first_weak.upgrade().is_none());
    assert_eq!(db.retained_source_bytes(), second_bytes);
    assert!(db.analyze_file(first_file).is_err());
    assert!(db.analyze_file(second_file).is_ok());
}

#[test]
fn package_analysis_is_independent_of_file_insertion_order() {
    let first_source = "package sample\nfunc First(x int) int { return x + 1 }\n";
    let second_source = "package sample\nfunc Second(x int) int { return x + 2 }\n";
    let mut forward = CompilerDatabase::default();
    let forward_first = insert_file(&mut forward, "example/sample", "a.go", first_source);
    insert_file(&mut forward, "example/sample", "b.go", second_source);
    let forward_package = forward.package_for_file(forward_first).unwrap();

    let mut reverse = CompilerDatabase::default();
    let reverse_second = insert_file(&mut reverse, "example/sample", "b.go", second_source);
    insert_file(&mut reverse, "example/sample", "a.go", first_source);
    let reverse_package = reverse.package_for_file(reverse_second).unwrap();

    assert_eq!(forward_package, reverse_package);
    assert_eq!(
        forward.analyze_package(forward_package).unwrap(),
        reverse.analyze_package(reverse_package).unwrap()
    );
}

#[test]
fn package_public_api_stays_green_after_an_exported_function_body_edit() {
    let original = "package sample\nfunc Exported(x int) int { return x + 1 }\n";
    let changed = "package sample\nfunc Exported(x int) int { return x + 99 }\n";
    let mut db = CompilerDatabase::default();
    let file = insert_file(&mut db, "example/sample", "sample.go", original);
    let package = db.package_for_file(file).unwrap();
    let before = db.analyze_package(package).unwrap();

    insert_file(&mut db, "example/sample", "sample.go", changed);
    db.reset_telemetry();
    let after = db.analyze_package(package).unwrap();

    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(
        before.public_api_fingerprint(),
        after.public_api_fingerprint()
    );
    assert_eq!(db.telemetry().executions(QueryKind::FileProjection), 1);
    assert_eq!(db.telemetry().executions(QueryKind::PackageAnalysis), 0);
}

#[test]
fn exported_signature_edit_invalidates_package_public_api() {
    let original = "package sample\nfunc Exported(x int) int { return x + 1 }\n";
    let changed = "package sample\nfunc Exported(x bool) int { return 1 }\n";
    let mut db = CompilerDatabase::default();
    let file = insert_file(&mut db, "example/sample", "sample.go", original);
    let package = db.package_for_file(file).unwrap();
    let before = db.analyze_package(package).unwrap();

    insert_file(&mut db, "example/sample", "sample.go", changed);
    db.reset_telemetry();
    let after = db.analyze_package(package).unwrap();

    assert!(!Arc::ptr_eq(&before, &after));
    assert_ne!(
        before.public_api_fingerprint(),
        after.public_api_fingerprint()
    );
    assert_eq!(db.telemetry().executions(QueryKind::PackageAnalysis), 1);
}

#[test]
fn package_index_reports_cross_file_duplicate_declarations() {
    let mut db = CompilerDatabase::default();
    let first = insert_file(
        &mut db,
        "example/sample",
        "a.go",
        "package sample\nfunc Duplicate() {}\n",
    );
    insert_file(
        &mut db,
        "example/sample",
        "b.go",
        "package sample\nfunc Duplicate() {}\n",
    );
    let package = db.package_for_file(first).unwrap();
    let analysis = db.analyze_package(package).unwrap();

    assert!(analysis.issues().iter().any(|issue| matches!(
        issue,
        PackageIssue::DuplicateDefinition { name, first_file, second_file }
            if name.as_ref() == "Duplicate" && first_file != second_file
    )));
}

#[test]
fn repeated_init_projection_waits_for_stable_syntax_disambiguators() {
    let mut db = CompilerDatabase::default();
    let file = insert_file(
        &mut db,
        "example/sample",
        "init.go",
        "package sample\nfunc init() {}\nfunc init() {}\n",
    );
    let package = db.package_for_file(file).unwrap();
    let analysis = db.analyze_package(package).unwrap();

    assert!(analysis.issues().iter().any(|issue| matches!(
        issue,
        PackageIssue::DuplicateDefinition {
            name,
            first_file,
            second_file,
        } if name.as_ref() == "init" && first_file == second_file
    )));
}

#[test]
fn package_identity_isolates_identical_clauses_and_names() {
    let source = "package shared\nfunc Exported() int { return 1 }\n";
    let mut db = CompilerDatabase::default();
    let first_file = insert_file(&mut db, "example/one", "main.go", source);
    let second_file = insert_file(&mut db, "example/two", "main.go", source);
    let first_package = db.package_for_file(first_file).unwrap();
    let second_package = db.package_for_file(second_file).unwrap();
    let first = db.analyze_package(first_package).unwrap();
    let second = db.analyze_package(second_package).unwrap();

    assert_ne!(first_package, second_package);
    assert_ne!(
        first.functions().first().unwrap().id(),
        second.functions().first().unwrap().id()
    );
    assert_ne!(
        first.public_api_fingerprint(),
        second.public_api_fingerprint()
    );
}

#[test]
fn moving_a_named_declaration_between_package_files_preserves_its_id() {
    let mut db = CompilerDatabase::default();
    let first = insert_file(
        &mut db,
        "example/sample",
        "a.go",
        "package sample\nfunc Exported() int { return 1 }\n",
    );
    insert_file(&mut db, "example/sample", "b.go", "package sample\n");
    let package = db.package_for_file(first).unwrap();
    let before = db.analyze_package(package).unwrap();
    let before_id = before.functions().first().unwrap().id();
    let before_api = before.public_api_fingerprint();

    insert_file(&mut db, "example/sample", "a.go", "package sample\n");
    insert_file(
        &mut db,
        "example/sample",
        "b.go",
        "package sample\nfunc Exported() int { return 1 }\n",
    );
    let after = db.analyze_package(package).unwrap();

    assert_eq!(after.functions().first().unwrap().id(), before_id);
    assert_eq!(after.public_api_fingerprint(), before_api);
}
