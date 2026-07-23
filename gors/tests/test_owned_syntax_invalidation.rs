#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;

use gors::compiler::db::{
    CompilerDatabase, FunctionBody, FunctionSignature, NormalizedMirFunction, QueryKind,
    TypedHirFunction, VerifiedMirFunction, VerifiedRustIrFunction,
};
use gors::compiler::ids::{DefId, FileId};
use gors::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};

const TRIVIA_BASE: &str = r#"package main

func f(value int) int {
    value = value + 1
    return value
}
func main() {
    println(f(1))
}
"#;

const TRIVIA_EDIT: &str = r#"package main

// f documents f.
func /* header */ f ( value int ) int /* body */ {
    value=value+1; // statement
    return value;
}
func main () {
    println ( f ( 1 ) );
}
"#;

const BODY_BASE: &str = r#"package main

func f(value int) int { return value + 1 }
func g(value int) int { return value + 2 }
"#;

const BODY_EDIT: &str = r#"package main

func f(value int) int { return value + 10 }
func g(value int) int { return value + 2 }
"#;

const HEADER_BASE: &str = r#"package main

func f(value int) int { return value }
func g(value int) int { return value + 2 }
"#;

const HEADER_EDIT: &str = r#"package main

func f(value bool) bool { return value }
func g(value int) int { return value + 2 }
"#;

struct FunctionProducts {
    signature: Arc<FunctionSignature>,
    body: Arc<FunctionBody>,
    hir: Arc<TypedHirFunction>,
    mir: Arc<VerifiedMirFunction>,
    normalized: Arc<NormalizedMirFunction>,
    rust_ir: Arc<VerifiedRustIrFunction>,
}

impl FunctionProducts {
    fn read(db: &CompilerDatabase, file: FileId, function: DefId) -> Self {
        Self {
            signature: db.function_signature(file, function).unwrap(),
            body: db.function_body(file, function).unwrap(),
            hir: db.typed_hir(file, function).unwrap(),
            mir: db.verified_mir(file, function).unwrap(),
            normalized: db.normalized_mir(file, function).unwrap(),
            rust_ir: db.verified_rust_ir(file, function).unwrap(),
        }
    }

    fn assert_pipeline_reused(&self, current: &Self) {
        assert!(Arc::ptr_eq(&self.hir, &current.hir));
        assert!(Arc::ptr_eq(&self.mir, &current.mir));
        assert!(Arc::ptr_eq(&self.normalized, &current.normalized));
        assert!(Arc::ptr_eq(&self.rust_ir, &current.rust_ir));
    }

    fn assert_pipeline_replaced(&self, current: &Self) {
        assert!(!Arc::ptr_eq(&self.hir, &current.hir));
        assert!(!Arc::ptr_eq(&self.mir, &current.mir));
        assert!(!Arc::ptr_eq(&self.normalized, &current.normalized));
        assert!(!Arc::ptr_eq(&self.rust_ir, &current.rust_ir));
    }
}

fn workspace() -> WorkspaceKey {
    WorkspaceKey::AdHoc("owned-syntax-invalidation".into())
}

fn package() -> PackageKey {
    PackageKey::ImportPath("example/owned-syntax".into())
}

fn install(db: &mut CompilerDatabase, source: &str) -> FileId {
    db.set_source(
        &workspace(),
        &package(),
        "main.go",
        Arc::new(SourceSnapshot::from_source("main.go", source).unwrap()),
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
fn trivia_reexecutes_file_work_but_backdates_owned_syntax_and_pipeline() {
    let mut db = CompilerDatabase::default();
    let file = install(&mut db, TRIVIA_BASE);
    let function = function_id(&db, file, "f");
    let previous = FunctionProducts::read(&db, file, function);
    let previous_layout = db.function_layout(file, function).unwrap();
    let previous_source_table = db.definition_source_table(file, function).unwrap();

    install(&mut db, TRIVIA_EDIT);
    db.reset_telemetry();

    let current = FunctionProducts::read(&db, file, function);
    let current_layout = db.function_layout(file, function).unwrap();
    let current_source_table = db.definition_source_table(file, function).unwrap();

    assert!(Arc::ptr_eq(&previous.signature, &current.signature));
    assert!(Arc::ptr_eq(&previous.body, &current.body));
    previous.assert_pipeline_reused(&current);
    assert!(!Arc::ptr_eq(&previous_layout, &current_layout));
    assert!(!Arc::ptr_eq(&previous_source_table, &current_source_table));

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionLayout), 1);
    assert_eq!(telemetry.executions(QueryKind::DefinitionSourceTable), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 0);
    assert_eq!(telemetry.executions(QueryKind::FunctionBody), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 0);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 0);
}

#[test]
fn body_edit_invalidates_only_body_and_changed_function_pipeline() {
    let mut db = CompilerDatabase::default();
    let file = install(&mut db, BODY_BASE);
    let f = function_id(&db, file, "f");
    let g = function_id(&db, file, "g");
    let previous_f = FunctionProducts::read(&db, file, f);
    let previous_g = FunctionProducts::read(&db, file, g);

    install(&mut db, BODY_EDIT);
    db.reset_telemetry();

    let current_f = FunctionProducts::read(&db, file, f);
    let current_g = FunctionProducts::read(&db, file, g);

    assert!(Arc::ptr_eq(&previous_f.signature, &current_f.signature));
    assert!(!Arc::ptr_eq(&previous_f.body, &current_f.body));
    previous_f.assert_pipeline_replaced(&current_f);
    assert!(Arc::ptr_eq(&previous_g.signature, &current_g.signature));
    assert!(Arc::ptr_eq(&previous_g.body, &current_g.body));
    previous_g.assert_pipeline_reused(&current_g);

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 0);
    assert_eq!(telemetry.executions(QueryKind::FunctionBody), 1);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
}

#[test]
fn header_edit_invalidates_only_signature_and_changed_function_pipeline() {
    let mut db = CompilerDatabase::default();
    let file = install(&mut db, HEADER_BASE);
    let f = function_id(&db, file, "f");
    let g = function_id(&db, file, "g");
    let previous_f = FunctionProducts::read(&db, file, f);
    let previous_g = FunctionProducts::read(&db, file, g);
    let previous_public_api = db.public_api(file).unwrap();

    install(&mut db, HEADER_EDIT);
    db.reset_telemetry();

    let current_f = FunctionProducts::read(&db, file, f);
    let current_g = FunctionProducts::read(&db, file, g);
    let current_public_api = db.public_api(file).unwrap();

    assert!(!Arc::ptr_eq(&previous_f.signature, &current_f.signature));
    assert!(Arc::ptr_eq(&previous_f.body, &current_f.body));
    previous_f.assert_pipeline_replaced(&current_f);
    assert!(Arc::ptr_eq(&previous_g.signature, &current_g.signature));
    assert!(Arc::ptr_eq(&previous_g.body, &current_g.body));
    previous_g.assert_pipeline_reused(&current_g);
    assert!(!Arc::ptr_eq(&previous_public_api, &current_public_api));

    let telemetry = db.telemetry();
    assert_eq!(telemetry.executions(QueryKind::FileProjection), 1);
    assert_eq!(telemetry.executions(QueryKind::PublicApi), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionSignature), 1);
    assert_eq!(telemetry.executions(QueryKind::FunctionBody), 0);
    assert_eq!(telemetry.executions(QueryKind::TypedHir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::NormalizedGoMir), 1);
    assert_eq!(telemetry.executions(QueryKind::VerifiedRustIr), 1);
}
