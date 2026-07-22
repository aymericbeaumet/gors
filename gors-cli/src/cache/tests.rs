#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::sync::mpsc;

fn request() -> CacheRequest {
    CacheRequest {
        fingerprint: "request".to_string(),
    }
}

fn request_options(source_paths: &[String]) -> CacheRequestOptions<'_> {
    CacheRequestOptions {
        command: "build",
        source_paths,
        release: false,
        output: None,
        sourcemap: None,
    }
}

fn write_expired_manifest(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    let old_timestamp = unix_time_ms()
        .saturating_sub(CACHE_MAX_AGE.as_millis().try_into().unwrap())
        .saturating_sub(1);
    CliCacheManifest::new(
        &request(),
        InputSnapshot {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        },
        BTreeMap::new(),
        None,
    )
    .with_last_used(old_timestamp)
    .save(path)
    .unwrap();
}

#[test]
fn cache_request_tracks_cli_abi() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.go");
    std::fs::write(&source, "package main\n").unwrap();
    let source_paths = vec![source.to_string_lossy().into_owned()];

    let initial =
        CacheRequest::new_with_identity(request_options(&source_paths), "cli-abi-a", None).unwrap();
    let changed_abi =
        CacheRequest::new_with_identity(request_options(&source_paths), "cli-abi-b", None).unwrap();

    assert_ne!(initial, changed_abi);
}

#[test]
fn cache_request_tracks_gorspath_identity_without_scanning_root_contents() {
    let invocation = tempfile::tempdir().unwrap();
    let source = invocation.path().join("main.go");
    std::fs::write(&source, "package main\n").unwrap();
    let source_paths = vec![source.to_string_lossy().into_owned()];

    let first_root = tempfile::tempdir().unwrap();
    let package_dir = first_root
        .path()
        .join("src")
        .join("example")
        .join("dependency");
    std::fs::create_dir_all(&package_dir).unwrap();
    let dependency = package_dir.join("dependency.go");
    std::fs::write(&dependency, "package dependency\nconst Value = 1\n").unwrap();
    let first_gorspath = std::env::join_paths([first_root.path()]).unwrap();

    let initial = CacheRequest::new_with_identity(
        request_options(&source_paths),
        "cli-abi",
        Some(first_gorspath.as_os_str()),
    )
    .unwrap();
    let unchanged = CacheRequest::new_with_identity(
        request_options(&source_paths),
        "cli-abi",
        Some(first_gorspath.as_os_str()),
    )
    .unwrap();
    assert_eq!(initial, unchanged);

    std::fs::write(&dependency, "package dependency\nconst Value = 2\n").unwrap();
    let edited = CacheRequest::new_with_identity(
        request_options(&source_paths),
        "cli-abi",
        Some(first_gorspath.as_os_str()),
    )
    .unwrap();
    assert_eq!(initial, edited);

    std::fs::write(package_dir.join("added.go"), "package dependency\n").unwrap();
    let added = CacheRequest::new_with_identity(
        request_options(&source_paths),
        "cli-abi",
        Some(first_gorspath.as_os_str()),
    )
    .unwrap();
    assert_eq!(edited, added);

    let second_root = tempfile::tempdir().unwrap();
    let second_gorspath = std::env::join_paths([second_root.path()]).unwrap();
    let changed_value = CacheRequest::new_with_identity(
        request_options(&source_paths),
        "cli-abi",
        Some(second_gorspath.as_os_str()),
    )
    .unwrap();
    assert_ne!(added, changed_value);
}

#[test]
fn input_snapshot_detects_file_edits_and_directory_membership_changes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.go");
    std::fs::write(&source, "package main\n").unwrap();
    let directory = normalized_path(temp.path()).unwrap();
    let snapshot = InputSnapshot {
        files: std::iter::once((
            normalized_path(&source).unwrap(),
            file_hash(&source).unwrap(),
        ))
        .collect(),
        directories: std::iter::once((directory, vec![normalized_path(&source).unwrap()]))
            .collect(),
    };
    assert!(snapshot.is_current());

    std::fs::write(&source, "package changed\n").unwrap();
    assert!(!snapshot.is_current());
    std::fs::write(&source, "package main\n").unwrap();
    assert!(snapshot.is_current());

    std::fs::write(temp.path().join("added.go"), "package main\n").unwrap();
    assert!(!snapshot.is_current());
}

#[test]
fn input_snapshot_uses_the_loaded_revision_without_rereading_sources() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("main.go");
    std::fs::write(&source, "package main\n").unwrap();
    let loaded = gors::workspace::load_program(temp.path()).unwrap();

    std::fs::write(&source, "package changed\n").unwrap();
    std::fs::write(temp.path().join("added.go"), "package main\n").unwrap();
    let snapshot = InputSnapshot::capture(&loaded).unwrap();

    assert_eq!(
        snapshot.files.get(&normalized_path(&source).unwrap()),
        Some(&sha2_hash(b"package main\n"))
    );
    assert!(!snapshot.is_current());
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
    std::fs::write(&executable, "binary").unwrap();
    let mut manifest = CliCacheManifest::new(
        &request(),
        InputSnapshot {
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        },
        BTreeMap::new(),
        None,
    );
    manifest.set_executable(&executable).unwrap();
    manifest.save(temp.path()).unwrap();

    let loaded: CliCacheManifest =
        serde_json::from_slice(&std::fs::read(temp.path().join(CACHE_MANIFEST_FILENAME)).unwrap())
            .unwrap();
    assert!(loaded.executable_is_valid(&executable));
    std::fs::write(&executable, "changed").unwrap();
    assert!(!loaded.executable_is_valid(&executable));
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
