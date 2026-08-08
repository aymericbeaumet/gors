#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::import_path::ImportPathIssue;
use crate::source::{TextRange, TextSize};

use super::{
    EmbeddedGoSdkPackageManifestCatalog, GoLanguageVersion, InputError,
    LayeredPackageManifestCatalog, LogicalPathIssue, PackageCatalogError, PackageInputManifest,
    PackageKey, PackageManifestCatalog, ProgramInput, SourceContent, SourceFileInput,
    SourceSnapshot, WorkspaceKey,
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

#[derive(Debug, Default)]
struct CountingCatalog {
    requests: AtomicUsize,
}

impl PackageManifestCatalog for CountingCatalog {
    fn materialize(
        &self,
        _package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        Ok(None)
    }
}

#[derive(Debug)]
struct FixedCatalog {
    requests: AtomicUsize,
    manifest: Option<Arc<PackageInputManifest>>,
}

impl FixedCatalog {
    fn owning(manifest: PackageInputManifest) -> Self {
        Self {
            requests: AtomicUsize::new(0),
            manifest: Some(Arc::new(manifest)),
        }
    }

    fn unowned() -> Self {
        Self {
            requests: AtomicUsize::new(0),
            manifest: None,
        }
    }
}

impl PackageManifestCatalog for FixedCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .manifest
            .as_ref()
            .filter(|manifest| manifest.key() == package)
            .cloned())
    }
}

#[derive(Debug, Default)]
struct FailingCatalog {
    requests: AtomicUsize,
}

impl PackageManifestCatalog for FailingCatalog {
    fn materialize(
        &self,
        package: &PackageKey,
    ) -> Result<Option<Arc<PackageInputManifest>>, PackageCatalogError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        Err(PackageCatalogError::new(
            package.clone(),
            std::io::Error::other("owned package failed"),
        ))
    }
}

