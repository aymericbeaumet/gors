#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use crate::compiler::input::{GoLanguageVersion, PackageKey, WorkspaceKey};

use super::{
    LoadError, PathExpectation, load_program, load_program_files, load_program_files_auto,
};

fn test_workspace() -> WorkspaceKey {
    WorkspaceKey::ad_hoc("workspace-loader-tests").unwrap()
}

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).unwrap();
}

fn logical_paths(loaded: &super::LoadedProgram) -> Vec<&str> {
    loaded
        .input()
        .entry_package()
        .files()
        .iter()
        .map(crate::compiler::input::SourceFileInput::logical_path)
        .collect()
}

#[test]
fn directory_load_is_filtered_ordered_and_watched() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path();
    write(&directory.join("z.go"), "package main\n");
    write(&directory.join("a.go"), "package main\n");
    write(&directory.join("ignored_test.go"), "package main\n");
    write(&directory.join(".hidden.go"), "package main\n");
    write(&directory.join("_generated.go"), "package main\n");
    write(&directory.join("notes.txt"), "not Go\n");
    std::fs::create_dir(directory.join("nested.go")).unwrap();

    let workspace = test_workspace();
    let loaded = load_program(workspace.clone(), directory).unwrap();
    let canonical_directory = std::fs::canonicalize(directory).unwrap();
    let canonical_a = std::fs::canonicalize(directory.join("a.go")).unwrap();

    assert_eq!(logical_paths(&loaded), ["a.go", "z.go"]);
    assert_eq!(loaded.watched_directories(), [canonical_directory]);
    assert_eq!(
        loaded.primary_diagnostic_path(),
        canonical_a.to_str().unwrap()
    );
    assert_eq!(loaded.input().workspace(), &workspace);
    assert!(loaded.input().entry_package().key().is_command_line());
}

#[test]
fn module_directory_uses_canonical_entry_and_lazy_dependency_catalog() {
    let temporary = tempfile::tempdir().unwrap();
    write(
        &temporary.path().join("go.mod"),
        "module example.com/project\n",
    );
    write(&temporary.path().join("main.go"), "package main\n");
    std::fs::create_dir(temporary.path().join("dep")).unwrap();
    write(
        &temporary.path().join("dep/dep.go"),
        "package dep\nconst Value = 1\n",
    );

    let loaded =
        load_program_files_auto(test_workspace(), &[temporary.path().to_path_buf()]).unwrap();

    assert_eq!(
        loaded.input().workspace(),
        &WorkspaceKey::module("example.com/project").unwrap()
    );
    assert_eq!(
        loaded.input().entry_package().language_version(),
        GoLanguageVersion::DEFAULT_MODULE
    );
    assert_eq!(
        loaded.input().entry_package().key().as_import_path(),
        Some("example.com/project")
    );
    let dependency = PackageKey::import_path("example.com/project/dep").unwrap();
    let manifest = loaded
        .input()
        .package_catalog()
        .materialize(&dependency)
        .unwrap()
        .unwrap();
    assert_eq!(manifest.key(), &dependency);
    assert_eq!(manifest.files().len(), 1);

    let standard_library = PackageKey::import_path("fmt").unwrap();
    let manifest = loaded
        .input()
        .package_catalog()
        .materialize(&standard_library)
        .unwrap()
        .unwrap();
    assert_eq!(manifest.key(), &standard_library);
    assert!(!manifest.files().is_empty());
    assert!(
        manifest
            .files()
            .first()
            .unwrap()
            .snapshot()
            .diagnostic_path()
            .starts_with("gors://go-sdk/fmt/")
    );
}

