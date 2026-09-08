#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;

use gors::compiler::db::{CompilerDatabase, QueryError, QueryKind};
use gors::compiler::ids::{DefId, FileId};
use gors::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};

fn workspace() -> WorkspaceKey {
    WorkspaceKey::AdHoc("owned-semantic-dependencies".into())
}

fn package() -> PackageKey {
    PackageKey::import_path("example/semantic-dependencies").unwrap()
}

fn install(db: &mut CompilerDatabase, logical_path: &str, source: &str) -> FileId {
    db.set_source(
        &workspace(),
        &package(),
        logical_path,
        Arc::new(SourceSnapshot::from_source(logical_path, source).unwrap()),
    )
    .unwrap()
    .file()
}

fn function_id(db: &CompilerDatabase, file: FileId, name: &str) -> DefId {
    db.analyze_file(file)
        .unwrap()
        .functions()
        .iter()
        .find(|function| function.name() == name)
        .map(|function| function.id())
        .expect("function should exist")
}

#[test]
fn lexical_bindings_do_not_create_false_package_dependencies() {
    let before = r#"package main
func f() int { return 1 }
func parameter(f int) int { return f }
func local() int { f := 2; return f }
"#;
    let after = r#"package main
func f(value int) int { return value }
func parameter(f int) int { return f }
func local() int { f := 2; return f }
"#;
    let mut db = CompilerDatabase::default();
    let file = install(&mut db, "main.go", before);
    let parameter = function_id(&db, file, "parameter");
    let local = function_id(&db, file, "local");
    let old_parameter = db.typed_hir(file, parameter).unwrap();
    let old_local = db.typed_hir(file, local).unwrap();

    install(&mut db, "main.go", after);
    db.reset_telemetry();

    assert!(Arc::ptr_eq(
        &old_parameter,
        &db.typed_hir(file, parameter).unwrap()
    ));
    assert!(Arc::ptr_eq(&old_local, &db.typed_hir(file, local).unwrap()));
    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::PackageFunctionLookup), 0);
}

#[test]
fn constants_resolve_forward_and_cross_file_references() {
    let mut db = CompilerDatabase::default();
    let entry = install(
        &mut db,
        "entry.go",
        r#"package main
const Forward = Later + 1
func value() int { return Forward + CrossFile }
"#,
    );
    install(
        &mut db,
        "constants.go",
        r#"package main
const CrossFile = 1
const Later = 40
"#,
    );
    let value = function_id(&db, entry, "value");
    let hir = db.typed_hir(entry, value).unwrap();
    assert_eq!(hir.function().name, "value");
}

#[test]
fn type_aliases_resolve_transitively_across_files() {
    let mut db = CompilerDatabase::default();
    let entry = install(
        &mut db,
        "entry.go",
        "package main\nfunc value() Count { var result Count = 6; return result }\n",
    );
    install(
        &mut db,
        "aliases.go",
        "package main\ntype Count = Number\ntype Number = int\n",
    );
    let value = function_id(&db, entry, "value");
    let signature = db.typed_signature(entry, value).unwrap();
    assert_eq!(
        signature.signature().results,
        [gors::compiler::types::Ty::Int(
            gors::compiler::types::IntTy::Int,
        )]
    );
    assert_eq!(db.typed_hir(entry, value).unwrap().function().name, "value");
}

#[test]
fn type_alias_cycles_fail_deterministically() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        "package main\ntype A = B\ntype B = A\nfunc value() int { return 1 }\n",
    );
    let value = function_id(&db, file, "value");
    let failure = match db.typed_hir(file, value).unwrap_err() {
        QueryError::StageFailure(failure) => failure,
        error => panic!("unexpected query error: {error}"),
    };
    assert_eq!(
        failure.diagnostics().first().unwrap().message,
        "type declaration cycle: A -> B -> A"
    );
}

#[test]
fn aliases_of_defined_types_preserve_the_definition_identity() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        "package main\ntype Count int\ntype Alias = Count\nfunc value() Alias { return Alias(6) }\n",
    );
    let value = function_id(&db, file, "value");
    let signature = db.typed_signature(file, value).unwrap();
    let [
        gors::compiler::types::Ty::Named {
            identity,
            underlying,
        },
    ] = signature.signature().results.as_slice()
    else {
        panic!("alias should resolve to its defined type")
    };
    assert_eq!(
        **underlying,
        gors::compiler::types::Ty::Int(gors::compiler::types::IntTy::Int)
    );
    let analysis = db
        .analyze_package(db.package_for_file(file).unwrap())
        .unwrap();
    assert_eq!(analysis.type_definitions().len(), 1);
    assert_eq!(
        analysis
            .type_definitions()
            .first()
            .expect("defined type descriptor")
            .id(),
        identity.definition().definition()
    );
    assert_eq!(analysis.type_aliases().len(), 1);
}

