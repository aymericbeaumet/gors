use super::*;

fn test_marker(operation_ids: Vec<u16>) -> CacheMarker {
    CacheMarker {
        schema_version: CACHE_MARKER_SCHEMA,
        fixture_cache_identity: "11".repeat(32),
        runtime_dependency: RuntimeDependencyMarker {
            schema_version: gors_runtime_abi::CURRENT_RUNTIME_DEPENDENCY_SCHEMA,
            contract_identity: gors_runtime_abi::RuntimeAbiManifest::current()
                .identity()
                .to_string(),
            operation_ids,
        },
        link_plan_identity: "22".repeat(32),
        binary_sha256: "33".repeat(32),
    }
}

#[test]
fn marker_decoder_requires_the_canonical_strict_schema() {
    let marker = test_marker(Vec::new());
    let canonical = marker.canonical_bytes().unwrap();
    assert_eq!(CacheMarker::decode_canonical(&canonical).unwrap(), marker);

    let mut noncanonical = b" ".to_vec();
    noncanonical.extend_from_slice(&canonical);
    assert!(CacheMarker::decode_canonical(&noncanonical).is_err());

    let mut unknown_field = String::from_utf8(canonical).unwrap();
    unknown_field.replace_range(unknown_field.len() - 2.., ",\"unknown\":true}\n");
    assert!(CacheMarker::decode_canonical(unknown_field.as_bytes()).is_err());
}

#[test]
fn marker_dependency_rejects_noncanonical_operation_ids() {
    let operation = gors_runtime_abi::RuntimeAbiManifest::current()
        .runtime_ops()
        .first()
        .copied()
        .unwrap();
    let operation_id = operation.id().get();

    let duplicate = test_marker(vec![operation_id, operation_id]);
    assert!(duplicate.current_dependency(&"11".repeat(32)).is_err());

    let unknown = test_marker(vec![u16::MAX]);
    assert!(unknown.current_dependency(&"11".repeat(32)).is_err());
}

#[test]
fn binary_publication_binds_and_validates_payload_integrity() {
    let root = tempfile::tempdir().unwrap();
    let entry = FixtureCacheEntry::acquire(root.path().join("entry"), "44".repeat(32)).unwrap();
    let binary = entry.path().join("main");
    fs::write(&binary, b"untrusted executable").unwrap();
    fs::write(entry.path().join(CACHE_MARKER_FILENAME), b"{}\n").unwrap();
    assert!(!admit_cached_binary(&entry, &binary).unwrap());

    let manifest = gors_runtime_abi::RuntimeAbiManifest::current();
    let dependency = gors_runtime_abi::RuntimeDependency::new(
        &manifest,
        gors_runtime_abi::RuntimeRequirement::default(),
    )
    .unwrap();
    let runtime = resolve_runtime(dependency.clone()).unwrap();
    let pending = reserve_pending_binary(&entry).unwrap();
    fs::write(&pending, b"complete generated executable").unwrap();

    publish_compiled_binary(&entry, &pending, &binary, &dependency, runtime.link_plan).unwrap();
    assert!(!pending.exists());
    assert!(admit_cached_binary(&entry, &binary).unwrap());

    fs::write(&binary, b"corrupt").unwrap();
    assert!(!admit_cached_binary(&entry, &binary).unwrap());
}
