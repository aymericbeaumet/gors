use std::collections::BTreeSet;
use std::error::Error;

use sha2::{Digest as _, Sha256};

use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, CURRENT_ARTIFACT_SCHEMA, CURRENT_CONTRACT_VERSION,
    CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, Endianness, GoPanicCondition,
    GoSemanticModel, HostIoEffect, ImplementationHash, PrimitiveOp, RuntimeAbiManifest,
    RuntimeArtifactManifest, RuntimeOp, RuntimeRequirement, RuntimeType, TargetCapabilities,
    TargetCapability, TargetModel, TargetModelError,
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

    assert_eq!(manifest.schema().get(), 2);
    assert_eq!(manifest.contract(), CURRENT_CONTRACT_VERSION);
    assert_eq!(manifest.contract(), ContractVersion::new(2, 0, 0));
    assert_eq!(manifest.identity().as_bytes(), &expected);
    assert_eq!(
        manifest.identity().to_string(),
        "bf9f363d574bdbac76a3220a787c4fbae3ea1cb3e55b78091a946aeaf0acb10f",
        "the canonical runtime contract changed; review the ABI diff and bump its semantic version before accepting a new identity",
    );
}

#[test]
fn current_operation_catalogs_are_complete_and_collision_free() {
    let current = RuntimeAbiManifest::current();
    assert_eq!(current.primitive_ops(), PrimitiveOp::ALL);
    assert_eq!(current.runtime_ops(), RuntimeOp::ALL);

    let symbols = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.symbol())
        .collect::<BTreeSet<_>>();
    assert_eq!(symbols.len(), RuntimeOp::ALL.len());

    let primitive_ids = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_ids.len(), PrimitiveOp::ALL.len());

    let primitive_names = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.name())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_names.len(), PrimitiveOp::ALL.len());

    let runtime_ids = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(runtime_ids.len(), RuntimeOp::ALL.len());

    let primitive_identities = PrimitiveOp::ALL
        .iter()
        .map(|operation| manifest([*operation], []).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_identities.len(), PrimitiveOp::ALL.len());

    let operation_identities = RuntimeOp::ALL
        .iter()
        .map(|operation| manifest([], [*operation]).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(operation_identities.len(), RuntimeOp::ALL.len());
}

