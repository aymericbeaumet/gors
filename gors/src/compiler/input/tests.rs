#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;

use crate::parser::SourceSnapshot;

use super::{
    InputError, LogicalPathIssue, PackageInputManifest, PackageKey, ProgramInput, SourceFileInput,
    WorkspaceKey,
};

fn workspace() -> WorkspaceKey {
    WorkspaceKey::ad_hoc("workspace").unwrap()
}

fn package(name: &str) -> PackageKey {
    PackageKey::import_path(name).unwrap()
}

fn source(logical_path: &str, diagnostic_path: &str, text: &str) -> SourceFileInput {
    SourceFileInput::from_source(logical_path, diagnostic_path, text).unwrap()
}

fn manifest(name: &str, files: Vec<SourceFileInput>) -> PackageInputManifest {
    PackageInputManifest::new(package(name), files).unwrap()
}

#[test]
fn canonicalizes_package_and_file_order() {
    let input = ProgramInput::new(
        workspace(),
        package("z.example/entry"),
        [
            manifest(
                "z.example/entry",
                vec![
                    source("z.go", "/checkout/z.go", "package main"),
                    source("nested/a.go", "/checkout/nested/a.go", "package main"),
                ],
            ),
            manifest(
                "a.example/dependency",
                vec![source("dep.go", "/checkout/dep.go", "package dependency")],
            ),
        ],
    )
    .unwrap();

    assert_eq!(
        input
            .packages()
            .iter()
            .filter_map(|package| package.key().as_import_path())
            .collect::<Vec<_>>(),
        ["a.example/dependency", "z.example/entry"]
    );
    assert_eq!(
        input.entry_package().key().as_import_path(),
        Some("z.example/entry")
    );
    assert_eq!(
        input
            .entry_package()
            .files()
            .iter()
            .map(SourceFileInput::logical_path)
            .collect::<Vec<_>>(),
        ["nested/a.go", "z.go"]
    );
}

#[test]
fn rejects_duplicate_package_keys() {
    let duplicate = package("example/duplicate");
    let error = ProgramInput::new(
        workspace(),
        duplicate.clone(),
        [
            manifest(
                "example/duplicate",
                vec![source("one.go", "/one.go", "package duplicate")],
            ),
            manifest(
                "example/duplicate",
                vec![source("two.go", "/two.go", "package duplicate")],
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(
        error,
        InputError::DuplicatePackageKey { package: duplicate }
    );
}

#[test]
fn rejects_duplicate_logical_paths_independent_of_display_path() {
    let key = package("example/package");
    let error = PackageInputManifest::new(
        key.clone(),
        [
            source("same.go", "/checkout-one/same.go", "package p"),
            source("same.go", "/checkout-two/same.go", "package p"),
        ],
    )
    .unwrap_err();
    assert_eq!(
        error,
        InputError::DuplicateLogicalPath {
            package: key,
            path: Arc::from("same.go"),
        }
    );
}

#[test]
fn rejects_missing_entry_package() {
    let missing = package("example/missing");
    let error = ProgramInput::new(
        workspace(),
        missing.clone(),
        [manifest(
            "example/present",
            vec![source("present.go", "/present.go", "package present")],
        )],
    )
    .unwrap_err();
    assert_eq!(error, InputError::MissingEntryPackage { package: missing });
}

#[test]
fn rejects_empty_keys() {
    assert_eq!(
        WorkspaceKey::module("").unwrap_err(),
        InputError::EmptyWorkspaceKey
    );
    assert_eq!(
        PackageKey::import_path("").unwrap_err(),
        InputError::EmptyPackageKey
    );
}

#[test]
fn rejects_direct_empty_variants_and_packages_without_files() {
    let command_line = PackageKey::command_line();
    let no_files = PackageInputManifest::new(command_line.clone(), []).unwrap_err();
    assert_eq!(
        no_files,
        InputError::PackageHasNoFiles {
            package: command_line.clone(),
        }
    );

    let entry = PackageInputManifest::new(
        command_line.clone(),
        [source("main.go", "/main.go", "package main")],
    )
    .unwrap();
    assert_eq!(
        ProgramInput::new(WorkspaceKey::Module(Arc::from("")), command_line, [entry],).unwrap_err(),
        InputError::EmptyWorkspaceKey
    );
    assert_eq!(
        PackageInputManifest::new(
            PackageKey::ImportPath(Arc::from("")),
            [source("invalid.go", "/invalid.go", "package invalid")],
        )
        .unwrap_err(),
        InputError::EmptyPackageKey
    );
}

#[test]
fn rejects_noncanonical_logical_paths() {
    let cases = [
        ("", LogicalPathIssue::Empty),
        ("/main.go", LogicalPathIssue::Absolute),
        ("C:/main.go", LogicalPathIssue::WindowsDrivePrefix),
        ("nested\\main.go", LogicalPathIssue::Backslash),
        ("nested//main.go", LogicalPathIssue::EmptyComponent),
        ("nested/", LogicalPathIssue::EmptyComponent),
        ("./main.go", LogicalPathIssue::CurrentDirectoryComponent),
        (
            "nested/../main.go",
            LogicalPathIssue::ParentDirectoryComponent,
        ),
    ];
    for (path, issue) in cases {
        let snapshot = Arc::new(SourceSnapshot::from_source(path, "package main"));
        assert_eq!(
            SourceFileInput::new(path, snapshot).unwrap_err(),
            InputError::InvalidLogicalPath {
                path: Arc::from(path),
                issue,
            }
        );
    }
}

#[test]
fn accepts_syntax_invalid_source_without_parsing_it() {
    let file = source(
        "main.go",
        "browser://workspace/main.go",
        "package main\nfunc {",
    );
    assert!(file.snapshot().parse().is_err());

    let input = ProgramInput::new(
        workspace(),
        PackageKey::command_line(),
        [PackageInputManifest::new(PackageKey::command_line(), [file]).unwrap()],
    )
    .unwrap();
    assert!(input.entry_package().key().is_command_line());
}
