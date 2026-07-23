#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::sync::mpsc;

fn identity() -> GeneratedRustIdentity {
    GeneratedRustIdentity::for_test("generated")
}

fn rustc_selection() -> (PathBuf, String) {
    let path = std::env::current_exe().unwrap();
    let identity = crate::runtime_link::rustc_snapshot_identity(&path).unwrap();
    (path, identity)
}

fn write_expired_manifest(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    let old_timestamp = unix_time_ms()
        .saturating_sub(CACHE_MAX_AGE.as_millis().try_into().unwrap())
        .saturating_sub(1);
    CliCacheManifest::new(
        &identity(),
        InputSnapshot {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        },
        BTreeMap::new(),
        None,
        &crate::runtime_descriptor::test_runtime_dependency(),
    )
    .with_last_used(old_timestamp)
    .save(path)
    .unwrap();
}

#[test]
fn input_snapshot_is_the_immutable_loaded_revision() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.go");
    std::fs::write(&source, "package main\n").unwrap();
    let loaded =
        gors::workspace::load_program(crate::cli_workspace().unwrap(), temp.path()).unwrap();

    std::fs::write(&source, "package changed\n").unwrap();
    std::fs::write(temp.path().join("added.go"), "package main\n").unwrap();
    let snapshot = InputSnapshot::capture(&loaded).unwrap();

    assert_eq!(
        snapshot.files.get(&normalized_path(&source).unwrap()),
        Some(&sha2_hash(b"package main\n"))
    );

    let reloaded =
        gors::workspace::load_program(crate::cli_workspace().unwrap(), temp.path()).unwrap();
    let current = InputSnapshot::capture(&reloaded).unwrap();
    assert_ne!(snapshot, current);
}

#[test]
fn cache_admission_compares_supplied_snapshot_without_rereading_sources() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("source");
    let output_directory = temp.path().join("output");
    std::fs::create_dir_all(&source_directory).unwrap();
    std::fs::create_dir_all(&output_directory).unwrap();
    let source = source_directory.join("main.go");
    std::fs::write(&source, "package main\n").unwrap();

    let loaded =
        gors::workspace::load_program(crate::cli_workspace().unwrap(), &source_directory).unwrap();
    let captured = InputSnapshot::capture(&loaded).unwrap();
    let generated_source = "fn main() {}\n";
    std::fs::write(output_directory.join("main.rs"), generated_source).unwrap();
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    let runtime_dependency = crate::runtime_descriptor::test_runtime_dependency();
    let mut output_manifest = GeneratedOutputManifest::new(&runtime_dependency);
    output_manifest.record(
        "main.rs".to_string(),
        generated_files.get("main.rs").unwrap().clone(),
    );
    output_manifest.save(&output_directory).unwrap();
    CliCacheManifest::new(
        &identity(),
        captured.clone(),
        generated_files,
        None,
        &runtime_dependency,
    )
    .save(&output_directory)
    .unwrap();

    std::fs::write(&source, "package changed\n").unwrap();
    assert!(
        CliCacheManifest::load_if_generated_valid(&output_directory, &identity(), &captured)
            .is_some(),
        "cache admission reread source bytes instead of trusting the supplied revision"
    );

    let reloaded =
        gors::workspace::load_program(crate::cli_workspace().unwrap(), &source_directory).unwrap();
    let current = InputSnapshot::capture(&reloaded).unwrap();
    assert_ne!(captured, current);
    assert!(
        CliCacheManifest::load_if_generated_valid(&output_directory, &identity(), &current)
            .is_none()
    );
}

#[test]
fn normalization_is_stable_before_and_after_output_creation() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("missing").join("output");
    let before = normalized_path(&output).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    let after = normalized_path(&output).unwrap();
    assert_eq!(before, after);
}

#[test]
fn generated_validation_rejects_deleted_modified_and_stale_rust_files() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
    let expected = std::iter::once((
        "main.rs".to_string(),
        file_hash(&temp.path().join("main.rs")).unwrap(),
    ))
    .collect();
    assert!(generated_files_are_current(temp.path(), &expected));

    std::fs::write(temp.path().join("main.rs"), "fn changed() {}\n").unwrap();
    assert!(!generated_files_are_current(temp.path(), &expected));
    std::fs::write(temp.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(temp.path().join("stale.rs"), "fn stale() {}\n").unwrap();
    assert!(!generated_files_are_current(temp.path(), &expected));
    std::fs::remove_file(temp.path().join("main.rs")).unwrap();
    assert!(!generated_files_are_current(temp.path(), &expected));
}