#[test]
fn aggregate_and_function_types_enter_typed_signatures() {
    use gors::compiler::types::{IntTy, Ty};

    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        concat!(
            "package main\n",
            "type Counter struct { Value int `json:\"value\"` }\n",
            "type Line struct { Points []Counter }\n",
            "type Reader interface { Read() int }\n",
            "func consume(value Line, callback func(int) string, reader Reader) {}\n",
        ),
    );
    let consume = function_id(&db, file, "consume");
    let signature = db.typed_signature(file, consume).unwrap();
    let [counter, callback, reader] = signature.signature().params.as_slice() else {
        panic!("consume should have three parameters")
    };

    let Ty::Named { underlying, .. } = counter else {
        panic!("Line should retain its defined-type identity")
    };
    let Ty::Struct(fields) = underlying.as_ref() else {
        panic!("Line should retain its exact struct layout")
    };
    let [field] = fields.as_slice() else {
        panic!("Line should retain one field")
    };
    assert_eq!(field.name, "Points");
    let Ty::Slice(counter) = &field.ty else {
        panic!("Line.Points should retain its slice element type")
    };
    let Ty::Named { underlying, .. } = counter.as_ref() else {
        panic!("Line.Points should retain Counter's defined-type identity")
    };
    let Ty::Struct(fields) = underlying.as_ref() else {
        panic!("Counter should retain its exact struct layout")
    };
    let [field] = fields.as_slice() else {
        panic!("Counter should retain one field")
    };
    assert_eq!(field.name, "Value");
    assert_eq!(field.ty, Ty::Int(IntTy::Int));
    assert_eq!(field.tag.as_deref(), Some("`json:\"value\"`"));

    let Ty::Function(callback) = callback else {
        panic!("callback should retain its function signature")
    };
    assert_eq!(callback.params, [Ty::Int(IntTy::Int)]);
    assert_eq!(callback.results, [Ty::String]);

    let Ty::Named { underlying, .. } = reader else {
        panic!("Reader should retain its defined-type identity")
    };
    let Ty::Interface(methods) = underlying.as_ref() else {
        panic!("Reader should retain its exact method set")
    };
    let [method] = methods.as_slice() else {
        panic!("Reader should retain one method")
    };
    assert_eq!(method.name, "Read");
    assert!(method.signature.params.is_empty());
    assert_eq!(method.signature.results, [Ty::Int(IntTy::Int)]);
}

#[test]
fn constant_cycles_fail_deterministically() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        r#"package main
const A = B
const B = A
func value() int { return A }
"#,
    );
    let value = function_id(&db, file, "value");
    let constant_a = db
        .analyze_file(file)
        .unwrap()
        .constants()
        .iter()
        .find(|constant| constant.name() == "A")
        .unwrap()
        .id();
    let failure = match db.typed_hir(file, value).unwrap_err() {
        QueryError::StageFailure(failure) => failure,
        error => panic!("unexpected query error: {error}"),
    };
    assert_eq!(failure.definition(), Some(constant_a));
    assert_eq!(
        failure.diagnostics().first().unwrap().message,
        "constant initialization cycle: A -> B -> A"
    );

    db.reset_telemetry();
    let repeated = match db.typed_hir(file, value).unwrap_err() {
        QueryError::StageFailure(failure) => failure,
        error => panic!("unexpected query error: {error}"),
    };
    assert!(Arc::ptr_eq(&failure, &repeated));
    assert_eq!(db.telemetry().total_executions(), 0);
}

#[test]
fn exported_constant_semantics_participate_in_package_api() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        "package main\nconst Exported = 1\nconst hidden = 1\n",
    );
    let package = db.package_for_file(file).unwrap();
    let base = db.analyze_package(package).unwrap();
    assert_eq!(base.constants().len(), 2);

    install(
        &mut db,
        "main.go",
        "package main\nconst Exported = 1\nconst hidden = 2\n",
    );
    let hidden_edit = db.analyze_package(package).unwrap();
    assert_eq!(
        base.public_api_fingerprint(),
        hidden_edit.public_api_fingerprint()
    );

    install(
        &mut db,
        "main.go",
        "package main\nconst Exported = 2\nconst hidden = 2\n",
    );
    let exported_edit = db.analyze_package(package).unwrap();
    assert_ne!(
        hidden_edit.public_api_fingerprint(),
        exported_edit.public_api_fingerprint()
    );
}

#[test]
fn exported_type_alias_semantics_participate_in_package_api() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        "package main\ntype Exported = int\ntype hidden = int\n",
    );
    let package = db.package_for_file(file).unwrap();
    let base = db.analyze_package(package).unwrap();
    assert_eq!(base.type_aliases().len(), 2);
    assert_eq!(db.analyze_file(file).unwrap().type_aliases().len(), 2);

    install(
        &mut db,
        "main.go",
        "package main\ntype Exported = int\ntype hidden = bool\n",
    );
    let hidden_edit = db.analyze_package(package).unwrap();
    assert_eq!(
        base.public_api_fingerprint(),
        hidden_edit.public_api_fingerprint()
    );

    install(
        &mut db,
        "main.go",
        "package main\ntype Exported = bool\ntype hidden = bool\n",
    );
    let exported_edit = db.analyze_package(package).unwrap();
    assert_ne!(
        hidden_edit.public_api_fingerprint(),
        exported_edit.public_api_fingerprint()
    );
}

#[test]
fn function_hir_depends_only_on_referenced_constants() {
    let mut db = CompilerDatabase::default();
    let file = install(
        &mut db,
        "main.go",
        "package main\nconst Used = 1\nconst Unused = 10\nfunc value() int { return Used }\n",
    );
    let value = function_id(&db, file, "value");
    let base = db.typed_hir(file, value).unwrap();

    install(
        &mut db,
        "main.go",
        "package main\nconst Used = 1\nconst Unused = 20\nfunc value() int { return Used }\n",
    );
    db.reset_telemetry();
    let unused_edit = db.typed_hir(file, value).unwrap();
    assert!(Arc::ptr_eq(&base, &unused_edit));
    assert_eq!(db.telemetry().executions(QueryKind::TypedHir), 0);

    install(
        &mut db,
        "main.go",
        "package main\nconst Used = 2\nconst Unused = 20\nfunc value() int { return Used }\n",
    );
    db.reset_telemetry();
    let used_edit = db.typed_hir(file, value).unwrap();
    assert!(!Arc::ptr_eq(&unused_edit, &used_edit));
    assert_eq!(db.telemetry().executions(QueryKind::TypedHir), 1);
}
