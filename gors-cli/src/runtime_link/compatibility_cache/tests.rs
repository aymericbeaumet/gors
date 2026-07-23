//! Compatibility-cache corruption, mutation, and concurrency coverage.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use gors_runtime_abi::{
    DataWidth, Endianness, RustRlibCompatibility, TargetModel, canonical_target_libdir_record,
};

use super::probe::{CompatibilityProbe, ProbeError};
use super::{
    CACHE_FILENAME, CACHE_SCHEMA, CompatibilityCacheError, ResolvedRustcCompatibility,
    SnapshotError, cache_entry_directory, resolve_with_probe,
};

const SELECTOR: &str = "test-toolchain";
const RUSTC_VERBOSE_VERSION: &[u8] =
    b"rustc 1.96.0 (test 2026-05-25)\nhost: test-host\nrelease: 1.96.0\n";

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn test_error(message: &'static str) -> std::io::Error {
    std::io::Error::other(message)
}

struct FakeProbe {
    rustc: Mutex<PathBuf>,
    target_libdir: Mutex<PathBuf>,
    rustc_verbose_version: Mutex<Vec<u8>>,
    which_calls: AtomicUsize,
    verbose_calls: AtomicUsize,
    target_libdir_calls: AtomicUsize,
}

impl FakeProbe {
    fn new(rustc: PathBuf, target_libdir: PathBuf) -> Self {
        Self {
            rustc: Mutex::new(rustc),
            target_libdir: Mutex::new(target_libdir),
            rustc_verbose_version: Mutex::new(RUSTC_VERBOSE_VERSION.to_vec()),
            which_calls: AtomicUsize::new(0),
            verbose_calls: AtomicUsize::new(0),
            target_libdir_calls: AtomicUsize::new(0),
        }
    }
}

impl CompatibilityProbe for FakeProbe {
    fn resolve_rustc(&self, _selector: &str) -> Result<PathBuf, ProbeError> {
        self.which_calls.fetch_add(1, Ordering::SeqCst);
        self.rustc
            .lock()
            .map(|path| path.clone())
            .map_err(|_| ProbeError::new("fake rustc path lock is poisoned"))
    }

    fn rustc_verbose_version(&self, _rustc: &Path) -> Result<Vec<u8>, ProbeError> {
        self.verbose_calls.fetch_add(1, Ordering::SeqCst);
        self.rustc_verbose_version
            .lock()
            .map(|record| record.clone())
            .map_err(|_| ProbeError::new("fake rustc version lock is poisoned"))
    }

    fn target_libdir(&self, _rustc: &Path, _target: &str) -> Result<PathBuf, ProbeError> {
        self.target_libdir_calls.fetch_add(1, Ordering::SeqCst);
        self.target_libdir
            .lock()
            .map(|path| path.clone())
            .map_err(|_| ProbeError::new("fake target-libdir lock is poisoned"))
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    cache_root: PathBuf,
    rustc: PathBuf,
    target_libdir: PathBuf,
    unhashed_library: PathBuf,
    target: TargetModel,
    probe: Arc<FakeProbe>,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let rustc = temporary.path().join("toolchain/bin/rustc");
        let rustc_parent = rustc
            .parent()
            .ok_or_else(|| test_error("fake rustc path has no parent"))?;
        std::fs::create_dir_all(rustc_parent)?;
        std::fs::write(&rustc, b"fake-rustc")?;
        let target_libdir = temporary.path().join("toolchain/lib/rustlib/test/lib");
        std::fs::create_dir_all(target_libdir.join("nested"))?;
        std::fs::write(
            target_libdir.join("libcore-0123456789abcdef.rlib"),
            b"hash-named",
        )?;
        let unhashed_library = target_libdir.join("native.a");
        std::fs::write(&unhashed_library, b"native-one")?;
        std::fs::write(target_libdir.join("nested/crt.o"), b"crt")?;
        let target = TargetModel::new("test-target", DataWidth::Bits64, Endianness::Little)?;
        let probe = Arc::new(FakeProbe::new(rustc.clone(), target_libdir.clone()));
        let cache_root = temporary.path().join("cache/toolchains");
        Ok(Self {
            _temporary: temporary,
            cache_root,
            rustc,
            target_libdir,
            unhashed_library,
            target,
            probe,
        })
    }