#[test]
fn cache_manifest_round_trips_and_validates_executable_content() {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("main");
    let generated_source = "fn main() {}\n";
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    std::fs::write(&executable, "binary").unwrap();
    let mut manifest = CliCacheManifest::new(
        &identity(),
        InputSnapshot {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        },
        generated_files.clone(),
        None,
        &crate::runtime_descriptor::test_runtime_dependency(),
    );
    let runtime_output = crate::runtime_descriptor::test_runtime_link_output();
    let (rustc_path, rustc_snapshot_identity) = rustc_selection();
    manifest
        .refresh_runtime(&runtime_output, &rustc_path, &rustc_snapshot_identity)
        .unwrap();
    std::fs::write(temp.path().join("main.rs"), generated_source).unwrap();
    let action = crate::rustc::RustcAction::for_generated_binary(
        temp.path(),
        &executable,
        Path::new(runtime_output.artifact_path()),
        runtime_output.link(),
        crate::rustc::AdmittedRustc::new(&rustc_path, &rustc_snapshot_identity),
        &generated_files,
        false,
    )
    .unwrap();
    manifest
        .set_executable("debug", &executable, &action)
        .unwrap();
    manifest.save(temp.path()).unwrap();
    manifest.save_terminal(temp.path()).unwrap();

    let mut loaded: CliCacheManifest =
        serde_json::from_slice(&std::fs::read(temp.path().join(CACHE_MANIFEST_FILENAME)).unwrap())
            .unwrap();
    let generated_json =
        std::fs::read_to_string(temp.path().join(CACHE_MANIFEST_FILENAME)).unwrap();
    assert!(!generated_json.contains("terminal"));
    let dependency = loaded.runtime_dependency().unwrap();
    loaded.terminal =
        TerminalState::load_if_valid(temp.path(), &loaded.generated_identity, &dependency);
    assert!(loaded.executable_is_valid("debug", &executable, &action));
    std::fs::write(&executable, "changed").unwrap();
    assert!(!loaded.executable_is_valid("debug", &executable, &action));
}

#[test]
fn corrupt_terminal_state_does_not_invalidate_generated_rust() {
    let temp = tempfile::tempdir().unwrap();
    let generated_source = "fn main() {}\n";
    std::fs::write(temp.path().join("main.rs"), generated_source).unwrap();
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    let dependency = crate::runtime_descriptor::test_runtime_dependency();
    let mut output_manifest = GeneratedOutputManifest::new(&dependency);
    output_manifest.record(
        "main.rs".to_string(),
        generated_files.get("main.rs").unwrap().clone(),
    );
    output_manifest.save(temp.path()).unwrap();
    let inputs = InputSnapshot {
        files: BTreeMap::new(),
        directories: BTreeMap::new(),
    };
    CliCacheManifest::new(
        &identity(),
        inputs.clone(),
        generated_files,
        None,
        &dependency,
    )
    .save(temp.path())
    .unwrap();
    std::fs::write(temp.path().join(TERMINAL_MANIFEST_FILENAME), b"not-json").unwrap();

    let loaded = CliCacheManifest::load_if_generated_valid(temp.path(), &identity(), &inputs)
        .expect("terminal corruption must not poison generated Rust admission");
    assert!(loaded.selected_runtime().is_none());
}

