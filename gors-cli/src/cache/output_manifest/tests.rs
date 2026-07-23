#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

fn manifest() -> GeneratedOutputManifest {
    GeneratedOutputManifest::new(&crate::runtime_descriptor::test_runtime_dependency())
}

#[test]
fn records_only_exact_generated_file_content() {
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));

    assert!(manifest.matches("main.rs", &"a".repeat(64)));
    assert!(!manifest.matches("main.rs", "changed"));
    assert!(!manifest.matches("missing.rs", &"a".repeat(64)));
}

#[test]
fn round_trip_is_current_for_compiler_and_stdlib_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    manifest.save(directory.path()).unwrap();

    let loaded = GeneratedOutputManifest::load(directory.path()).expect("current manifest");
    assert_eq!(loaded.len(), 1);
    assert!(loaded.matches("main.rs", &"a".repeat(64)));
    assert_eq!(
        loaded.runtime_dependency().unwrap(),
        crate::runtime_descriptor::test_runtime_dependency()
    );
}

#[test]
fn save_replaces_an_existing_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    manifest.save(directory.path()).unwrap();
    manifest.record("module.rs".to_string(), "b".repeat(64));
    manifest.save(directory.path()).unwrap();

    let loaded = GeneratedOutputManifest::load(directory.path()).expect("replacement manifest");
    assert_eq!(loaded.len(), 2);
    assert!(loaded.matches("module.rs", &"b".repeat(64)));
}

#[test]
fn target_neutral_manifest_does_not_require_a_terminal_link_descriptor() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    manifest.save(directory.path()).unwrap();

    assert!(GeneratedOutputManifest::load(directory.path()).is_some());
}

#[test]
fn rejects_an_incompatible_schema() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    manifest.schema_version += 1;
    manifest.save(directory.path()).unwrap();

    assert!(GeneratedOutputManifest::load(directory.path()).is_none());
}

#[test]
fn rejects_noncanonical_or_escaping_output_paths() {
    for output_file in ["../victim", "/tmp/victim", "nested/victim", r"..\victim"] {
        let directory = tempfile::tempdir().unwrap();
        let mut manifest = manifest();
        manifest.record("main.rs".to_string(), "a".repeat(64));
        manifest.files.insert(
            "stale.rs".to_string(),
            GeneratedFileEntry {
                content_hash: "a".repeat(64),
                output_file: output_file.to_string(),
            },
        );
        manifest.save(directory.path()).unwrap();

        assert!(
            GeneratedOutputManifest::load(directory.path()).is_none(),
            "accepted unsafe output path {output_file:?}"
        );
    }
}

#[test]
fn rejects_unknown_manifest_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    let mut value = serde_json::to_value(&manifest).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("future_field".to_string(), serde_json::json!(true));
    std::fs::write(
        directory.path().join(FILENAME),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();

    assert!(GeneratedOutputManifest::load(directory.path()).is_none());
}

#[test]
fn rejects_terminal_files_in_the_generated_rust_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let mut manifest = manifest();
    manifest.record("main.rs".to_string(), "a".repeat(64));
    manifest.record(
        crate::runtime_descriptor::LINK_OUTPUT_FILENAME.to_string(),
        "b".repeat(64),
    );
    manifest.save(directory.path()).unwrap();

    assert!(GeneratedOutputManifest::load(directory.path()).is_none());
}