    fn expected(&self) -> Result<RustRlibCompatibility, Box<dyn std::error::Error>> {
        Ok(RustRlibCompatibility::new(
            RUSTC_VERBOSE_VERSION,
            canonical_target_libdir_record(&self.target_libdir)?,
            self.target.clone(),
        )?)
    }

    fn resolve(
        &self,
        expected: &RustRlibCompatibility,
    ) -> Result<ResolvedRustcCompatibility, CompatibilityCacheError> {
        resolve_with_probe(
            &self.cache_root,
            SELECTOR,
            &self.target,
            expected,
            self.probe.as_ref(),
        )
    }

    fn cache_path(&self) -> PathBuf {
        cache_entry_directory(&self.cache_root, SELECTOR, &self.target).join(CACHE_FILENAME)
    }
}

#[test]
fn warm_hit_only_resolves_rustc_and_reuses_canonical_compatibility() -> TestResult {
    let fixture = Fixture::new()?;
    let expected = fixture.expected()?;

    let cold = fixture.resolve(&expected)?;
    let warm = fixture.resolve(&expected)?;

    assert_eq!(
        cold.compatibility().canonical_bytes(),
        expected.canonical_bytes()
    );
    assert_eq!(
        warm.compatibility().canonical_bytes(),
        expected.canonical_bytes()
    );
    assert_eq!(cold.rustc_path(), fixture.rustc);
    assert_eq!(warm.rustc_path(), fixture.rustc);
    assert_eq!(
        cold.rustc_snapshot_identity(),
        warm.rustc_snapshot_identity()
    );
    assert_eq!(cold.rustc_snapshot_identity().len(), 64);
    let canonical_target_libdir = std::fs::canonicalize(&fixture.target_libdir)?;
    assert_eq!(cold.target_libdir(), canonical_target_libdir.as_path());
    assert_eq!(warm.target_libdir(), canonical_target_libdir.as_path());
    assert_eq!(
        cold.target_libdir_snapshot_identity(),
        warm.target_libdir_snapshot_identity()
    );
    assert_eq!(cold.target_libdir_snapshot_identity().len(), 64);
    assert_eq!(fixture.probe.which_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 1);
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.cache_path())?)?;
    let payload = document
        .get("payload")
        .ok_or_else(|| test_error("cache document has no payload"))?;
    let expected_identity = expected.identity().to_string();
    assert_eq!(
        payload
            .get("compatibility_identity")
            .and_then(|value| value.as_str()),
        Some(expected_identity.as_str())
    );
    assert!(payload.get("compatibility_record").is_some());
    assert!(payload.get("rustc_release_record").is_some());
    assert!(payload.get("target_libdir_record").is_some());
    assert!(payload.get("rustc_snapshot").is_some());
    assert_eq!(
        payload
            .get("rustc_snapshot_identity")
            .and_then(|value| value.as_str()),
        Some(cold.rustc_snapshot_identity())
    );
    assert!(payload.get("target_libdir_snapshot").is_some());
    Ok(())
}

