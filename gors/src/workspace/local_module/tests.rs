#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use crate::import_path::{CanonicalImportPath, ImportPathIssue};

use super::{LocalModuleCatalog, LocalModuleError, ModuleFileIssue, parse_module_directive};

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    std::fs::write(path, contents).unwrap();
}

fn module(root: &Path, module_path: &str) -> LocalModuleCatalog {
    write(
        &root.join("go.mod"),
        format!("module {module_path}\n\ngo 1.24\n"),
    );
    LocalModuleCatalog::open(root).unwrap()
}

fn import(path: &str) -> CanonicalImportPath {
    CanonicalImportPath::new(path).unwrap()
}

#[test]
fn module_reader_accepts_one_canonical_directive_and_ignores_dependencies() {
    let source = r#"
        // module example.com/ignored
        module "example.com/team/project" // canonical identity

        go 1.24
        require example.com/dependency v1.2.3
    "#;

    assert_eq!(
        parse_module_directive(source).unwrap(),
        import("example.com/team/project")
    );
}

#[test]
fn module_reader_rejects_missing_duplicate_malformed_and_invalid_directives() {
    assert_eq!(
        parse_module_directive("go 1.24\n").unwrap_err(),
        ModuleFileIssue::MissingModuleDirective
    );
    assert_eq!(
        parse_module_directive("module example.com/one\nmodule example.com/two\n").unwrap_err(),
        ModuleFileIssue::DuplicateModuleDirective {
            first_line: 1,
            duplicate_line: 2,
        }
    );
    assert_eq!(
        parse_module_directive("module example.com/one extra\n").unwrap_err(),
        ModuleFileIssue::MalformedModuleDirective { line: 1 }
    );
    assert_eq!(
        parse_module_directive("module example.com/../escape\n").unwrap_err(),
        ModuleFileIssue::InvalidModulePath {
            line: 1,
            issue: ImportPathIssue::DotElement("..".to_owned()),
        }
    );
}

#[test]
fn opening_catalog_distinguishes_malformed_module_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("go.mod"), "go 1.24\n");

    assert!(matches!(
        LocalModuleCatalog::open(temporary.path()),
        Err(LocalModuleError::MalformedModule {
            issue: ModuleFileIssue::MissingModuleDirective,
            ..
        })
    ));
}

#[test]
fn opening_catalog_preserves_filesystem_failures_as_io_errors() {
    let temporary = tempfile::tempdir().unwrap();

    assert!(matches!(
        LocalModuleCatalog::open(temporary.path()),
        Err(LocalModuleError::Io {
            operation: "canonicalize go.mod",
            ..
        })
    ));
}

#[test]
fn maps_module_root_and_subpackages_without_recursive_discovery() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("root.go"), "package project\n");
    let subpackage = temporary.path().join("internal").join("value");
    std::fs::create_dir_all(&subpackage).unwrap();
    write(&subpackage.join("value.go"), "package value\n");
    let catalog = module(temporary.path(), "example.com/team/project");

    assert_eq!(catalog.materialized_package_count(), 0);
    let root = catalog
        .materialize(&import("example.com/team/project"))
        .unwrap();
    assert_eq!(root.import_path(), &import("example.com/team/project"));
    assert_eq!(root.files().first().unwrap().logical_path(), "root.go");
    assert_eq!(catalog.materialized_package_count(), 1);

    let child = catalog
        .materialize(&import("example.com/team/project/internal/value"))
        .unwrap();
    assert_eq!(
        child.canonical_directory(),
        std::fs::canonicalize(subpackage).unwrap()
    );
    assert_eq!(child.files().first().unwrap().source(), "package value\n");
    assert_eq!(catalog.materialized_package_count(), 2);
}

#[test]
fn distinguishes_external_and_missing_packages_at_segment_boundaries() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("root.go"), "package project\n");
    let catalog = module(temporary.path(), "example.com/team/project");

    assert!(matches!(
        catalog.materialize(&import("example.com/team/project-other")),
        Err(LocalModuleError::ExternalImport { .. })
    ));
    assert!(matches!(
        catalog.materialize(&import("example.com/team/project/missing")),
        Err(LocalModuleError::MissingPackage { .. })
    ));
    assert_eq!(catalog.materialized_package_count(), 0);
}

#[test]
fn existing_directory_without_build_sources_is_not_a_missing_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let empty_package = temporary.path().join("empty");
    std::fs::create_dir(&empty_package).unwrap();
    write(&empty_package.join("only_test.go"), "package empty\n");
    let catalog = module(temporary.path(), "example.com/project");

    assert!(matches!(
        catalog.materialize(&import("example.com/project/empty")),
        Err(LocalModuleError::PackageHasNoGoFiles { .. })
    ));
}

