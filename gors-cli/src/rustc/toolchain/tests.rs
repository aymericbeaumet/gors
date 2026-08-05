#![allow(clippy::unwrap_used)]

use super::*;

struct Fixture {
    _directory: tempfile::TempDir,
    rustc: PathBuf,
    linker: PathBuf,
    target_libdir: PathBuf,
    descriptor: TerminalToolchain,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let rustc = directory.path().join("rustc");
        let linker = directory.path().join("linker");
        let target_libdir = directory.path().join("target-libdir");
        std::fs::create_dir(&target_libdir).unwrap();
        std::fs::write(&rustc, b"test-rustc").unwrap();
        std::fs::write(&linker, b"test-linker").unwrap();
        make_executable(&rustc);
        make_executable(&linker);
        let descriptor =
            TerminalToolchain::for_test(&rustc, &linker, &target_libdir, "test-target").unwrap();
        Self {
            _directory: directory,
            rustc,
            linker,
            target_libdir,
            descriptor,
        }
    }
}

#[test]
fn descriptor_is_canonical_stable_and_content_addressed() {
    let fixture = Fixture::new();
    let reconstructed = TerminalToolchain::for_test(
        &fixture.rustc,
        &fixture.linker,
        &fixture.target_libdir,
        "test-target",
    )
    .unwrap();

    assert!(fixture.descriptor.is_canonical());
    assert_eq!(fixture.descriptor, reconstructed);
    assert_eq!(
        fixture.descriptor.identity().unwrap(),
        reconstructed.identity().unwrap()
    );
    assert_eq!(fixture.descriptor.identity().unwrap().len(), 64);

    std::fs::write(&fixture.linker, b"other-linker").unwrap();
    make_executable(&fixture.linker);
    let changed = TerminalToolchain::for_test(
        &fixture.rustc,
        &fixture.linker,
        &fixture.target_libdir,
        "test-target",
    )
    .unwrap();
    assert_ne!(
        fixture.descriptor.identity().unwrap(),
        changed.identity().unwrap()
    );
}

#[test]
fn manifest_reconstruction_performs_no_tool_io_but_execution_admission_does() {
    let fixture = Fixture::new();
    let encoded = serde_json::to_vec(&fixture.descriptor).unwrap();
    std::fs::remove_file(&fixture.rustc).unwrap();
    std::fs::remove_file(&fixture.linker).unwrap();

    let reconstructed: TerminalToolchain = serde_json::from_slice(&encoded).unwrap();
    assert!(reconstructed.is_canonical());
    assert_eq!(reconstructed, fixture.descriptor);
    assert!(reconstructed.verify_live().is_err());
}

#[test]
fn live_verification_rejects_linker_revision_drift() {
    let fixture = Fixture::new();
    std::fs::write(&fixture.linker, b"changed-linker").unwrap();
    make_executable(&fixture.linker);

    assert!(matches!(
        fixture.descriptor.verify_live(),
        Err(TerminalToolchainError::ToolChanged { role: "linker", .. })
    ));
}

#[test]
fn live_verification_rejects_target_libdir_drift() {
    let fixture = Fixture::new();
    std::fs::write(fixture.target_libdir.join("libcore.rlib"), b"changed").unwrap();

    assert!(matches!(
        fixture.descriptor.verify_live(),
        Err(TerminalToolchainError::TargetLibdirSnapshotChanged { .. })
    ));
}

#[test]
fn semantic_toolchain_identity_excludes_target_libdir_revision_facts() {
    let fixture = Fixture::new();
    std::fs::write(fixture.target_libdir.join("libcore.rlib"), b"changed").unwrap();
    let changed = TerminalToolchain::for_test(
        &fixture.rustc,
        &fixture.linker,
        &fixture.target_libdir,
        "test-target",
    )
    .unwrap();

    assert_ne!(fixture.descriptor, changed);
    assert_eq!(
        fixture.descriptor.identity().unwrap(),
        changed.identity().unwrap(),
        "target-libdir contents belong to the separate runtime compatibility identity"
    );
}

#[test]
fn deterministic_environment_does_not_capture_unrelated_ambient_variables() {
    let fixture = Fixture::new();
    let environment = fixture.descriptor.environment();

    assert_eq!(environment.get("LANG").map(String::as_str), Some("C"));
    assert_eq!(environment.get("LC_ALL").map(String::as_str), Some("C"));
    assert_eq!(
        environment.get("SOURCE_DATE_EPOCH").map(String::as_str),
        Some("0")
    );
    assert!(!environment.contains_key("HOME"));
    assert!(!environment.contains_key("RUSTFLAGS"));
    assert!(!environment.contains_key("CARGO_ENCODED_RUSTFLAGS"));
}

#[test]
fn canonical_environment_rejects_modified_fixed_values_and_unexpected_keys() {
    let fixture = Fixture::new();

    let mut changed_locale = fixture.descriptor.clone();
    changed_locale
        .environment
        .insert("LANG".to_string(), "en_US.UTF-8".to_string());
    assert!(!changed_locale.is_canonical());

    let mut empty_path = fixture.descriptor.clone();
    empty_path
        .environment
        .insert("PATH".to_string(), String::new());
    assert!(!empty_path.is_canonical());

    let mut injected = fixture.descriptor;
    injected
        .environment
        .insert("RUSTFLAGS".to_string(), "-Ctarget-cpu=native".to_string());
    assert!(!injected.is_canonical());
}

#[test]
fn descriptor_rejects_unknown_nested_fields_and_wrong_schema() {
    let fixture = Fixture::new();
    let mut value = serde_json::to_value(&fixture.descriptor).unwrap();

    value
        .get_mut("rustc")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .insert("future".to_string(), serde_json::json!(true));
    assert!(serde_json::from_value::<TerminalToolchain>(value).is_err());

    let mut value = serde_json::to_value(&fixture.descriptor).unwrap();
    value.as_object_mut().unwrap().insert(
        "schema_version".to_string(),
        serde_json::json!(TOOLCHAIN_SCHEMA - 1),
    );
    let stale: TerminalToolchain = serde_json::from_value(value).unwrap();
    assert!(!stale.is_canonical());
}

#[test]
fn descriptor_rejects_target_platform_mismatch() {
    let fixture = Fixture::new();
    let mut descriptor = fixture.descriptor;
    descriptor.target = "aarch64-apple-darwin".to_string();

    assert!(!descriptor.is_canonical());
}

#[test]
fn linker_selection_rejects_unsupported_apple_and_windows_abis() {
    assert!(matches!(
        resolve_linker("aarch64-apple-ios"),
        Err(TerminalToolchainError::LinkerResolution(_))
    ));
    assert!(matches!(
        resolve_linker("x86_64-pc-windows-gnu"),
        Err(TerminalToolchainError::LinkerResolution(_))
    ));
    assert!(matches!(
        resolve_linker("wasm32-unknown-unknown"),
        Err(TerminalToolchainError::LinkerResolution(_))
    ));
}

#[test]
fn deployment_target_values_are_strict_dotted_versions() {
    for valid in ["10.12", "11.0", "14.4.1"] {
        assert!(canonical_deployment_target_value(valid), "{valid}");
    }
    for invalid in [
        "",
        "11",
        "11.",
        ".11",
        "11.0.0.1",
        "11.a",
        "11.0\nOTHER=1",
        "1000.0",
    ] {
        assert!(!canonical_deployment_target_value(invalid), "{invalid}");
    }
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
