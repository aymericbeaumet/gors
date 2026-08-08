use super::*;

#[test]
fn semantic_contract_dimensions_change_only_the_contract_hash() {
    let baseline = manifest([PrimitiveOp::BoolNot], [INT_DIV]);
    let schema = RuntimeAbiManifest::new(
        gors_runtime_abi::ManifestSchemaVersion::new(3),
        baseline.contract(),
        baseline.semantics(),
        baseline.primitive_ops().iter().copied(),
        baseline.runtime_ops().iter().copied(),
    );
    let version = RuntimeAbiManifest::new(
        baseline.schema(),
        ContractVersion::new(1, 2, 4),
        baseline.semantics(),
        baseline.primitive_ops().iter().copied(),
        baseline.runtime_ops().iter().copied(),
    );
    let semantics = RuntimeAbiManifest::new(
        baseline.schema(),
        baseline.contract(),
        GoSemanticModel::new(DataWidth::Bits32),
        baseline.primitive_ops().iter().copied(),
        baseline.runtime_ops().iter().copied(),
    );
    let primitive = RuntimeAbiManifest::new(
        baseline.schema(),
        baseline.contract(),
        baseline.semantics(),
        [PrimitiveOp::BoolNot, PrimitiveOp::BoolEqual],
        baseline.runtime_ops().iter().copied(),
    );
    let runtime = RuntimeAbiManifest::new(
        baseline.schema(),
        baseline.contract(),
        baseline.semantics(),
        baseline.primitive_ops().iter().copied(),
        [INT_DIV, INT_REM],
    );

    for changed in [schema, version, semantics, primitive, runtime] {
        assert_ne!(baseline.identity(), changed.identity());
    }
}

#[test]
fn target_and_implementation_change_artifact_but_not_contract_identity()
-> Result<(), Box<dyn Error>> {
    let contract = manifest([], [INT_DIV]);
    let first = artifact(
        &contract,
        target_model("wasm32-unknown-unknown")?,
        [],
        compatibility_identity(b"rustc one"),
        b"implementation one",
    );
    let second = artifact(
        &contract,
        target_model("x86_64-unknown-linux-gnu")?,
        [TargetCapability::Threads],
        compatibility_identity(b"rustc two"),
        b"implementation two",
    );

    assert_eq!(first.contract(), contract.identity());
    assert_eq!(second.contract(), contract.identity());
    assert_eq!(first.schema(), CURRENT_ARTIFACT_SCHEMA);
    assert_eq!(first.format(), RuntimeArtifactFormat::RustRlibV1);
    assert!(first.verifies_payload(b"implementation one"));
    assert!(!first.verifies_payload(b"implementation two"));
    assert_ne!(first.identity(), second.identity());
    Ok(())
}

#[test]
fn runtime_requirements_are_canonical_and_composable() {
    let left = RuntimeRequirement::new([
        RuntimeOp::PrintI64,
        RuntimeOp::ConcatGoStrings,
        INT_DIV,
        RuntimeOp::PrintI64,
    ]);
    let reordered =
        RuntimeRequirement::new([INT_DIV, RuntimeOp::PrintI64, RuntimeOp::ConcatGoStrings]);

    assert_eq!(left, reordered);
    assert_eq!(
        left.as_slice(),
        [RuntimeOp::ConcatGoStrings, INT_DIV, RuntimeOp::PrintI64]
    );
    assert_eq!(left.iter().collect::<Vec<_>>(), left.as_slice());
    assert_eq!((&left).into_iter().collect::<Vec<_>>(), left.as_slice());
    assert!(left.contains(INT_DIV));
    assert!(!left.contains(INT_REM));
    assert_eq!(left.len(), 3);
    assert!(!left.is_empty());

    let union = left.union(&RuntimeRequirement::new([INT_DIV, RuntimeOp::PrintNewline]));
    assert_eq!(
        union.as_slice(),
        [
            RuntimeOp::ConcatGoStrings,
            INT_DIV,
            RuntimeOp::PrintI64,
            RuntimeOp::PrintNewline
        ]
    );
    assert_eq!(
        union.required_capabilities().as_slice(),
        [TargetCapability::StandardIo]
    );

    let empty = RuntimeRequirement::default();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.required_capabilities().as_slice().is_empty());
}

