#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use super::*;
use crate::runtime_descriptor::test_runtime_link_descriptor;
use std::collections::BTreeMap;
use std::ffi::OsString;

const RUNTIME_PAYLOAD: &[u8] = b"test-runtime";

struct ActionFixture {
    _temporary: tempfile::TempDir,
    action: RustcAction,
}

fn action_fixture(release: bool) -> ActionFixture {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join(GENERATED_SOURCE_FILENAME);
    let module = temporary.path().join("module.rs");
    let runtime = temporary.path().join("lib__gors_runtime.rlib");
    let rustc = temporary.path().join("rustc");
    let output = temporary.path().join("main");
    std::fs::write(source, b"fn main() {}\n").unwrap();
    std::fs::write(module, b"pub fn value() -> i64 { 1 }\n").unwrap();
    std::fs::write(&runtime, RUNTIME_PAYLOAD).unwrap();
    std::fs::write(&rustc, b"test rustc").unwrap();
    let rustc_snapshot_identity = crate::runtime_link::rustc_snapshot_identity(&rustc).unwrap();
    let generated_file_hashes = generated_hashes([
        (GENERATED_SOURCE_FILENAME, b"fn main() {}\n".as_slice()),
        ("module.rs", b"pub fn value() -> i64 { 1 }\n".as_slice()),
    ]);
    let action = RustcAction::for_generated_binary(
        temporary.path(),
        &output,
        &runtime,
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(&rustc, &rustc_snapshot_identity),
        &generated_file_hashes,
        RustcProfile::from_release_flag(release),
    )
    .unwrap();
    ActionFixture {
        _temporary: temporary,
        action,
    }
}

fn generated_hashes<const N: usize>(files: [(&str, &[u8]); N]) -> BTreeMap<String, String> {
    files
        .into_iter()
        .map(|(filename, content)| (filename.to_string(), hex(Sha256::digest(content).into())))
        .collect()
}

fn action_generated_hashes(action: &RustcAction) -> BTreeMap<String, String> {
    action
        .generated_sources
        .iter()
        .map(|source| (source.filename.clone(), hex(source.content_hash)))
        .collect()
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn action_is_stable_and_uses_one_explicit_portable_runtime_link() {
    let first = action_fixture(false);
    let generated_file_hashes = action_generated_hashes(&first.action);
    let second = RustcAction::for_generated_binary(
        first.action.cwd(),
        first.action.output_path(),
        &first.action.runtime_artifact_path,
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(
            Path::new(first.action.program()),
            &first.action.rustc_snapshot_identity,
        ),
        &generated_file_hashes,
        RustcProfile::Development,
    )
    .unwrap();

    assert_eq!(first.action.identity(), second.identity());
    assert_eq!(first.action.program(), second.program());
    assert!(Path::new(first.action.program()).is_absolute());
    assert_eq!(first.action.cwd(), second.cwd());
    assert_eq!(first.action.argv(), second.argv());
    assert_eq!(first.action.identity().to_string().len(), 64);
    assert_eq!(
        first.action.pending_path(),
        first.action.cwd().join(PENDING_BINARY_FILENAME)
    );

    let arguments = first.action.argv();
    assert!(arguments.iter().all(|argument| argument != "rustup"));
    assert!(arguments.iter().all(|argument| argument != "run"));
    assert!(arguments.iter().all(|argument| argument != "rustc"));
    assert_eq!(arguments.first(), Some(&OsString::from("main.rs")));
    assert_eq!(
        arguments
            .iter()
            .filter(|argument| argument.as_os_str() == OsStr::new("--extern"))
            .count(),
        1
    );
    let mut expected_extern = OsString::from(RUST_RUNTIME_CRATE_NAME);
    expected_extern.push("=");
    expected_extern.push(first.action.runtime_artifact_path.as_os_str());
    assert_eq!(
        arguments
            .iter()
            .filter(|argument| argument.as_os_str() == expected_extern.as_os_str())
            .count(),
        1
    );
    assert!(arguments.iter().any(|argument| argument == "--target"));
    assert!(arguments.iter().any(|argument| argument == "test-target"));
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "-Ctarget-cpu=generic")
    );
    assert!(
        arguments
            .iter()
            .any(|argument| argument == "-Ctarget-feature=")
    );
    assert!(
        arguments
            .iter()
            .all(|argument| !argument.to_string_lossy().contains("native"))
    );
    assert!(arguments.windows(2).any(|pair| pair
        == [
            OsString::from("-o"),
            OsString::from(PENDING_BINARY_FILENAME)
        ]));
}