#[test]
fn runtime_effect_metadata_is_complete_and_exact() {
    for operation in RuntimeOp::ALL {
        let effects = operation.effects();
        let expected_allocation = match operation {
            RuntimeOp::GoStringFromBytes | RuntimeOp::ConcatGoStrings => {
                AllocationEffect::MayAllocate
            }
            RuntimeOp::GoStringFromStatic
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => AllocationEffect::None,
        };
        let expected_argument_mutation = match operation {
            RuntimeOp::ConcatGoStrings => ArgumentMutationEffect::MayMutateOwnedArgument,
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => ArgumentMutationEffect::None,
        };
        let expected_host_io = match operation {
            RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => HostIoEffect::StandardError,
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr => HostIoEffect::None,
        };
        let expected_panics: &[GoPanicCondition] = match operation {
            RuntimeOp::IntDiv | RuntimeOp::IntRem => &[GoPanicCondition::IntegerDivideByZero],
            RuntimeOp::IntShl | RuntimeOp::IntShr => &[GoPanicCondition::NegativeShiftAmount],
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => &[],
        };

        assert_eq!(effects.allocation(), expected_allocation, "{operation:?}");
        assert_eq!(
            effects.argument_mutation(),
            expected_argument_mutation,
            "{operation:?}"
        );
        assert_eq!(effects.host_io(), expected_host_io, "{operation:?}");
        assert_eq!(effects.go_panics(), expected_panics, "{operation:?}");
    }
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
fn primitive_signatures_are_complete_and_exact() {
    for operation in PrimitiveOp::ALL {
        let expected: (&[RuntimeType], RuntimeType) = match operation {
            PrimitiveOp::BoolNot => (&[RuntimeType::Bool], RuntimeType::Bool),
            PrimitiveOp::BoolEqual | PrimitiveOp::BoolNotEqual => {
                (&[RuntimeType::Bool, RuntimeType::Bool], RuntimeType::Bool)
            }
            PrimitiveOp::IntBitNot | PrimitiveOp::IntWrappingNeg => {
                (&[RuntimeType::I64], RuntimeType::I64)
            }
            PrimitiveOp::IntBitAnd
            | PrimitiveOp::IntBitOr
            | PrimitiveOp::IntBitXor
            | PrimitiveOp::IntAndNot
            | PrimitiveOp::IntWrappingAdd
            | PrimitiveOp::IntWrappingSub
            | PrimitiveOp::IntWrappingMul => {
                (&[RuntimeType::I64, RuntimeType::I64], RuntimeType::I64)
            }
            PrimitiveOp::IntEqual
            | PrimitiveOp::IntNotEqual
            | PrimitiveOp::IntLess
            | PrimitiveOp::IntLessEqual
            | PrimitiveOp::IntGreater
            | PrimitiveOp::IntGreaterEqual => {
                (&[RuntimeType::I64, RuntimeType::I64], RuntimeType::Bool)
            }
            PrimitiveOp::StringEqual
            | PrimitiveOp::StringNotEqual
            | PrimitiveOp::StringLess
            | PrimitiveOp::StringLessEqual
            | PrimitiveOp::StringGreater
            | PrimitiveOp::StringGreaterEqual => (
                &[RuntimeType::GoString, RuntimeType::GoString],
                RuntimeType::Bool,
            ),
        };

        assert_eq!(
            operation.signature().parameters(),
            expected.0,
            "{operation:?}"
        );
        assert_eq!(operation.signature().result(), expected.1, "{operation:?}");
    }
}

#[test]
fn runtime_signatures_are_complete_and_exact() {
    for operation in RuntimeOp::ALL {
        let expected: (&[RuntimeType], RuntimeType) = match operation {
            RuntimeOp::GoStringFromBytes => (&[RuntimeType::ByteSlice], RuntimeType::GoString),
            RuntimeOp::GoStringFromStatic => {
                (&[RuntimeType::StaticByteSlice], RuntimeType::GoString)
            }
            RuntimeOp::ConcatGoStrings => (
                &[RuntimeType::GoString, RuntimeType::GoString],
                RuntimeType::GoString,
            ),
            RuntimeOp::IntDiv | RuntimeOp::IntRem | RuntimeOp::IntShl | RuntimeOp::IntShr => {
                (&[RuntimeType::I64, RuntimeType::I64], RuntimeType::I64)
            }
            RuntimeOp::PrintBool => (&[RuntimeType::Bool], RuntimeType::Unit),
            RuntimeOp::PrintI64 => (&[RuntimeType::I64], RuntimeType::Unit),
            RuntimeOp::PrintSpace | RuntimeOp::PrintNewline => (&[], RuntimeType::Unit),
            RuntimeOp::PrintGoString => (&[RuntimeType::GoString], RuntimeType::Unit),
        };

        assert_eq!(
            operation.signature().parameters(),
            expected.0,
            "{operation:?}"
        );
        assert_eq!(operation.signature().result(), expected.1, "{operation:?}");
    }

    assert_eq!(RuntimeOp::IntDiv.symbol(), "int_div");
    assert_eq!(RuntimeOp::PrintGoString.symbol(), "print_go_string");
}

#[test]
fn runtime_requirements_are_canonical_and_composable() {
    let left = RuntimeRequirement::new([
        RuntimeOp::PrintI64,
        RuntimeOp::ConcatGoStrings,
        RuntimeOp::IntDiv,
        RuntimeOp::PrintI64,
    ]);
    let reordered = RuntimeRequirement::new([
        RuntimeOp::IntDiv,
        RuntimeOp::PrintI64,
        RuntimeOp::ConcatGoStrings,
    ]);

    assert_eq!(left, reordered);
    assert_eq!(
        left.as_slice(),
        [
            RuntimeOp::ConcatGoStrings,
            RuntimeOp::IntDiv,
            RuntimeOp::PrintI64
        ]
    );
    assert_eq!(left.iter().collect::<Vec<_>>(), left.as_slice());
    assert_eq!((&left).into_iter().collect::<Vec<_>>(), left.as_slice());
    assert!(left.contains(RuntimeOp::IntDiv));
    assert!(!left.contains(RuntimeOp::IntRem));
    assert_eq!(left.len(), 3);
    assert!(!left.is_empty());

    let union = left.union(&RuntimeRequirement::new([
        RuntimeOp::IntDiv,
        RuntimeOp::PrintNewline,
    ]));
    assert_eq!(
        union.as_slice(),
        [
            RuntimeOp::ConcatGoStrings,
            RuntimeOp::IntDiv,
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