#[test]
fn keeps_one_explicit_entry_and_does_not_eagerly_touch_its_catalog() {
    let catalog = Arc::new(CountingCatalog::default());
    let shared_catalog: Arc<dyn PackageManifestCatalog> = catalog.clone();
    let input = ProgramInput::new(
        workspace(),
        manifest(
            "z.example/entry",
            vec![
                source("z.go", "/checkout/z.go", "package main"),
                source("nested/a.go", "/checkout/nested/a.go", "package main"),
            ],
        ),
        shared_catalog,
    )
    .unwrap();

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
    assert_eq!(catalog.requests.load(Ordering::Relaxed), 0);
    assert!(
        input
            .package_catalog()
            .materialize(&package("a.example/dependency"))
            .unwrap()
            .is_none()
    );
    assert_eq!(catalog.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn layered_catalog_stops_at_the_first_owner_without_eagerly_touching_later_layers() {
    let requested = package("example.com/dependency");
    let first = Arc::new(FixedCatalog::owning(manifest(
        "example.com/dependency",
        vec![source("dep.go", "first://dep.go", "package dependency")],
    )));
    let second = Arc::new(FixedCatalog::owning(manifest(
        "example.com/dependency",
        vec![source("dep.go", "second://dep.go", "package dependency")],
    )));
    let layers: [Arc<dyn PackageManifestCatalog>; 2] = [first.clone(), second.clone()];
    let catalog = LayeredPackageManifestCatalog::new(layers);

    let resolved = catalog.materialize(&requested).unwrap().unwrap();

    assert_eq!(catalog.layer_count(), 2);
    assert_eq!(
        resolved
            .files()
            .first()
            .unwrap()
            .snapshot()
            .diagnostic_path(),
        "first://dep.go"
    );
    assert_eq!(first.requests.load(Ordering::Relaxed), 1);
    assert_eq!(second.requests.load(Ordering::Relaxed), 0);
}

#[test]
fn layered_catalog_falls_through_only_when_a_namespace_is_unowned() {
    let requested = package("example.com/dependency");
    let first = Arc::new(FixedCatalog::unowned());
    let second = Arc::new(FixedCatalog::owning(manifest(
        "example.com/dependency",
        vec![source("dep.go", "second://dep.go", "package dependency")],
    )));
    let layers: [Arc<dyn PackageManifestCatalog>; 2] = [first.clone(), second.clone()];
    let catalog = LayeredPackageManifestCatalog::new(layers);

    let resolved = catalog.materialize(&requested).unwrap().unwrap();

    assert_eq!(
        resolved
            .files()
            .first()
            .unwrap()
            .snapshot()
            .diagnostic_path(),
        "second://dep.go"
    );
    assert_eq!(first.requests.load(Ordering::Relaxed), 1);
    assert_eq!(second.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn layered_catalog_preserves_an_owner_error_without_falling_through() {
    let requested = package("example.com/dependency");
    let first = Arc::new(FailingCatalog::default());
    let second = Arc::new(FixedCatalog::owning(manifest(
        "example.com/dependency",
        vec![source("dep.go", "second://dep.go", "package dependency")],
    )));
    let layers: [Arc<dyn PackageManifestCatalog>; 2] = [first.clone(), second.clone()];
    let catalog = LayeredPackageManifestCatalog::new(layers);

    let error = catalog.materialize(&requested).unwrap_err();

    assert_eq!(error.package(), &requested);
    assert_eq!(error.cause().to_string(), "owned package failed");
    assert_eq!(first.requests.load(Ordering::Relaxed), 1);
    assert_eq!(second.requests.load(Ordering::Relaxed), 0);
}

#[test]
fn embedded_sdk_catalog_is_lazy_source_only_and_memoized_per_package() {
    let catalog = EmbeddedGoSdkPackageManifestCatalog::new();
    let fmt = package("fmt");

    assert_eq!(catalog.materialized_package_count(), 0);
    assert!(
        catalog
            .materialize(&PackageKey::command_line())
            .unwrap()
            .is_none()
    );
    assert!(
        catalog
            .materialize(&package("example.com/not-the-sdk"))
            .unwrap()
            .is_none()
    );
    assert_eq!(catalog.materialized_package_count(), 0);

    let first = catalog.materialize(&fmt).unwrap().unwrap();
    let second = catalog.materialize(&fmt).unwrap().unwrap();

    assert_eq!(first.key(), &fmt);
    assert!(!first.files().is_empty());
    assert!(first.files().iter().all(|file| {
        file.snapshot()
            .diagnostic_path()
            .starts_with("gors://go-sdk/fmt/")
    }));
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(catalog.materialized_package_count(), 1);
}

#[test]
fn language_versions_are_canonical_and_package_owned() {
    assert_eq!(
        GoLanguageVersion::parse("1.13.7").unwrap(),
        GoLanguageVersion::new(1, 13)
    );
    assert_eq!(
        GoLanguageVersion::parse("go1.21").unwrap().to_string(),
        "go1.21"
    );
    for invalid in ["", "1", "1.", "go", "go1.021", "v1.21", "1.21.x"] {
        assert!(GoLanguageVersion::parse(invalid).is_err(), "{invalid}");
    }

    let version = GoLanguageVersion::new(1, 12);
    let manifest = PackageInputManifest::new_with_language_version(
        package("example.com/old"),
        version,
        [source("old.go", "/old.go", "package old")],
    )
    .unwrap();
    assert_eq!(manifest.language_version(), version);
    assert_eq!(
        PackageInputManifest::new(
            package("example.com/current"),
            [source("current.go", "/current.go", "package current")],
        )
        .unwrap()
        .language_version(),
        GoLanguageVersion::current().unwrap()
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
fn rejects_empty_keys() {
    assert!(matches!(
        WorkspaceKey::module("").unwrap_err(),
        InputError::InvalidWorkspaceModulePath {
            issue: ImportPathIssue::Empty,
            ..
        }
    ));
    assert_eq!(
        WorkspaceKey::ad_hoc("").unwrap_err(),
        InputError::EmptyAdHocWorkspaceKey
    );
    assert!(matches!(
        PackageKey::import_path("").unwrap_err(),
        InputError::InvalidPackageImportPath {
            issue: ImportPathIssue::Empty,
            ..
        }
    ));
}

#[test]
fn rejects_noncanonical_package_paths_and_packages_without_files() {
    assert!(matches!(
        PackageKey::import_path("example.com/../escape").unwrap_err(),
        InputError::InvalidPackageImportPath {
            issue: ImportPathIssue::DotElement(element),
            ..
        } if element == ".."
    ));

    let command_line = PackageKey::command_line();
    let no_files = PackageInputManifest::new(command_line.clone(), []).unwrap_err();
    assert_eq!(
        no_files,
        InputError::PackageHasNoFiles {
            package: command_line.clone(),
        }
    );

    let entry = PackageInputManifest::new(
        command_line,
        [source("main.go", "/main.go", "package main")],
    )
    .unwrap();
    assert_eq!(
        ProgramInput::standalone(WorkspaceKey::AdHoc(Arc::from("")), entry).unwrap_err(),
        InputError::EmptyAdHocWorkspaceKey
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
        let snapshot = Arc::new(SourceSnapshot::from_source(path, "package main").unwrap());
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
    let snapshot = file.snapshot();
    assert!(crate::parser::parse_file(snapshot.diagnostic_path(), snapshot.source()).is_err());

    let input = ProgramInput::standalone(
        workspace(),
        PackageInputManifest::new(PackageKey::command_line(), [file]).unwrap(),
    )
    .unwrap();
    assert!(input.entry_package().key().is_command_line());
}

#[test]
fn source_file_input_rejects_lengths_outside_fixed_width_coordinates() {
    if usize::BITS <= u32::BITS {
        return;
    }
    let byte_len = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
    let logical_path = Arc::<str>::from("large.go");
    let overflow = TextSize::try_from(byte_len).unwrap_err();
    assert_eq!(
        SourceFileInput::source_size_error(&logical_path, overflow),
        InputError::SourceTooLarge {
            path: logical_path,
            byte_len,
        }
    );
}

#[test]
fn physical_coordinates_use_utf8_byte_columns_and_include_eof() {
    let content = SourceContent::from_source("αβ\nz").unwrap();
    assert_eq!(content.text_len(), TextSize::new(6));
    assert_eq!(content.text_range(), TextRange::up_to(TextSize::new(6)));

    let middle_of_beta = content
        .physical_line_column(TextSize::new(3))
        .unwrap()
        .unwrap();
    assert_eq!(middle_of_beta.line().get(), 1);
    assert_eq!(middle_of_beta.byte_column().get(), 4);

    let second_line = content
        .physical_line_column(TextSize::new(5))
        .unwrap()
        .unwrap();
    assert_eq!(second_line.line().get(), 2);
    assert_eq!(second_line.byte_column().get(), 1);

    let eof = content
        .physical_line_column(TextSize::new(6))
        .unwrap()
        .unwrap();
    assert_eq!(eof.line().get(), 2);
    assert_eq!(eof.byte_column().get(), 2);
    assert_eq!(
        content.physical_line_column(TextSize::new(7)).unwrap(),
        None
    );

    let trailing_newline = SourceContent::from_source("x\n").unwrap();
    let eof_after_newline = trailing_newline
        .physical_line_column(TextSize::new(2))
        .unwrap()
        .unwrap();
    assert_eq!(eof_after_newline.line().get(), 1);
    assert_eq!(eof_after_newline.byte_column().get(), 3);
}