#[test]
fn module_go_directive_gates_binary_literals_at_the_literal_span() {
    let temporary = tempfile::tempdir().unwrap();
    write(
        &temporary.path().join("go.mod"),
        "module example.com/language\n\ngo 1.12\n",
    );
    write(
        &temporary.path().join("main.go"),
        "package main\n\nfunc main() {\n\tx := 0b1011\n\tprintln(x)\n}\n",
    );

    let loaded =
        load_program_files_auto(test_workspace(), &[temporary.path().to_path_buf()]).unwrap();
    assert_eq!(
        loaded.input().entry_package().language_version(),
        GoLanguageVersion::new(1, 12)
    );
    let error = crate::compiler::compile_program(loaded.into_input())
        .err()
        .expect("Go 1.12 must reject binary literals");
    let diagnostic = error.diagnostics().first().unwrap();
    assert_eq!(diagnostic.code, "GORS2002");
    assert_eq!((diagnostic.line, diagnostic.column), (4, 7));
    assert_eq!(
        diagnostic.message,
        "binary literal requires go1.13 or later (-lang was set to go1.12; check go.mod)"
    );

    write(
        &temporary.path().join("go.mod"),
        "module example.com/language\n\ngo 1.13\n",
    );
    let loaded =
        load_program_files_auto(test_workspace(), &[temporary.path().to_path_buf()]).unwrap();
    crate::compiler::compile_program(loaded.into_input())
        .expect("the same binary literal is valid under Go 1.13");
}

#[test]
fn ad_hoc_auto_load_keeps_its_workspace_and_resolves_the_embedded_sdk() {
    let temporary = tempfile::tempdir().unwrap();
    let main = temporary.path().join("main.go");
    write(&main, "package main\nimport \"fmt\"\n");
    let workspace = test_workspace();

    let loaded = load_program_files_auto(workspace.clone(), &[main]).unwrap();

    assert_eq!(loaded.input().workspace(), &workspace);
    assert!(loaded.input().entry_package().key().is_command_line());
    let standard_library = PackageKey::import_path("fmt").unwrap();
    assert!(
        loaded
            .input()
            .package_catalog()
            .materialize(&standard_library)
            .unwrap()
            .is_some()
    );
}

#[test]
fn ad_hoc_auto_load_compiles_a_reachable_embedded_sdk_package() {
    let temporary = tempfile::tempdir().unwrap();
    let main = temporary.path().join("main.go");
    write(
        &main,
        "package main\nimport _ \"structs\"\nfunc main() { println(1) }\n",
    );

    let loaded = load_program_files_auto(test_workspace(), &[main]).unwrap();
    let compiled = crate::compiler::compile_program(loaded.into_input()).unwrap();

    assert!(!compiled.entry.items.is_empty());
    assert_eq!(
        compiled
            .modules
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["structs"]
    );
}

#[test]
fn explicit_module_file_remains_command_line_but_resolves_local_imports() {
    let temporary = tempfile::tempdir().unwrap();
    write(
        &temporary.path().join("go.mod"),
        "module example.com/project\n",
    );
    let main = temporary.path().join("main.go");
    write(&main, "package main\n");
    std::fs::create_dir(temporary.path().join("dep")).unwrap();
    write(&temporary.path().join("dep/dep.go"), "package dep\n");

    let loaded = load_program_files_auto(test_workspace(), &[main]).unwrap();

    assert!(loaded.input().entry_package().key().is_command_line());
    assert_eq!(
        loaded.input().workspace(),
        &WorkspaceKey::module("example.com/project").unwrap()
    );
    let dependency = PackageKey::import_path("example.com/project/dep").unwrap();
    assert!(
        loaded
            .input()
            .package_catalog()
            .materialize(&dependency)
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_single_explicit_file_has_no_directory_membership_watch() {
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("main.go");
    write(&file, "package main\n");

    let loaded = load_program(test_workspace(), &file).unwrap();
    let canonical_file = std::fs::canonicalize(file).unwrap();

    assert_eq!(logical_paths(&loaded), ["main.go"]);
    assert!(loaded.watched_directories().is_empty());
    assert_eq!(
        loaded.primary_diagnostic_path(),
        canonical_file.to_str().unwrap()
    );
}

#[test]
fn explicit_files_are_canonicalized_and_order_independent() {
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("a.go");
    let second = temporary.path().join("b.go");
    write(&first, "package main\n");
    write(&second, "package main\n");

    let reverse = load_program_files(test_workspace(), &[second.clone(), first.clone()]).unwrap();
    let forward = load_program_files(test_workspace(), &[first, second]).unwrap();

    assert_eq!(logical_paths(&reverse), ["a.go", "b.go"]);
    assert_eq!(logical_paths(&reverse), logical_paths(&forward));
    assert_eq!(
        reverse.primary_diagnostic_path(),
        forward.primary_diagnostic_path()
    );
    assert!(reverse.watched_directories().is_empty());
}

#[test]
fn a_single_directory_argument_keeps_directory_semantics() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("main.go"), "package main\n");

    let loaded = load_program_files(test_workspace(), &[temporary.path().to_path_buf()]).unwrap();

    assert_eq!(logical_paths(&loaded), ["main.go"]);
    assert_eq!(
        loaded.watched_directories(),
        [std::fs::canonicalize(temporary.path()).unwrap()]
    );
}

