use std::sync::Arc;

use super::{
    ParsedFile, PathParseError, module_relative_path, parse_path, parse_program,
    parse_program_files, parse_program_from_source,
};
use crate::parser::{ImportPathIssue, SourceSnapshot};

#[test]
fn source_snapshot_creation_does_not_hide_parse_errors() {
    let snapshot = SourceSnapshot::from_source("broken.go", "package");

    assert_eq!(snapshot.path(), "broken.go");
    assert!(snapshot.parse().is_err());
}

#[test]
fn local_import_matching_requires_a_module_path_boundary() {
    assert_eq!(
        module_relative_path("example.test/root/dep", "example.test/root"),
        Some("dep")
    );
    assert_eq!(
        module_relative_path("example.test/rooted/dep", "example.test/root"),
        None
    );
}

#[test]
fn parsed_file_owns_and_releases_its_source_revision() {
    let file = ParsedFile::from_source(
        "memory.go".to_string(),
        "package memory\nconst Value = 1\n".to_string(),
    )
    .unwrap();
    let shared_file = file.clone();
    let snapshot = file.snapshot();
    let weak_snapshot = Arc::downgrade(&snapshot);

    assert_eq!(snapshot.path(), "memory.go");
    assert_eq!(snapshot.line_start(1), Some(0));
    assert_eq!(snapshot.line_start(2), Some(15));
    assert_eq!(snapshot.line_count(), 3);
    assert!(snapshot.retained_bytes() >= snapshot.source().len());
    assert!(Arc::ptr_eq(&snapshot, &shared_file.snapshot()));
    assert_eq!(file.parse().unwrap().name.name, "memory");

    drop(snapshot);
    drop(file);
    assert!(weak_snapshot.upgrade().is_some());
    drop(shared_file);
    assert!(weak_snapshot.upgrade().is_none());
}

#[test]
fn directory_files_remain_independent_ast_products() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("alpha.go"),
        "package sample\nconst Alpha = 1\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("beta.go"),
        "package sample\nconst Beta = 2\n",
    )
    .unwrap();

    let package = parse_path(&directory.path().to_string_lossy()).unwrap();

    assert_eq!(package.name(), "sample");
    assert_eq!(package.files().len(), 2);
    let declaration_counts = package
        .files()
        .iter()
        .map(|file| file.parse().unwrap().decls.len())
        .collect::<Vec<_>>();
    assert_eq!(declaration_counts, vec![1, 1]);
    assert!(
        package
            .files()
            .first()
            .is_some_and(|file| file.path().ends_with("alpha.go"))
    );
    assert!(
        package
            .files()
            .get(1)
            .is_some_and(|file| file.path().ends_with("beta.go"))
    );
}

#[test]
fn package_mismatch_reports_the_independent_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("first.go"), "package first\n").unwrap();
    std::fs::write(directory.path().join("second.go"), "package second\n").unwrap();

    let error = parse_path(&directory.path().to_string_lossy()).unwrap_err();

    assert!(matches!(
        error,
        PathParseError::PackageMismatch {
            expected,
            found,
            file,
        } if expected == "first" && found == "second" && file.ends_with("second.go")
    ));
}

#[test]
fn imports_are_collected_from_every_package_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("dep")).unwrap();
    std::fs::write(
        directory.path().join("dep/dep.go"),
        "package dep\nconst Value = 1\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("main.go"),
        "package main\nfunc main() {}\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("imports.go"),
        "package main\nimport \"example.test/root/dep\"\n",
    )
    .unwrap();

    let program = parse_program(&directory.path().to_string_lossy()).unwrap();

    assert_eq!(program.main_package().files().len(), 2);
    assert_eq!(program.imports().len(), 1);
    let imported = program.imports().first().unwrap();
    assert_eq!(imported.name(), "dep");
    assert_eq!(imported.import_path(), "example.test/root/dep");
}

#[test]
fn entry_package_uses_its_canonical_module_root_identity() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("main.go"), "package main\n").unwrap();

    let program = parse_program(&directory.path().to_string_lossy()).unwrap();

    assert_eq!(program.main_package().import_path(), "example.test/root");
}

#[test]
fn entry_package_uses_its_canonical_nested_module_identity() {
    let directory = tempfile::tempdir().unwrap();
    let command = directory.path().join("cmd/tool");
    std::fs::create_dir_all(&command).unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::write(command.join("main.go"), "package main\n").unwrap();

    let program = parse_program(&command.to_string_lossy()).unwrap();

    assert_eq!(
        program.main_package().import_path(),
        "example.test/root/cmd/tool"
    );
}

