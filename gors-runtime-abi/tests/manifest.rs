use std::collections::BTreeSet;
use std::error::Error;

use sha2::{Digest as _, Sha256};

use gors_runtime_abi::{
    CURRENT_ARTIFACT_SCHEMA, CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, Endianness,
    GoSemanticModel, ImplementationHash, PrimitiveOp, RuntimeAbiManifest, RuntimeArtifactManifest,
    RuntimeOp, RuntimeType, TargetCapabilities, TargetCapability, TargetModel, TargetModelError,
};

fn target(
    triple: &str,
    capabilities: impl IntoIterator<Item = TargetCapability>,
) -> Result<TargetModel, TargetModelError> {
    TargetModel::new(
        triple,
        DataWidth::Bits32,
        Endianness::Little,
        TargetCapabilities::new(capabilities),
    )
}

fn manifest(
    primitive_ops: impl IntoIterator<Item = PrimitiveOp>,
    runtime_ops: impl IntoIterator<Item = RuntimeOp>,
) -> RuntimeAbiManifest {
    RuntimeAbiManifest::new(
        CURRENT_MANIFEST_SCHEMA,
        ContractVersion::new(1, 2, 3),
        GoSemanticModel::new(DataWidth::Bits64),
        primitive_ops,
        runtime_ops,
    )
}

#[test]
fn contract_canonicalization_removes_input_order_and_duplicates() {
    let left = manifest(
        [
            PrimitiveOp::IntEqual,
            PrimitiveOp::BoolNot,
            PrimitiveOp::IntEqual,
        ],
        [RuntimeOp::PrintI64, RuntimeOp::IntDiv, RuntimeOp::PrintI64],
    );
    let right = manifest(
        [PrimitiveOp::BoolNot, PrimitiveOp::IntEqual],
        [RuntimeOp::IntDiv, RuntimeOp::PrintI64],
    );

    assert_eq!(left, right);
    assert_eq!(left.canonical_bytes(), right.canonical_bytes());
    assert_eq!(left.identity(), right.identity());
}

#[test]
fn current_contract_identity_is_sha256_of_canonical_bytes() {
    let manifest = RuntimeAbiManifest::current();
    let expected: [u8; 32] = Sha256::digest(manifest.canonical_bytes()).into();

    assert_eq!(manifest.identity().as_bytes(), &expected);
    assert_eq!(
        manifest.identity().to_string(),
        "42c5afa981c0e6cafd7e5f903d08c4cd4e04fe7231db644f76f68bfc48d5b573",
        "the canonical runtime contract changed; review the ABI diff and bump its semantic version before accepting a new identity",
    );
}

#[test]
fn current_operation_catalogs_are_complete_and_collision_free() {
    let manifest = RuntimeAbiManifest::current();
    assert_eq!(manifest.primitive_ops(), PrimitiveOp::ALL);
    assert_eq!(manifest.runtime_ops(), RuntimeOp::ALL);

    let symbols = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.symbol())
        .collect::<BTreeSet<_>>();
    assert_eq!(symbols.len(), RuntimeOp::ALL.len());
}

#[test]
fn implementation_hash_uses_sha256_and_hex_display() {
    assert_eq!(
        ImplementationHash::sha256(b"").to_string(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn semantic_contract_dimensions_change_only_the_contract_hash() {
    let baseline = manifest([PrimitiveOp::BoolNot], [RuntimeOp::IntDiv]);
    let schema = RuntimeAbiManifest::new(
        gors_runtime_abi::ManifestSchemaVersion::new(2),
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
        [RuntimeOp::IntDiv, RuntimeOp::IntRem],
    );

    for changed in [schema, version, semantics, primitive, runtime] {
        assert_ne!(baseline.identity(), changed.identity());
    }
}

#[test]
fn target_and_implementation_change_artifact_but_not_contract_identity()
-> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::IntDiv]);
    let first = RuntimeArtifactManifest::new(
        &contract,
        target("wasm32-unknown-unknown", [])?,
        ImplementationHash::sha256(b"implementation one"),
    )?;
    let second = RuntimeArtifactManifest::new(
        &contract,
        target("x86_64-unknown-linux-gnu", [TargetCapability::Threads])?,
        ImplementationHash::sha256(b"implementation two"),
    )?;

    assert_eq!(first.contract(), contract.identity());
    assert_eq!(second.contract(), contract.identity());
    assert_eq!(first.schema(), CURRENT_ARTIFACT_SCHEMA);
    assert_ne!(first.identity(), second.identity());
    Ok(())
}

#[test]
fn runtime_signatures_are_exact_and_typed() {
    assert_eq!(
        RuntimeOp::GoStringFromBytes.signature().parameters(),
        [RuntimeType::ByteSlice]
    );
    assert_eq!(
        RuntimeOp::GoStringFromStatic.signature().parameters(),
        [RuntimeType::StaticByteSlice]
    );
    assert_eq!(
        RuntimeOp::ConcatGoStrings.signature().parameters(),
        [RuntimeType::GoString, RuntimeType::GoString]
    );
    assert_eq!(
        RuntimeOp::ConcatGoStrings.signature().result(),
        RuntimeType::GoString
    );
    assert_eq!(RuntimeOp::IntDiv.symbol(), "int_div");
    assert_eq!(RuntimeOp::PrintGoString.symbol(), "print_go_string");
}

#[test]
fn artifact_selection_enforces_runtime_capability_requirements() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintI64]);
    let error = RuntimeArtifactManifest::new(
        &contract,
        target("wasm32-unknown-unknown", [])?,
        ImplementationHash::sha256(b"runtime"),
    );

    assert!(matches!(
        error,
        Err(error)
            if error.operation() == RuntimeOp::PrintI64
                && error.capability() == TargetCapability::StandardIo
    ));
    assert!(
        RuntimeArtifactManifest::new(
            &contract,
            target("wasm32-unknown-unknown", [TargetCapability::StandardIo])?,
            ImplementationHash::sha256(b"runtime"),
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn artifact_capabilities_are_canonicalized() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintBool]);
    let left = RuntimeArtifactManifest::new(
        &contract,
        target(
            "wasm32-unknown-unknown",
            [
                TargetCapability::StandardIo,
                TargetCapability::Atomics32,
                TargetCapability::StandardIo,
            ],
        )?,
        ImplementationHash::sha256(b"runtime"),
    )?;
    let right = RuntimeArtifactManifest::new(
        &contract,
        target(
            "wasm32-unknown-unknown",
            [TargetCapability::Atomics32, TargetCapability::StandardIo],
        )?,
        ImplementationHash::sha256(b"runtime"),
    )?;

    assert_eq!(left, right);
    assert_eq!(left.canonical_bytes(), right.canonical_bytes());
    assert_eq!(left.identity(), right.identity());
    Ok(())
}

#[test]
fn invalid_target_triples_are_rejected() {
    assert!(target("wasm32-unknown-unknown", []).is_ok());
    assert_eq!(target("", []), Err(TargetModelError::EmptyTriple));
    assert_eq!(
        target("x86_64 unknown linux gnu", []),
        Err(TargetModelError::InvalidTripleCharacter)
    );
}