#[test]
fn syntax_invalid_source_is_a_successful_raw_load() {
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("main.go");
    write(&file, "package main\nfunc {");

    let loaded = load_program(test_workspace(), &file).unwrap();
    let source = loaded
        .input()
        .entry_package()
        .files()
        .first()
        .unwrap()
        .snapshot();

    assert_eq!(source.source(), "package main\nfunc {");
    assert!(crate::parser::parse_file(source.diagnostic_path(), source.source()).is_err());
}

#[test]
fn rejects_empty_and_duplicate_explicit_inputs() {
    let empty: [PathBuf; 0] = [];
    assert!(matches!(
        load_program_files(test_workspace(), &empty),
        Err(LoadError::NoInputPaths)
    ));

    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("main.go");
    write(&file, "package main\n");
    let canonical = std::fs::canonicalize(&file).unwrap();
    let error = load_program_files(test_workspace(), &[file.clone(), file]).unwrap_err();
    assert!(matches!(
        error,
        LoadError::DuplicateSourceFile { path } if path == canonical
    ));
}

#[test]
fn rejects_explicit_files_from_different_directories() {
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first = first_directory.path().join("first.go");
    let second = second_directory.path().join("second.go");
    write(&first, "package main\n");
    write(&second, "package main\n");

    let error = load_program_files(test_workspace(), &[first, second]).unwrap_err();
    assert!(matches!(
        error,
        LoadError::SourceFilesFromDifferentDirectories { .. }
    ));
}

#[test]
fn rejects_ineligible_explicit_sources_and_multiple_directories() {
    let temporary = tempfile::tempdir().unwrap();
    for filename in ["notes.txt", "ignored_test.go", ".hidden.go", "_private.go"] {
        let path = temporary.path().join(filename);
        write(&path, "package main\n");
        assert!(matches!(
            load_program(test_workspace(), &path),
            Err(LoadError::InvalidPathKind {
                expected: PathExpectation::EligibleGoSource,
                ..
            })
        ));
    }

    let other = tempfile::tempdir().unwrap();
    write(&other.path().join("main.go"), "package main\n");
    assert!(matches!(
        load_program_files(
            test_workspace(),
            &[temporary.path().to_path_buf(), other.path().to_path_buf(),],
        ),
        Err(LoadError::InvalidPathKind {
            expected: PathExpectation::SourceFile,
            ..
        })
    ));
}

#[test]
fn reports_directory_without_eligible_sources() {
    let temporary = tempfile::tempdir().unwrap();
    write(&temporary.path().join("only_test.go"), "package main\n");
    write(&temporary.path().join("notes.txt"), "not Go\n");
    let canonical = std::fs::canonicalize(temporary.path()).unwrap();

    let error = load_program(test_workspace(), temporary.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadError::NoGoFiles { directory } if directory == canonical
    ));
}

#[test]
fn rejects_non_utf8_source_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("main.go");
    std::fs::write(&file, [0xff, 0xfe]).unwrap();
    let canonical = std::fs::canonicalize(&file).unwrap();

    let error = load_program(test_workspace(), &file).unwrap_err();
    assert!(matches!(
        error,
        LoadError::NonUtf8Source { path } if path == canonical
    ));
}

#[cfg(unix)]
#[test]
fn rejects_non_utf8_go_filenames() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let filename = OsString::from_vec(vec![b'x', 0xff, b'.', b'g', b'o']);
    let file = PathBuf::from(filename);

    assert!(matches!(
        load_program(test_workspace(), file),
        Err(LoadError::NonUtf8Path { .. })
    ));
}

#[test]
fn entry_identity_does_not_depend_on_source_package_clause() {
    let temporary = tempfile::tempdir().unwrap();
    let file = temporary.path().join("main.go");
    write(&file, "package deliberately_different\n");

    let loaded = load_program(test_workspace(), &file).unwrap();

    assert_eq!(
        loaded.input().entry_package().key(),
        &PackageKey::CommandLine
    );
}