#[test]
fn action_identity_changes_with_every_owned_semantic_input() {
    let fixture = action_fixture(false);
    let action = fixture.action;
    let original = action.identity();
    let assert_changed = |mutated: &RustcAction, field: &str| {
        assert_ne!(mutated.identity(), original, "identity omitted {field}");
    };

    let mut mutated = action.clone();
    mutated.program.push("-other");
    assert_changed(&mutated, "program");

    let mut mutated = action.clone();
    mutated.rustc_snapshot_identity.push_str("-other");
    assert_changed(&mutated, "rustc snapshot identity");

    let mut mutated = action.clone();
    mutated.cwd.push("other");
    assert_changed(&mutated, "cwd");

    let mut mutated = action.clone();
    mutated.generated_sources[0].filename.push_str("-other");
    assert_changed(&mutated, "generated source filename");

    let mut mutated = action.clone();
    mutated.generated_sources[0].content_hash[0] ^= 1;
    assert_changed(&mutated, "generated source content hash");

    let mut mutated = action.clone();
    mutated.generated_sources.swap(0, 1);
    assert_changed(&mutated, "generated source order");

    let mut mutated = action.clone();
    mutated.runtime_artifact_path.push("other.rlib");
    assert_changed(&mutated, "runtime artifact path");

    let mut mutated = action.clone();
    mutated.runtime_implementation_hash[0] ^= 1;
    assert_changed(&mutated, "runtime implementation hash");

    let mut mutated = action.clone();
    mutated.runtime_link_plan_identity.push_str("-other");
    assert_changed(&mutated, "runtime link-plan identity");

    let mut mutated = action.clone();
    mutated.runtime_compatibility_identity.push_str("-other");
    assert_changed(&mutated, "runtime compatibility identity");

    let mut mutated = action.clone();
    mutated.output_path.push("other");
    assert_changed(&mutated, "published output path");

    let mut mutated = action.clone();
    mutated.pending_path.push("other");
    assert_changed(&mutated, "pending output path");

    let mut mutated = action.clone();
    mutated.profile = RustcProfile::Production;
    assert_changed(&mutated, "profile");

    let mut mutated = action.clone();
    mutated.target.push_str("-other");
    assert_changed(&mutated, "target");

    let mut mutated = action.clone();
    mutated.target_cpu.push_str("-other");
    assert_changed(&mutated, "target CPU");

    let mut mutated = action.clone();
    mutated.requested_target_features.push("+simd".to_string());
    assert_changed(&mutated, "requested target features");

    for position in 0..action.argv.len() {
        let mut mutated = action.clone();
        mutated.argv[position].push("-other");
        assert_changed(&mutated, &format!("argv[{position}]"));
    }
    let mut reordered = action;
    reordered.argv.swap(0, 1);
    assert_changed(&reordered, "argv order");
}

#[test]
fn action_identity_changes_with_source_profile_and_runtime_selection() {
    let debug = action_fixture(false);
    let original = debug.action.identity();

    std::fs::write(
        debug.action.cwd().join(GENERATED_SOURCE_FILENAME),
        b"fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let changed_hashes = generated_hashes([
        (
            GENERATED_SOURCE_FILENAME,
            b"fn main() { println!(\"changed\"); }\n".as_slice(),
        ),
        ("module.rs", b"pub fn value() -> i64 { 1 }\n".as_slice()),
    ]);
    let source_changed = RustcAction::for_generated_binary(
        debug.action.cwd(),
        debug.action.output_path(),
        &debug.action.runtime_artifact_path,
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(
            Path::new(debug.action.program()),
            &debug.action.rustc_snapshot_identity,
        ),
        &changed_hashes,
        RustcProfile::Development,
    )
    .unwrap();
    assert_ne!(source_changed.identity(), original);

    let release = RustcAction::for_generated_binary(
        debug.action.cwd(),
        debug.action.output_path(),
        &debug.action.runtime_artifact_path,
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(
            Path::new(debug.action.program()),
            &debug.action.rustc_snapshot_identity,
        ),
        &changed_hashes,
        RustcProfile::Production,
    )
    .unwrap();
    assert_ne!(release.identity(), source_changed.identity());

    let alternate_runtime = debug.action.cwd().join("alternate-runtime.rlib");
    std::fs::write(&alternate_runtime, b"alternate").unwrap();
    let runtime_changed = RustcAction::for_generated_binary(
        debug.action.cwd(),
        debug.action.output_path(),
        &alternate_runtime,
        &crate::runtime_descriptor::test_runtime_link_descriptor_with_payload(b"alternate"),
        AdmittedRustc::new(
            Path::new(debug.action.program()),
            &debug.action.rustc_snapshot_identity,
        ),
        &changed_hashes,
        RustcProfile::Development,
    )
    .unwrap();
    assert_ne!(runtime_changed.identity(), source_changed.identity());
}