#[test]
fn malformed_unknown_and_bad_checksum_records_are_reprobed_and_repaired() -> TestResult {
    let fixture = Fixture::new()?;
    let expected = fixture.expected()?;
    fixture.resolve(&expected)?;
    let cache_path = fixture.cache_path();

    std::fs::write(&cache_path, b"{")?;
    fixture.resolve(&expected)?;

    let mut unknown: serde_json::Value = serde_json::from_slice(&std::fs::read(&cache_path)?)?;
    unknown
        .as_object_mut()
        .ok_or_else(|| test_error("cache document is not an object"))?
        .insert("future_field".to_owned(), serde_json::json!(true));
    std::fs::write(&cache_path, serde_json::to_vec(&unknown)?)?;
    fixture.resolve(&expected)?;

    let mut bad_checksum: serde_json::Value = serde_json::from_slice(&std::fs::read(&cache_path)?)?;
    bad_checksum
        .as_object_mut()
        .ok_or_else(|| test_error("cache document is not an object"))?
        .insert("checksum".to_owned(), serde_json::json!("00".repeat(32)));
    std::fs::write(&cache_path, serde_json::to_vec(&bad_checksum)?)?;
    fixture.resolve(&expected)?;

    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 4);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 4);
    let repaired: serde_json::Value = serde_json::from_slice(&std::fs::read(cache_path)?)?;
    assert_eq!(
        repaired.get("schema_version"),
        Some(&serde_json::json!(CACHE_SCHEMA))
    );
    assert!(repaired.get("future_field").is_none());
    assert_ne!(
        repaired.get("checksum"),
        Some(&serde_json::json!("00".repeat(32)))
    );
    Ok(())
}

#[test]
fn rustc_and_recursive_target_metadata_mutations_force_full_reprobes() -> TestResult {
    let fixture = Fixture::new()?;
    let initial = fixture.expected()?;
    fixture.resolve(&initial)?;

    std::fs::write(&fixture.unhashed_library, b"native-library-grew")?;
    let changed = fixture.expected()?;
    assert_ne!(initial.identity(), changed.identity());
    let before_rustc_change = fixture.resolve(&changed)?;

    std::fs::write(&fixture.rustc, b"fake-rustc-with-new-size")?;
    let after_rustc_change = fixture.resolve(&changed)?;

    assert_ne!(
        before_rustc_change.rustc_snapshot_identity(),
        after_rustc_change.rustc_snapshot_identity()
    );

    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[test]
fn expected_provider_mismatch_forces_reprobe_and_fails_closed() -> TestResult {
    let fixture = Fixture::new()?;
    let actual = fixture.expected()?;
    fixture.resolve(&actual)?;
    let different = RustRlibCompatibility::new(
        RUSTC_VERBOSE_VERSION,
        b"different-target-record".as_slice(),
        fixture.target.clone(),
    )?;

    let error = match fixture.resolve(&different) {
        Ok(_) => return Err(test_error("provider mismatch was unexpectedly admitted").into()),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        CompatibilityCacheError::ProviderMismatch { .. }
    ));
    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn escaping_target_libdir_symlink_is_never_admitted_from_snapshot() -> TestResult {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new()?;
    let symlink_path = fixture.target_libdir.join("native-link");
    symlink("native.a", &symlink_path)?;
    let expected = fixture.expected()?;
    fixture.resolve(&expected)?;

    std::fs::remove_file(&symlink_path)?;
    symlink("../../../../escape", &symlink_path)?;
    let error = match fixture.resolve(&expected) {
        Ok(_) => return Err(test_error("escaping symlink was unexpectedly admitted").into()),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        CompatibilityCacheError::Snapshot(SnapshotError::EscapingSymlink { .. })
            | CompatibilityCacheError::TargetInventory(_)
    ));
    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn concurrent_misses_publish_once_under_the_per_key_os_lock() -> TestResult {
    let fixture = Fixture::new()?;
    let expected = fixture.expected()?;
    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let mut handles = Vec::new();

    for _ in 0..workers {
        let cache_root = fixture.cache_root.clone();
        let target = fixture.target.clone();
        let expected = expected.clone();
        let probe = Arc::clone(&fixture.probe);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            resolve_with_probe(&cache_root, SELECTOR, &target, &expected, probe.as_ref())
                .map(|resolved| resolved.compatibility().identity())
        }));
    }

    for handle in handles {
        let identity = handle
            .join()
            .map_err(|_| test_error("compatibility cache worker panicked"))??;
        assert_eq!(identity, expected.identity());
    }
    assert_eq!(fixture.probe.which_calls.load(Ordering::SeqCst), workers);
    assert_eq!(fixture.probe.verbose_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.probe.target_libdir_calls.load(Ordering::SeqCst), 1);
    Ok(())
}
