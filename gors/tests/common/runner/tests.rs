#![allow(clippy::unwrap_used)]

use super::{TestConfig, discover_program_dirs, fixtures_dir};
use std::collections::BTreeSet;
use std::fs;

fn test_config() -> TestConfig {
    TestConfig {
        limit: None,
        filter: None,
        verbose: false,
        fail_fast: false,
        include_unsupported: false,
    }
}

#[test]
fn underscore_fixture_requires_explicit_status() {
    let fixture_root = tempfile::tempdir().unwrap();
    let fixture = fixture_root.path().join("_known_failure");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("main.go"), "package main\nfunc main() {}\n").unwrap();

    let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

    assert!(error.contains("underscore-prefixed fixtures require an explicit"));
}

#[test]
fn explicit_unsupported_fixture_is_discovered_but_not_run() {
    let fixture_root = tempfile::tempdir().unwrap();
    let fixture = fixture_root.path().join("_known_failure");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("main.go"), "package main\nfunc main() {}\n").unwrap();
    fs::write(
        fixture_root.path().join("fixtures.json"),
        r#"{
  "fixtures": {
"_known_failure": {
  "status": "unsupported",
  "reason": "compiler issue recorded by the fixture owner"
}
  }
}"#,
    )
    .unwrap();

    let catalog = discover_program_dirs(fixture_root.path(), &test_config()).unwrap();

    assert!(catalog.runnable_dirs.is_empty());
    assert_eq!(
        catalog.all_program_names,
        BTreeSet::from(["_known_failure".to_string()])
    );
    assert_eq!(
        catalog.excluded_names,
        BTreeSet::from(["_known_failure".to_string()])
    );
}

#[test]
fn manifest_rejects_missing_fixture_and_empty_reason() {
    let fixture_root = tempfile::tempdir().unwrap();
    fs::write(
        fixture_root.path().join("fixtures.json"),
        r#"{
  "fixtures": {
"_missing": {
  "status": "unsupported",
  "reason": ""
}
  }
}"#,
    )
    .unwrap();

    let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

    assert!(error.contains("does not reference a directory containing main.go"));
}

#[test]
fn manifest_program_count_makes_missing_fixtures_a_hard_error() {
    let fixture_root = tempfile::tempdir().unwrap();
    fs::write(
        fixture_root.path().join("fixtures.json"),
        r#"{
  "expectedProgramCount": 1,
  "fixtures": {}
}"#,
    )
    .unwrap();

    let error = discover_program_dirs(fixture_root.path(), &test_config()).unwrap_err();

    assert!(error.contains("expected 1 program directories"));
    assert!(error.contains("discovered 0"));
}

#[test]
fn repository_fixture_manifests_classify_every_private_program() {
    for fixture_set in ["go_spec", "go_stdlib", "go_programs"] {
        discover_program_dirs(&fixtures_dir().join(fixture_set), &test_config())
            .unwrap_or_else(|error| panic!("fixtures/{fixture_set}: {error}"));
    }
}
