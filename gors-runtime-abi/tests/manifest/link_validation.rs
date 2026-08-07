use super::*;

#[test]
fn link_validation_order_is_schema_contract_target_then_toolchain() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintI64]);
    let other_contract = manifest([], [INT_DIV]);
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let other_target = target_model("wasm32-unknown-unknown")?;
    let expected_toolchain = compatibility_identity(b"expected rustc");
    let other_toolchain = compatibility_identity(b"other rustc");
    let dependency = RuntimeDependency::new(&contract, RuntimeRequirement::default())?;
    let request = RuntimeLinkRequest::new(
        dependency,
        target.clone(),
        RuntimeArtifactFormat::RustRlibV1,
        expected_toolchain,
    );

    let stale = RuntimeArtifactManifest::from_parts(
        ArtifactSchemaVersion::new(1),
        other_contract.identity(),
        other_target.clone(),
        TargetCapabilities::default(),
        RuntimeArtifactFormat::RustRlibV1,
        other_toolchain,
        ImplementationHash::sha256(b"runtime"),
    );
    assert!(matches!(
        stale.select(request.clone()),
        Err(RuntimeLinkError::UnsupportedSchema { .. })
    ));

    let wrong_contract = artifact(
        &other_contract,
        other_target.clone(),
        [],
        other_toolchain,
        b"runtime",
    );
    assert!(matches!(
        wrong_contract.select(request.clone()),
        Err(RuntimeLinkError::ContractMismatch { .. })
    ));

    let wrong_target = artifact(&contract, other_target, [], other_toolchain, b"runtime");
    assert!(matches!(
        wrong_target.select(request.clone()),
        Err(RuntimeLinkError::TargetMismatch { .. })
    ));

    let wrong_toolchain = artifact(&contract, target, [], other_toolchain, b"runtime");
    assert!(matches!(
        wrong_toolchain.select(request),
        Err(RuntimeLinkError::CompatibilityMismatch { .. })
    ));
    Ok(())
}