#[test]
fn executable_metadata_admission_does_not_read_generated_rust() {
    let temp = tempfile::tempdir().unwrap();
    let generated_source = "fn main() {}\n";
    let generated_path = temp.path().join("main.rs");
    std::fs::write(&generated_path, generated_source).unwrap();
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    let dependency = crate::runtime_descriptor::test_runtime_dependency();
    let mut output_manifest = GeneratedOutputManifest::new(&dependency);
    output_manifest.record(
        "main.rs".to_string(),
        generated_files.get("main.rs").unwrap().clone(),
    );
    output_manifest.save(temp.path()).unwrap();
    let inputs = InputSnapshot {
        files: BTreeMap::new(),
        directories: BTreeMap::new(),
    };
    CliCacheManifest::new(
        &identity(),
        inputs.clone(),
        generated_files,
        None,
        &dependency,
    )
    .save(temp.path())
    .unwrap();

    std::fs::write(&generated_path, "fn externally_changed() {}\n").unwrap();
    let metadata =
        CliCacheManifest::load_if_source_revision_matches(temp.path(), &identity(), &inputs)
            .expect("executable admission should not consume generated intermediates");
    assert!(!metadata.generated_files_are_current(temp.path()));
    assert!(CliCacheManifest::load_if_generated_valid(temp.path(), &identity(), &inputs).is_none());
}

#[test]
fn immediate_cache_hit_does_not_rewrite_access_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let generated_source = "fn main() {}\n";
    std::fs::write(temp.path().join("main.rs"), generated_source).unwrap();
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    let dependency = crate::runtime_descriptor::test_runtime_dependency();
    let mut output_manifest = GeneratedOutputManifest::new(&dependency);
    output_manifest.record(
        "main.rs".to_string(),
        generated_files.get("main.rs").unwrap().clone(),
    );
    output_manifest.save(temp.path()).unwrap();
    let inputs = InputSnapshot {
        files: BTreeMap::new(),
        directories: BTreeMap::new(),
    };
    CliCacheManifest::new(
        &identity(),
        inputs.clone(),
        generated_files,
        None,
        &dependency,
    )
    .save(temp.path())
    .unwrap();
    let manifest_path = temp.path().join(CACHE_MANIFEST_FILENAME);
    let before = std::fs::read(&manifest_path).unwrap();

    CliCacheManifest::load_if_source_revision_matches(temp.path(), &identity(), &inputs)
        .expect("fresh cache metadata");

    assert_eq!(std::fs::read(manifest_path).unwrap(), before);
}

#[test]
fn executable_reuse_requires_the_exact_runtime_link_plan() {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("main");
    std::fs::write(&executable, "binary").unwrap();
    let selected = crate::runtime_descriptor::test_runtime_link_descriptor_with_payload(b"one");
    let changed = crate::runtime_descriptor::test_runtime_link_descriptor_with_payload(b"two");
    let generated_source = "fn main() {}\n";
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha2_hash(generated_source.as_bytes()),
    )]);
    let mut manifest = CliCacheManifest::new(
        &identity(),
        InputSnapshot {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        },
        generated_files.clone(),
        None,
        &crate::runtime_descriptor::test_runtime_dependency(),
    );
    let selected_output = crate::runtime_descriptor::RuntimeLinkOutput::new(
        selected.clone(),
        &temp.path().join("runtime.rlib"),
    );
    let (rustc_path, rustc_snapshot_identity) = rustc_selection();
    manifest
        .refresh_runtime(&selected_output, &rustc_path, &rustc_snapshot_identity)
        .unwrap();
    std::fs::write(temp.path().join("main.rs"), generated_source).unwrap();
    let selected_action = crate::rustc::RustcAction::for_generated_binary(
        temp.path(),
        &executable,
        Path::new(selected_output.artifact_path()),
        &selected,
        crate::rustc::AdmittedRustc::new(&rustc_path, &rustc_snapshot_identity),
        &generated_files,
        true,
    )
    .unwrap();
    let changed_action = crate::rustc::RustcAction::for_generated_binary(
        temp.path(),
        &executable,
        Path::new(selected_output.artifact_path()),
        &changed,
        crate::rustc::AdmittedRustc::new(&rustc_path, &rustc_snapshot_identity),
        &generated_files,
        true,
    )
    .unwrap();
    manifest
        .set_executable("release", &executable, &selected_action)
        .unwrap();

    assert!(manifest.executable_is_valid("release", &executable, &selected_action));
    assert!(!manifest.executable_is_valid("release", &executable, &changed_action));
    assert!(!manifest.executable_is_valid("debug", &executable, &selected_action));
}

