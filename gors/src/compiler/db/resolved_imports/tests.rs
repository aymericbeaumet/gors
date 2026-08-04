#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use super::{ResolvedFileImports, ResolvedImport, ResolvedImportBinding};
use crate::compiler::db::{CompilerDatabase, QueryError};
use crate::compiler::ids::{FileId, PackageId};
use crate::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};

const IMPORTER_SOURCE: &str = r#"package importer
import (
    "example.test/default"
    alias "example.test/named"
    _ "example.test/blank"
    . "example.test/dot"
)
"#;

struct ImportFixture {
    database: CompilerDatabase,
    file: FileId,
    resolved: Arc<ResolvedFileImports>,
}

fn insert_package(
    database: &mut CompilerDatabase,
    workspace: &WorkspaceKey,
    import_path: &str,
    package_name: &str,
) -> PackageId {
    let package = PackageKey::import_path(import_path).unwrap();
    let logical_path = format!("{package_name}.go");
    let snapshot = Arc::new(
        SourceSnapshot::from_source(logical_path.clone(), format!("package {package_name}\n"))
            .unwrap(),
    );
    let file = database
        .set_source(workspace, &package, &logical_path, snapshot)
        .unwrap()
        .file();
    database.package_for_file(file).unwrap()
}

fn import_fixture() -> ImportFixture {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let targets = [
        (
            insert_package(
                &mut database,
                &workspace,
                "example.test/default",
                "actual_default",
            ),
            "actual_default",
        ),
        (
            insert_package(
                &mut database,
                &workspace,
                "example.test/named",
                "named_target",
            ),
            "named_target",
        ),
        (
            insert_package(
                &mut database,
                &workspace,
                "example.test/blank",
                "blank_target",
            ),
            "blank_target",
        ),
        (
            insert_package(&mut database, &workspace, "example.test/dot", "dot_target"),
            "dot_target",
        ),
    ];
    let importer = PackageKey::import_path("example.test/importer").unwrap();
    let file = database
        .set_source(
            &workspace,
            &importer,
            "imports.go",
            Arc::new(SourceSnapshot::from_source("/checkout/imports.go", IMPORTER_SOURCE).unwrap()),
        )
        .unwrap()
        .file();
    let source_imports = database.file_imports(file).unwrap();
    let resolved = source_imports
        .direct()
        .iter()
        .zip(targets)
        .map(|(occurrence, (target, package_name))| {
            ResolvedImport::from_occurrence(target, package_name, occurrence)
        })
        .collect::<Vec<_>>();
    let resolved = Arc::new(ResolvedFileImports::try_new(file, resolved).unwrap());
    ImportFixture {
        database,
        file,
        resolved,
    }
}

#[test]
fn resolved_bindings_preserve_default_name_alias_blank_and_dot() {
    let mut fixture = import_fixture();
    let update = fixture
        .database
        .set_resolved_file_imports(Arc::clone(&fixture.resolved))
        .unwrap();
    assert_eq!(update.file(), fixture.file);
    assert!(update.changed());
    assert!(update.inserted());

    let installed = fixture
        .database
        .resolved_file_imports(fixture.file)
        .unwrap();
    let mut imports = installed.imports().iter();
    let default = imports.next().unwrap();
    let named = imports.next().unwrap();
    let blank = imports.next().unwrap();
    let dot = imports.next().unwrap();

    assert_eq!(default.canonical_path().as_str(), "example.test/default");
    assert_eq!(default.binding().local_name(), Some("actual_default"));
    assert!(matches!(
        default.binding(),
        ResolvedImportBinding::Default { .. }
    ));
    assert_eq!(named.binding().local_name(), Some("alias"));
    assert!(matches!(
        named.binding(),
        ResolvedImportBinding::Named { .. }
    ));
    assert_eq!(blank.binding().local_name(), None);
    assert!(matches!(
        blank.binding(),
        ResolvedImportBinding::Blank { .. }
    ));
    assert_eq!(dot.binding().local_name(), None);
    assert!(matches!(dot.binding(), ResolvedImportBinding::Dot { .. }));
    assert_eq!(
        fixture.database.active_resolved_import_files(),
        [fixture.file]
    );
    assert!(fixture.database.retained_resolved_import_bytes() > 0);
}

#[test]
fn structurally_equal_update_is_a_salsa_no_op_but_alias_changes_are_not() {
    let mut fixture = import_fixture();
    fixture
        .database
        .set_resolved_file_imports(Arc::clone(&fixture.resolved))
        .unwrap();
    let before = fixture
        .database
        .resolved_file_imports(fixture.file)
        .unwrap();
    fixture.database.reset_telemetry();

    let equal = Arc::new(fixture.resolved.as_ref().clone());
    let update = fixture.database.set_resolved_file_imports(equal).unwrap();
    assert!(!update.changed());
    assert!(!update.inserted());
    let after = fixture
        .database
        .resolved_file_imports(fixture.file)
        .unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(
        fixture.database.telemetry().engine().cancellation_requests,
        0
    );

    let mut changed = fixture.resolved.imports().to_vec();
    let named = changed.get(1).unwrap().clone();
    *changed.get_mut(1).unwrap() = ResolvedImport::try_new(
        named.target_package(),
        named.canonical_path().clone(),
        ResolvedImportBinding::Named {
            local_name: Arc::from("other_alias"),
            source: named.binding().source(),
        },
        named.source(),
    )
    .unwrap();
    let changed = Arc::new(ResolvedFileImports::try_new(fixture.file, changed).unwrap());
    let update = fixture.database.set_resolved_file_imports(changed).unwrap();
    assert!(update.changed());
    assert!(!update.inserted());
    assert_eq!(
        fixture
            .database
            .resolved_file_imports(fixture.file)
            .unwrap()
            .imports()
            .get(1)
            .unwrap()
            .binding()
            .local_name(),
        Some("other_alias")
    );
}

#[test]
fn insertion_and_removal_roll_back_exactly_and_committed_removal_evicts_payload() {
    let mut fixture = import_fixture();
    let (_, insertion) = fixture
        .database
        .set_resolved_file_imports_transactional(Arc::clone(&fixture.resolved))
        .unwrap();
    assert!(fixture.database.resolved_file_imports(fixture.file).is_ok());
    fixture
        .database
        .rollback_resolved_import_mutations(insertion);
    assert!(matches!(
        fixture.database.resolved_file_imports(fixture.file),
        Err(QueryError::UnknownResolvedImports(file)) if file == fixture.file
    ));

    fixture
        .database
        .set_resolved_file_imports(Arc::clone(&fixture.resolved))
        .unwrap();
    let before = fixture
        .database
        .resolved_file_imports(fixture.file)
        .unwrap();
    let removal = fixture
        .database
        .remove_resolved_file_imports_transactional(fixture.file)
        .unwrap();
    fixture
        .database
        .rollback_resolved_import_mutations(Some(removal));
    let restored = fixture
        .database
        .resolved_file_imports(fixture.file)
        .unwrap();
    assert!(Arc::ptr_eq(&before, &restored));

    fixture
        .database
        .remove_resolved_file_imports(fixture.file)
        .unwrap();
    assert!(matches!(
        fixture.database.resolved_file_imports(fixture.file),
        Err(QueryError::UnknownResolvedImports(file)) if file == fixture.file
    ));
    assert!(fixture.database.active_resolved_import_files().is_empty());
    assert_eq!(fixture.database.retained_resolved_import_bytes(), 0);
}