#[test]
fn entry_package_self_import_is_a_structured_cycle() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("main.go"),
        "package main\nimport \"example.test/root\"\n",
    )
    .unwrap();

    let error = parse_program(&directory.path().to_string_lossy()).unwrap_err();

    assert!(matches!(
        error,
        PathParseError::ImportCycle { cycle }
            if cycle == ["example.test/root", "example.test/root"]
    ));
}

#[test]
fn recursive_local_import_cycle_reports_the_minimal_active_cycle() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("main.go"),
        "package main\nimport \"example.test/root/a\"\n",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("a")).unwrap();
    std::fs::write(
        directory.path().join("a/a.go"),
        "package a\nimport \"example.test/root/b\"\n",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("b")).unwrap();
    std::fs::write(
        directory.path().join("b/b.go"),
        "package b\nimport \"example.test/root/a\"\n",
    )
    .unwrap();

    let error = parse_program(&directory.path().to_string_lossy()).unwrap_err();

    assert!(matches!(
        error,
        PathParseError::ImportCycle { cycle }
            if cycle == [
                "example.test/root/a",
                "example.test/root/b",
                "example.test/root/a"
            ]
    ));
}

#[test]
fn import_literals_use_go_string_decoding() {
    let source = r#"package sample
import (
    `fmt`
    "encod\x69ng/json"
    "example\056test/\u0064\U00000065\160"
)
"#;

    let file = ParsedFile::from_source("imports.go", source).unwrap();

    assert_eq!(file.imports(), ["fmt", "encoding/json", "example.test/dep"]);
}

#[test]
fn raw_import_literals_discard_carriage_returns() {
    let source = "package sample\nimport `example.test/root/\rdep`\n";

    let file = ParsedFile::from_source("raw.go", source).unwrap();

    assert_eq!(file.imports(), ["example.test/root/dep"]);
}

#[test]
fn escaped_local_import_resolves_the_decoded_path() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("dep")).unwrap();
    std::fs::write(
        directory.path().join("dep/dep.go"),
        "package dep\nconst Value = 1\n",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("main.go"),
        "package main\nimport \"example.test/root/\\x64ep\"\nfunc main() {}\n",
    )
    .unwrap();

    let program = parse_program(&directory.path().to_string_lossy()).unwrap();

    assert_eq!(program.imports().len(), 1);
    assert_eq!(
        program.imports().first().unwrap().import_path(),
        "example.test/root/dep"
    );
}