#[cfg(unix)]
#[test]
fn rejects_package_directory_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("outside.go"), "package outside\n");
    write(&temporary.path().join("root.go"), "package project\n");
    symlink(outside.path(), temporary.path().join("escape")).unwrap();
    let catalog = module(temporary.path(), "example.com/team/project");

    let error = catalog
        .materialize(&import("example.com/team/project/escape"))
        .unwrap_err();
    assert!(matches!(
        error,
        LocalModuleError::ContainmentEscape {
            canonical_path: Some(path),
            ..
        } if path == std::fs::canonicalize(outside.path()).unwrap()
    ));
}

#[cfg(unix)]
#[test]
fn rejects_source_file_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let package = temporary.path().join("safe");
    std::fs::create_dir(&package).unwrap();
    let outside_file = outside.path().join("outside.go");
    write(&outside_file, "package safe\n");
    symlink(&outside_file, package.join("linked.go")).unwrap();
    let catalog = module(temporary.path(), "example.com/project");

    assert!(matches!(
        catalog.materialize(&import("example.com/project/safe")),
        Err(LocalModuleError::ContainmentEscape { .. })
    ));
}

#[test]
fn reads_immediate_eligible_files_in_canonical_order() {
    let temporary = tempfile::tempdir().unwrap();
    let package = temporary.path().join("ordered");
    std::fs::create_dir(&package).unwrap();
    write(&package.join("z.go"), "package ordered\nconst Z = 1\n");
    write(&package.join("a.go"), "package ordered\nconst A = 1\n");
    write(&package.join("ignored_test.go"), "package ordered\n");
    write(&package.join(".hidden.go"), "package ordered\n");
    write(&package.join("_generated.go"), "package ordered\n");
    write(&package.join("notes.txt"), "not Go\n");
    std::fs::create_dir(package.join("nested.go")).unwrap();
    let nested = package.join("nested");
    std::fs::create_dir(&nested).unwrap();
    write(&nested.join("not_immediate.go"), "package nested\n");
    let catalog = module(temporary.path(), "example.com/project");

    let package = catalog
        .materialize(&import("example.com/project/ordered"))
        .unwrap();
    let logical_paths = package
        .files()
        .iter()
        .map(|file| file.logical_path())
        .collect::<Vec<_>>();

    assert_eq!(logical_paths, ["a.go", "z.go"]);
}

#[test]
fn memoizes_one_immutable_read_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let package_directory = temporary.path().join("cached");
    std::fs::create_dir(&package_directory).unwrap();
    let source = package_directory.join("value.go");
    write(&source, "package cached\nconst Value = 1\n");
    let catalog = module(temporary.path(), "example.com/project");
    let requested = import("example.com/project/cached");

    let first = catalog.materialize(&requested).unwrap();
    write(&source, "package cached\nconst Value = 2\n");
    write(
        &package_directory.join("later.go"),
        "package cached\nconst Later = 1\n",
    );
    let second = catalog.materialize(&requested).unwrap();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(second.files().len(), 1);
    assert_eq!(
        second.files().first().unwrap().source(),
        "package cached\nconst Value = 1\n"
    );
}

#[test]
fn leaves_unrequested_packages_completely_unread() {
    let temporary = tempfile::tempdir().unwrap();
    let requested = temporary.path().join("requested");
    let unrelated = temporary.path().join("unrelated");
    std::fs::create_dir(&requested).unwrap();
    std::fs::create_dir(&unrelated).unwrap();
    write(&requested.join("good.go"), "package requested\n");
    write(&unrelated.join("broken.go"), [0xff, 0xfe]);
    let catalog = module(temporary.path(), "example.com/project");

    let package = catalog
        .materialize(&import("example.com/project/requested"))
        .unwrap();

    assert_eq!(package.files().first().unwrap().logical_path(), "good.go");
    assert_eq!(catalog.materialized_package_count(), 1);
}

#[test]
fn concurrent_requests_share_one_materialization() {
    let temporary = tempfile::tempdir().unwrap();
    let package_directory = temporary.path().join("parallel");
    std::fs::create_dir(&package_directory).unwrap();
    write(&package_directory.join("value.go"), "package parallel\n");
    let catalog = Arc::new(module(temporary.path(), "example.com/project"));
    let requested = import("example.com/project/parallel");

    let mut threads = Vec::new();
    for _ in 0..8 {
        let catalog = Arc::clone(&catalog);
        let requested = requested.clone();
        threads.push(std::thread::spawn(move || {
            catalog.materialize(&requested).unwrap()
        }));
    }
    let mut packages = threads.into_iter().map(|thread| thread.join().unwrap());
    let first = packages.next().unwrap();

    assert!(packages.all(|package| Arc::ptr_eq(&first, &package)));
    assert_eq!(catalog.materialized_package_count(), 1);
}