#[test]
fn execution_rejects_any_generated_source_changed_after_identity_construction() {
    let fixture = action_fixture(false);
    std::fs::write(
        fixture.action.cwd().join("module.rs"),
        b"pub fn value() -> i64 { 2 }\n",
    )
    .unwrap();
    assert!(matches!(
        fixture.action.execute(),
        Err(RustcActionError::GeneratedSourceChanged { path })
            if path.ends_with("module.rs")
    ));
}

#[test]
fn execution_rejects_runtime_artifact_changed_after_provider_selection() {
    let fixture = action_fixture(false);
    std::fs::write(&fixture.action.runtime_artifact_path, b"changed-runtime").unwrap();
    assert!(matches!(
        fixture.action.execute(),
        Err(RustcActionError::RuntimeArtifactChanged { .. })
    ));
}

#[test]
fn warm_action_construction_reuses_admitted_hashes_without_input_io() {
    let fixture = action_fixture(false);
    let generated_file_hashes = action_generated_hashes(&fixture.action);
    std::fs::remove_file(fixture.action.cwd().join("module.rs")).unwrap();
    std::fs::remove_file(&fixture.action.runtime_artifact_path).unwrap();

    let reconstructed = RustcAction::for_generated_binary(
        fixture.action.cwd(),
        fixture.action.output_path(),
        &fixture.action.runtime_artifact_path,
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(
            Path::new(fixture.action.program()),
            &fixture.action.rustc_snapshot_identity,
        ),
        &generated_file_hashes,
        RustcProfile::Development,
    )
    .unwrap();

    assert_eq!(reconstructed.identity(), fixture.action.identity());
    assert!(matches!(
        reconstructed.execute(),
        Err(RustcActionError::GeneratedSourceInspection { path, .. })
            if path.ends_with("module.rs")
    ));
}

#[test]
fn warm_action_construction_does_not_inspect_historical_rustc() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(
        temporary.path().join(GENERATED_SOURCE_FILENAME),
        "fn main() {}\n",
    )
    .unwrap();
    std::fs::write(temporary.path().join("runtime.rlib"), RUNTIME_PAYLOAD).unwrap();
    let generated_file_hashes =
        generated_hashes([(GENERATED_SOURCE_FILENAME, b"fn main() {}\n".as_slice())]);
    let missing_rustc = temporary.path().join("removed-rustc");
    let action = RustcAction::for_generated_binary(
        temporary.path(),
        &temporary.path().join("main"),
        &temporary.path().join("runtime.rlib"),
        &test_runtime_link_descriptor(),
        AdmittedRustc::new(&missing_rustc, &"0".repeat(64)),
        &generated_file_hashes,
        RustcProfile::Development,
    )
    .unwrap();

    assert_eq!(Path::new(action.program()), missing_rustc);
    assert!(matches!(
        action.execute(),
        Err(RustcActionError::RustcSnapshotInspection { .. })
    ));
}

#[test]
fn execution_rejects_rustc_changed_after_compatibility_selection() {
    let fixture = action_fixture(false);
    std::fs::write(
        Path::new(fixture.action.program()),
        b"changed test rustc payload",
    )
    .unwrap();
    assert!(matches!(
        fixture.action.execute(),
        Err(RustcActionError::RustcSnapshotChanged { .. })
    ));
}

#[cfg(unix)]
#[test]
fn execution_uses_the_owned_command_and_stable_pending_output() {
    let fixture = action_fixture(false);
    let mut action = fixture.action;
    action.program = OsString::from("/bin/sh");
    action.rustc_snapshot_identity =
        crate::runtime_link::rustc_snapshot_identity(Path::new("/bin/sh")).unwrap();
    action.argv = vec![
        OsString::from("-c"),
        OsString::from("printf stable > .main.pending"),
    ];
    let executable = action.execute().unwrap();

    assert_eq!(std::fs::read(action.output_path()).unwrap(), b"stable");
    assert!(!action.pending_path().exists());
    assert_eq!(executable.path(), action.output_path());
    assert_eq!(executable.size_bytes(), 6);
}

#[test]
fn failed_internal_publication_preserves_the_admitted_executable() {
    let temporary = tempfile::tempdir().unwrap();
    let pending = temporary.path().join(PENDING_BINARY_FILENAME);
    let output = temporary.path().join("main-development");
    std::fs::write(&output, b"admitted executable").unwrap();

    let error = publish_pending_executable(&pending, &output).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(std::fs::read(&output).unwrap(), b"admitted executable");
}
