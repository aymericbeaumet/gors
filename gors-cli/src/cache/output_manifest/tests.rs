#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

#[test]
fn records_only_exact_generated_file_content() {
    let mut manifest = GeneratedOutputManifest::new();
    manifest.record("main.rs".to_string(), "abc123".to_string());

    assert!(manifest.matches("main.rs", "abc123"));
    assert!(!manifest.matches("main.rs", "changed"));
    assert!(!manifest.matches("missing.rs", "abc123"));
}

#[test]
fn round_trip_is_current_for_compiler_and_stdlib_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = GeneratedOutputManifest::new();
    manifest.record("main.rs".to_string(), "abc123".to_string());
    manifest.save(directory.path()).unwrap();

    let loaded = GeneratedOutputManifest::load(directory.path()).expect("current manifest");
    assert_eq!(loaded.len(), 1);
    assert!(loaded.matches("main.rs", "abc123"));
}

#[test]
fn rejects_an_incompatible_schema() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = GeneratedOutputManifest::new();
    manifest.schema_version += 1;
    manifest.save(directory.path()).unwrap();

    assert!(GeneratedOutputManifest::load(directory.path()).is_none());
}
