#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use super::ImportBinding;
use crate::compiler::db::CompilerDatabase;
use crate::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};

fn import_database(source: &str) -> (CompilerDatabase, crate::compiler::ids::FileId) {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let package = PackageKey::import_path("example.test/sample").unwrap();
    let mut database = CompilerDatabase::default();
    let file = database
        .set_source(
            &workspace,
            &package,
            "imports.go",
            Arc::new(SourceSnapshot::from_source("/checkout/imports.go", source).unwrap()),
        )
        .unwrap()
        .file();
    (database, file)
}

#[test]
fn projection_preserves_every_binding_form_and_physical_source() {
    let source = r#"package sample
import (
    "example.test/default"
    alias "example.test/named"
    _ "example.test/blank"
    . "example.test/dot"
)
"#;
    let (database, file) = import_database(source);
    let imports = database.file_imports(file).unwrap();
    assert_eq!(imports.direct().len(), 4);
    let mut direct = imports.direct().iter();
    let default = direct.next().unwrap();
    let named = direct.next().unwrap();
    let blank = direct.next().unwrap();
    let dot = direct.next().unwrap();

    assert!(matches!(default.binding(), ImportBinding::Default { .. }));
    assert!(matches!(
        named.binding(),
        ImportBinding::Named { name, .. } if name.as_ref() == "alias"
    ));
    assert!(matches!(blank.binding(), ImportBinding::Blank { .. }));
    assert!(matches!(dot.binding(), ImportBinding::Dot { .. }));
    assert_eq!(named.binding().explicit_name(), Some("alias"));
    assert_eq!(default.binding().explicit_name(), None);

    for import in imports.direct() {
        assert_eq!(import.source().file(), file);
        let range = import.source().range();
        assert_eq!(
            &source[range.start().to_usize()..range.end().to_usize()],
            import.literal()
        );
        assert_eq!(import.canonical_path().as_str(), import.path());
    }
    assert_eq!(
        &source[named.binding().source().range().start().to_usize()
            ..named.binding().source().range().end().to_usize()],
        "alias"
    );
    assert_eq!(
        &source[blank.binding().source().range().start().to_usize()
            ..blank.binding().source().range().end().to_usize()],
        "_"
    );
    assert_eq!(
        &source[dot.binding().source().range().start().to_usize()
            ..dot.binding().source().range().end().to_usize()],
        "."
    );
}

#[test]
fn invalid_imports_retain_their_physical_literal_range() {
    let source = "package sample\nimport \"\\xff\"\n";
    let (database, file) = import_database(source);
    let imports = database.file_imports(file).unwrap();
    let invalid = imports.invalid().first().unwrap();
    let range = invalid.source().range();
    assert_eq!(invalid.source().file(), file);
    assert_eq!(
        &source[range.start().to_usize()..range.end().to_usize()],
        invalid.literal()
    );
}
