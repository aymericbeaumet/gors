use super::*;

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

    // Package analysis consumes the file-granular projection and therefore
    // revalidates after a body edit, but its body-independent value stays
    // identical so public-API consumers can remain green.
    assert_eq!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        before.public_api_fingerprint(),
        after.public_api_fingerprint()
    );
    assert_eq!(db.telemetry().executions(QueryKind::FileProjection), 1);
    assert_eq!(db.telemetry().executions(QueryKind::PackageAnalysis), 1);
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
fn repeated_init_projection_uses_one_package_owned_identity_without_duplicate_issue() {
    let mut db = CompilerDatabase::default();
    let file = insert_file(
        &mut db,
        "example/sample",
        "init.go",
        "package sample\nfunc init() {}\nfunc init() {}\n",
    );
    let package = db.package_for_file(file).unwrap();
    let file_analysis = db.analyze_file(file).unwrap();
    let analysis = db.analyze_package(package).unwrap();

    let init_fragments = file_analysis
        .functions()
        .iter()
        .filter(|function| function.name() == "init")
        .collect::<Vec<_>>();
    assert_eq!(init_fragments.len(), 2);
    let init_id = init_fragments.first().unwrap().id();
    assert!(
        init_fragments
            .iter()
            .all(|fragment| fragment.id() == init_id)
    );
    assert!(file_analysis.issues().is_empty(), "{file_analysis:?}");
    assert!(analysis.issues().is_empty(), "{analysis:?}");
    assert!(analysis.functions().is_empty());
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