#[test]
fn runtime_requirement_wire_ids_round_trip_and_reject_unknown_values() -> Result<(), Box<dyn Error>>
{
    let requirement =
        RuntimeRequirement::new([RuntimeOp::PrintNewline, INT_DIV, RuntimeOp::PrintNewline]);
    let ids = requirement.operation_ids().collect::<Vec<_>>();

    assert_eq!(ids, [8, 16]);
    assert_eq!(RuntimeRequirement::from_operation_ids(ids)?, requirement);
    let error = match RuntimeRequirement::from_operation_ids([8, 12, 16]) {
        Ok(_) => return Err("unknown stable IDs must fail closed".into()),
        Err(error) => error,
    };
    assert_eq!(error.get(), 12);
    assert_eq!(error.to_string(), "unknown runtime operation ID 12");
    Ok(())
}

#[test]
fn artifact_selection_enforces_runtime_capability_requirements() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [INT_DIV, RuntimeOp::PrintI64]);
    let target = target_model("wasm32-unknown-unknown")?;
    let toolchain = compatibility_identity(b"rustc");
    let provider = artifact(&contract, target.clone(), [], toolchain, b"runtime");

    let pure = provider.select(request(&contract, [INT_DIV], target.clone(), toolchain)?)?;
    assert_eq!(pure.request().dependency().requirement().len(), 1);

    let Err(error) = provider.select(request(
        &contract,
        [RuntimeOp::PrintI64],
        target,
        toolchain,
    )?) else {
        return Err("stdio use was not checked against the program requirement".into());
    };
    assert!(matches!(
        error,
        RuntimeLinkError::MissingCapability {
            operation: RuntimeOp::PrintI64,
            capability: TargetCapability::StandardIo,
        }
    ));
    Ok(())
}

#[test]
fn artifact_capabilities_are_canonicalized() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintBool]);
    let left = artifact(
        &contract,
        target_model("wasm32-unknown-unknown")?,
        [
            TargetCapability::StandardIo,
            TargetCapability::Atomics32,
            TargetCapability::StandardIo,
        ],
        compatibility_identity(b"rustc"),
        b"runtime",
    );
    let right = artifact(
        &contract,
        target_model("wasm32-unknown-unknown")?,
        [TargetCapability::Atomics32, TargetCapability::StandardIo],
        compatibility_identity(b"rustc"),
        b"runtime",
    );

    assert_eq!(left, right);
    assert_eq!(
        left.provided_capabilities().as_slice(),
        [TargetCapability::Atomics32, TargetCapability::StandardIo]
    );
    assert_eq!(left.canonical_bytes(), right.canonical_bytes());
    assert_eq!(left.identity(), right.identity());
    Ok(())
}

#[test]
fn empty_operation_requirement_still_selects_a_runtime() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintI64]);
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let toolchain = compatibility_identity(b"rustc");
    let provider = artifact(&contract, target.clone(), [], toolchain, b"runtime");
    let plan = provider.select(request(&contract, [], target, toolchain)?)?;

    assert!(plan.request().dependency().requirement().is_empty());
    assert_eq!(plan.artifact(), provider.identity());
    assert_eq!(plan.implementation(), provider.implementation());
    Ok(())
}

#[test]
fn dependency_rejects_an_operation_outside_its_contract() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [INT_DIV]);
    let Err(error) =
        RuntimeDependency::new(&contract, RuntimeRequirement::new([RuntimeOp::PrintI64]))
    else {
        return Err("a consumer selected an operation absent from its contract".into());
    };
    assert_eq!(error.operation(), RuntimeOp::PrintI64);
    Ok(())
}