#[test]
fn cache_access_lock_allows_shared_users_and_excludes_pruning() {
    let temp = tempfile::tempdir().unwrap();
    let first_shared = CacheAccessLock::acquire_shared(temp.path()).unwrap();

    let shared_base = temp.path().to_path_buf();
    let (second_shared_tx, second_shared_rx) = mpsc::channel();
    let second_shared_thread = std::thread::spawn(move || {
        let lock = CacheAccessLock::acquire_shared(&shared_base).unwrap();
        second_shared_tx.send(lock).unwrap();
    });
    let second_shared = second_shared_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("a second cache user should acquire the shared lock");
    second_shared_thread.join().unwrap();

    let exclusive_base = temp.path().to_path_buf();
    let (exclusive_attempt_tx, exclusive_attempt_rx) = mpsc::channel();
    let (exclusive_tx, exclusive_rx) = mpsc::channel();
    let exclusive_thread = std::thread::spawn(move || {
        exclusive_attempt_tx.send(()).unwrap();
        let lock = CacheAccessLock::acquire_exclusive(&exclusive_base).unwrap();
        exclusive_tx.send(lock).unwrap();
    });
    exclusive_attempt_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("exclusive lock attempt");
    assert!(
        exclusive_rx
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "pruning acquired while shared cache users were active"
    );
    drop(first_shared);
    assert!(
        exclusive_rx
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "pruning acquired before every shared cache user exited"
    );
    drop(second_shared);
    let exclusive = exclusive_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("exclusive lock after shared users exit");
    exclusive_thread.join().unwrap();

    let blocked_shared_base = temp.path().to_path_buf();
    let (shared_attempt_tx, shared_attempt_rx) = mpsc::channel();
    let (blocked_shared_tx, blocked_shared_rx) = mpsc::channel();
    let blocked_shared_thread = std::thread::spawn(move || {
        shared_attempt_tx.send(()).unwrap();
        let lock = CacheAccessLock::acquire_shared(&blocked_shared_base).unwrap();
        blocked_shared_tx.send(lock).unwrap();
    });
    shared_attempt_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("shared lock attempt");
    assert!(
        blocked_shared_rx
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "cache user acquired while pruning held the exclusive lock"
    );
    drop(exclusive);
    let shared = blocked_shared_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("shared lock after pruning exits");
    drop(shared);
    blocked_shared_thread.join().unwrap();
}

#[test]
fn pruning_waits_for_active_cache_users_before_removing_entries() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("run").join("old");
    write_expired_manifest(&old);
    let active_user = CacheAccessLock::acquire_shared(temp.path()).unwrap();

    let cache_base = temp.path().to_path_buf();
    let (initial_check_tx, initial_check_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let prune_thread = std::thread::spawn(move || {
        maybe_prune_cli_cache_inner(&cache_base, None, || {
            initial_check_tx.send(()).unwrap();
        })
        .unwrap();
        done_tx.send(()).unwrap();
    });
    initial_check_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("initial prune check");
    assert!(
        done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "pruning completed while a cache user was active"
    );
    assert!(old.exists());

    drop(active_user);
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("pruning after cache user exits");
    prune_thread.join().unwrap();
    assert!(!old.exists());
}

#[test]
fn pruning_rechecks_fresh_marker_after_acquiring_exclusive_lock() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("run").join("old");
    write_expired_manifest(&old);

    let cache_base = temp.path().to_path_buf();
    let marker = temp.path().join(CACHE_PRUNE_MARKER_FILENAME);
    let (initial_check_tx, initial_check_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let prune_thread = std::thread::spawn(move || {
        maybe_prune_cli_cache_inner(&cache_base, None, || {
            initial_check_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
        })
        .unwrap();
    });
    initial_check_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("initial prune check");
    std::fs::write(marker, unix_time_ms().to_string()).unwrap();
    continue_tx.send(()).unwrap();
    prune_thread.join().unwrap();

    assert!(
        old.exists(),
        "a stale pre-lock decision ignored the fresh prune marker"
    );
}

#[test]
fn prune_removes_expired_entries_but_preserves_current_entry() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("run").join("old");
    let keep = temp.path().join("run").join("keep");
    write_expired_manifest(&old);
    write_expired_manifest(&keep);

    prune_cli_cache(temp.path(), Some(&keep), SystemTime::now()).unwrap();
    assert!(!old.exists());
    assert!(keep.exists());
}
