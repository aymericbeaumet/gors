#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use gors::compiler::db::{BuildConfig, CompilerDatabase, FileAnalysis, PackageIssue, QueryKind};
use gors::compiler::ids::{DefId, FileId};
use gors::parser::SourceSnapshot;

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

fn source(text: &str) -> Arc<SourceSnapshot> {
    Arc::new(SourceSnapshot::from_source("main.go", text))
}

fn insert(db: &mut CompilerDatabase, text: &str) -> FileId {
    db.set_source("workspace", "example/main", "main.go", source(text))
        .unwrap()
}

fn insert_file(
    db: &mut CompilerDatabase,
    import_path: &str,
    logical_path: &str,
    text: &str,
) -> FileId {
    db.set_source(
        "workspace",
        import_path,
        logical_path,
        Arc::new(SourceSnapshot::from_source(logical_path, text)),
    )
    .unwrap()
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
    assert_eq!(gors::RUNTIME_ABI_ID, "gors-runtime-abi-v2");
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
    assert_eq!(db.telemetry().total_executions(), 1);
}

#[test]
fn parse_failure_is_recoverable_input_state() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, "package main\nfunc broken(");
    let failed = db.analyze_file(file).unwrap();
    assert!(failed.failure().is_some());
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
fn parse_failure_products_ignore_checkout_paths() {
    let invalid = "package main\nfunc broken(";
    let mut first = CompilerDatabase::default();
    let mut second = CompilerDatabase::default();
    let first_file = first
        .set_source(
            "workspace",
            "example/main",
            "main.go",
            Arc::new(SourceSnapshot::from_source(
                "/checkout/one/main.go",
                invalid,
            )),
        )
        .unwrap();
    let second_file = second
        .set_source(
            "workspace",
            "example/main",
            "main.go",
            Arc::new(SourceSnapshot::from_source(
                "/checkout/two/main.go",
                invalid,
            )),
        )
        .unwrap();

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
    let first_bytes = first_source.retained_bytes();
    let first_file = db
        .set_source(
            "workspace",
            "example/main",
            "first.go",
            Arc::clone(&first_source),
        )
        .unwrap();
    let second_source = Arc::new(SourceSnapshot::from_source(
        "second.go",
        "package main\nfunc h() {}\n",
    ));
    let second_bytes = second_source.retained_bytes();
    let second_file = db
        .set_source(
            "workspace",
            "example/main",
            "second.go",
            Arc::clone(&second_source),
        )
        .unwrap();
    let _ = db.analyze_file(first_file).unwrap();
    let _ = db.analyze_file(second_file).unwrap();
    assert_eq!(
        db.retained_source_bytes(),
        first_bytes.saturating_add(second_bytes)
    );

    drop(first_source);
    db.remove_source(first_file).unwrap();
    assert!(first_weak.upgrade().is_none());
    assert_eq!(db.retained_source_bytes(), second_bytes);
    assert!(db.analyze_file(first_file).is_err());
    assert!(db.analyze_file(second_file).is_ok());
}

#[test]
fn read_only_snapshots_produce_deterministic_parallel_results() {
    let mut db = CompilerDatabase::default();
    let file = insert(&mut db, ORIGINAL);
    let functions = functions(&db.analyze_file(file).unwrap());
    let f = function_id(&functions, "f");
    let first = db.snapshot();
    let second = db.snapshot();

    let first_worker = std::thread::spawn(move || {
        (
            first.public_api(file).unwrap(),
            first.function_signature(file, f).unwrap(),
            first.function_body(file, f).unwrap(),
        )
    });
    let second_worker = std::thread::spawn(move || {
        (
            second.public_api(file).unwrap(),
            second.function_signature(file, f).unwrap(),
            second.function_body(file, f).unwrap(),
        )
    });

    assert_eq!(first_worker.join().unwrap(), second_worker.join().unwrap());
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
    assert_ne!(first.functions()[0].id(), second.functions()[0].id());
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
    let before_id = before.functions()[0].id();
    let before_api = before.public_api_fingerprint();

    insert_file(&mut db, "example/sample", "a.go", "package sample\n");
    insert_file(
        &mut db,
        "example/sample",
        "b.go",
        "package sample\nfunc Exported() int { return 1 }\n",
    );
    let after = db.analyze_package(package).unwrap();

    assert_eq!(after.functions()[0].id(), before_id);
    assert_eq!(after.public_api_fingerprint(), before_api);
}

#[test]
fn package_analysis_is_deterministic_across_worker_snapshots() {
    let mut db = CompilerDatabase::default();
    let first = insert_file(
        &mut db,
        "example/sample",
        "a.go",
        "package sample\nfunc First() int { return 1 }\n",
    );
    insert_file(
        &mut db,
        "example/sample",
        "b.go",
        "package sample\nfunc Second() int { return 2 }\n",
    );
    let package = db.package_for_file(first).unwrap();
    let first = db.snapshot();
    let second = db.snapshot();

    let first_worker = std::thread::spawn(move || first.analyze_package(package).unwrap());
    let second_worker = std::thread::spawn(move || second.analyze_package(package).unwrap());

    assert_eq!(first_worker.join().unwrap(), second_worker.join().unwrap());
}