#[test]
fn invalid_decoded_import_paths_are_structured_and_file_scoped() -> Result<(), String> {
    let cases = [
        (
            r#""example.test/root/\x2e\x2e/outside""#,
            "dot path element",
        ),
        (r#""example.test\\outside""#, "backslash"),
        (r#""/absolute/package""#, "absolute"),
        (r#""example.test//empty""#, "empty element"),
    ];

    for (literal, expected_reason) in cases {
        let source = format!("package sample\nimport {literal}\n");
        let error = parse_program_from_source("invalid.go", &source).unwrap_err();
        let error = match error {
            PathParseError::InvalidImportPath(error) => error,
            unexpected => {
                return Err(format!(
                    "expected invalid import path for {literal}, got {unexpected}"
                ));
            }
        };
        assert_eq!(error.path(), "invalid.go");
        assert_eq!(error.snapshot().source(), source);
        assert_eq!(error.line_column(), (2, 8));
        assert!(
            error.issue().to_string().contains(expected_reason),
            "unexpected issue: {}",
            error.issue()
        );
    }
    Ok(())
}

#[test]
fn decoded_import_paths_reject_non_utf8_and_nul() -> Result<(), String> {
    let cases = [
        (r#""\xff""#, ImportPathIssue::InvalidUtf8),
        (r#""bad\000path""#, ImportPathIssue::ContainsNul),
    ];

    for (literal, expected_issue) in cases {
        let source = format!("package sample\nimport {literal}\n");
        let error = parse_program_from_source("invalid.go", &source).unwrap_err();
        let error = match error {
            PathParseError::InvalidImportPath(error) => error,
            unexpected => {
                return Err(format!(
                    "expected invalid import path for {literal}, got {unexpected}"
                ));
            }
        };
        assert_eq!(error.issue(), &expected_issue);
    }
    Ok(())
}

#[test]
fn explicit_file_order_is_normalized_before_package_and_module_discovery() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("go.mod"),
        "module example.test/root\n",
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("dep")).unwrap();
    std::fs::write(directory.path().join("dep/dep.go"), "package dep\n").unwrap();
    let alpha = directory.path().join("alpha.go");
    let beta = directory.path().join("beta.go");
    std::fs::write(&alpha, "package main\nfunc main() {}\n").unwrap();
    std::fs::write(&beta, "package main\nimport \"example.test/root/dep\"\n").unwrap();
    let forward = vec![
        alpha.to_string_lossy().into_owned(),
        beta.to_string_lossy().into_owned(),
    ];
    let reverse = forward.iter().rev().cloned().collect::<Vec<_>>();

    let first = parse_program_files(&forward).unwrap();
    let second = parse_program_files(&reverse).unwrap();
    let first_files = first
        .main_package()
        .files()
        .iter()
        .map(|file| file.path())
        .collect::<Vec<_>>();
    let second_files = second
        .main_package()
        .files()
        .iter()
        .map(|file| file.path())
        .collect::<Vec<_>>();

    assert_eq!(first_files, second_files);
    assert_eq!(first_files.len(), 2);
    assert!(first_files.first().unwrap().ends_with("alpha.go"));
    assert!(first_files.get(1).unwrap().ends_with("beta.go"));
    assert_eq!(
        first.imports().first().unwrap().import_path(),
        "example.test/root/dep"
    );
    assert_eq!(
        second.imports().first().unwrap().import_path(),
        "example.test/root/dep"
    );
}

#[test]
fn explicit_files_reject_duplicate_physical_paths() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.go");
    std::fs::write(&file, "package main\n").unwrap();
    let path = file.to_string_lossy().into_owned();

    let error = parse_program_files(&[path.clone(), path]).unwrap_err();

    assert!(matches!(error, PathParseError::DuplicateSourceFile { .. }));
}

#[test]
fn explicit_files_reject_cross_directory_packages() {
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first = first_directory.path().join("first.go");
    let second = second_directory.path().join("second.go");
    std::fs::write(&first, "package sample\n").unwrap();
    std::fs::write(&second, "package sample\n").unwrap();

    let error = parse_program_files(&[
        first.to_string_lossy().into_owned(),
        second.to_string_lossy().into_owned(),
    ])
    .unwrap_err();

    assert!(matches!(
        error,
        PathParseError::SourceFilesFromDifferentDirectories { .. }
    ));
}

#[test]
fn high_level_parse_errors_retain_the_exact_failing_file_revision() -> Result<(), String> {
    let directory = tempfile::tempdir().unwrap();
    let valid = directory.path().join("alpha.go");
    let broken = directory.path().join("broken.go");
    let broken_source = "package main\nfunc broken(";
    std::fs::write(&valid, "package main\nfunc main() {}\n").unwrap();
    std::fs::write(&broken, broken_source).unwrap();

    let error = parse_program_files(&[
        broken.to_string_lossy().into_owned(),
        valid.to_string_lossy().into_owned(),
    ])
    .unwrap_err();
    let error = match error {
        PathParseError::ParserError(error) => error,
        unexpected => return Err(format!("expected a parser error, got {unexpected}")),
    };

    assert!(error.path().ends_with("broken.go"));
    assert_eq!(error.source_text(), broken_source);
    assert!(error.line_column().is_some());
    assert!(Arc::ptr_eq(&error.snapshot(), &error.snapshot()));
    Ok(())
}

#[cfg(unix)]
#[test]
fn local_import_symlinks_may_not_escape_the_module_root() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().unwrap();
    let module = workspace.path().join("module");
    let outside = workspace.path().join("outside");
    std::fs::create_dir(&module).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(module.join("go.mod"), "module example.test/root\n").unwrap();
    std::fs::write(
        module.join("main.go"),
        "package main\nimport \"example.test/root/escape\"\n",
    )
    .unwrap();
    std::fs::write(outside.join("outside.go"), "package outside\n").unwrap();
    symlink(&outside, module.join("escape")).unwrap();

    let error = parse_program(&module.to_string_lossy()).unwrap_err();

    assert!(matches!(
        error,
        PathParseError::LocalImportOutsideModule { import_path, .. }
            if import_path == "example.test/root/escape"
    ));
}
