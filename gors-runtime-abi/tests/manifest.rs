use std::collections::BTreeSet;
use std::error::Error;

use sha2::{Digest as _, Sha256};

use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA,
    CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA, CompatibilityIdentity, ContractVersion,
    DataWidth, Endianness, GoPanicCondition, GoSemanticModel, HostIoEffect, ImplementationHash,
    PrimitiveOp, RuntimeAbiManifest, RuntimeArtifactFormat, RuntimeArtifactManifest,
    RuntimeDependency, RuntimeLinkError, RuntimeLinkRequest, RuntimeOp, RuntimeRequirement,
    RuntimeType, RustRlibCompatibility, TargetCapabilities, TargetCapability, TargetModel,
    TargetModelError,
};

fn target_model(triple: &str) -> Result<TargetModel, TargetModelError> {
    TargetModel::new(triple, DataWidth::Bits32, Endianness::Little)
}

fn compatibility_identity(label: &[u8]) -> CompatibilityIdentity {
    CompatibilityIdentity::sha256(label)
}

fn artifact(
    contract: &RuntimeAbiManifest,
    target: TargetModel,
    capabilities: impl IntoIterator<Item = TargetCapability>,
    toolchain: CompatibilityIdentity,
    implementation: &[u8],
) -> RuntimeArtifactManifest {
    RuntimeArtifactManifest::new(
        contract.identity(),
        target,
        TargetCapabilities::new(capabilities),
        RuntimeArtifactFormat::RustRlibV1,
        toolchain,
        ImplementationHash::sha256(implementation),
    )
}

fn request(
    contract: &RuntimeAbiManifest,
    requirement: impl IntoIterator<Item = RuntimeOp>,
    target: TargetModel,
    toolchain: CompatibilityIdentity,
) -> Result<RuntimeLinkRequest, Box<dyn Error>> {
    let dependency = RuntimeDependency::new(contract, RuntimeRequirement::new(requirement))?;
    Ok(RuntimeLinkRequest::new(
        dependency,
        target,
        RuntimeArtifactFormat::RustRlibV1,
        toolchain,
    ))
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
    assert_eq!(manifest.contract(), ContractVersion::new(2, 2, 0));
    assert_eq!(manifest.identity().as_bytes(), &expected);
    assert_eq!(
        manifest.identity().to_string(),
        "3e5ebadfa17dd752b7dd068ddb349df05d6e21b107d597a1533372e8dc545ed4",
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
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Set => AllocationEffect::MayAllocate,
            RuntimeOp::GoStringFromStatic
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64KeyAt => AllocationEffect::None,
        };
        let expected_argument_mutation = match operation {
            RuntimeOp::ConcatGoStrings
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Set
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear => ArgumentMutationEffect::MayMutateOwnedArgument,
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
            | RuntimeOp::PrintGoString
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64KeyAt => ArgumentMutationEffect::None,
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
            | RuntimeOp::IntShr
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringI64Set
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64KeyAt => HostIoEffect::None,
        };
        let expected_panics: &[GoPanicCondition] = match operation {
            RuntimeOp::IntDiv | RuntimeOp::IntRem => &[GoPanicCondition::IntegerDivideByZero],
            RuntimeOp::IntShl | RuntimeOp::IntShr => &[GoPanicCondition::NegativeShiftAmount],
            RuntimeOp::PanicBool | RuntimeOp::PanicI64 | RuntimeOp::PanicGoString => {
                &[GoPanicCondition::ExplicitPanic]
            }
            RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoMapStringI64KeyAt => &[GoPanicCondition::IndexOutOfRange],
            RuntimeOp::GoSliceI64Range | RuntimeOp::GoSliceI64Make => {
                &[GoPanicCondition::SliceBoundsOutOfRange]
            }
            RuntimeOp::GoMapStringI64Set => &[GoPanicCondition::NilMapAssignment],
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapStringI64IsNil => &[],
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
            PrimitiveOp::FloatNeg => (&[RuntimeType::F64], RuntimeType::F64),
            PrimitiveOp::FloatAdd
            | PrimitiveOp::FloatSub
            | PrimitiveOp::FloatMul
            | PrimitiveOp::FloatDiv => (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::F64),
            PrimitiveOp::FloatEqual
            | PrimitiveOp::FloatNotEqual
            | PrimitiveOp::FloatLess
            | PrimitiveOp::FloatLessEqual
            | PrimitiveOp::FloatGreater
            | PrimitiveOp::FloatGreaterEqual => {
                (&[RuntimeType::F64, RuntimeType::F64], RuntimeType::Bool)
            }
            PrimitiveOp::ComplexNeg => (&[RuntimeType::Complex128], RuntimeType::Complex128),
            PrimitiveOp::ComplexAdd
            | PrimitiveOp::ComplexSub
            | PrimitiveOp::ComplexMul
            | PrimitiveOp::ComplexDiv => (
                &[RuntimeType::Complex128, RuntimeType::Complex128],
                RuntimeType::Complex128,
            ),
            PrimitiveOp::ComplexEqual | PrimitiveOp::ComplexNotEqual => (
                &[RuntimeType::Complex128, RuntimeType::Complex128],
                RuntimeType::Bool,
            ),
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
            RuntimeOp::PanicBool => (&[RuntimeType::Bool], RuntimeType::Unit),
            RuntimeOp::PanicI64 => (&[RuntimeType::I64], RuntimeType::Unit),
            RuntimeOp::PanicGoString => (&[RuntimeType::GoString], RuntimeType::Unit),
            RuntimeOp::GoSliceI64FromStatic => {
                (&[RuntimeType::StaticI64Slice], RuntimeType::GoSliceI64)
            }
            RuntimeOp::GoSliceI64Index => (
                &[RuntimeType::GoSliceI64, RuntimeType::I64],
                RuntimeType::I64,
            ),
            RuntimeOp::GoSliceI64Range => (
                &[
                    RuntimeType::GoSliceI64,
                    RuntimeType::I64,
                    RuntimeType::I64,
                    RuntimeType::I64,
                ],
                RuntimeType::GoSliceI64,
            ),
            RuntimeOp::GoSliceI64Set => (
                &[RuntimeType::GoSliceI64, RuntimeType::I64, RuntimeType::I64],
                RuntimeType::Unit,
            ),
            RuntimeOp::GoSliceI64Make => (
                &[RuntimeType::I64, RuntimeType::I64],
                RuntimeType::GoSliceI64,
            ),
            RuntimeOp::GoSliceI64Len | RuntimeOp::GoSliceI64Cap => {
                (&[RuntimeType::GoSliceI64], RuntimeType::I64)
            }
            RuntimeOp::GoSliceI64Append => (
                &[RuntimeType::GoSliceI64, RuntimeType::I64],
                RuntimeType::GoSliceI64,
            ),
            RuntimeOp::GoSliceU8FromStatic => {
                (&[RuntimeType::StaticByteSlice], RuntimeType::GoSliceU8)
            }
            RuntimeOp::GoSliceU8AppendSlice => (
                &[RuntimeType::GoSliceU8, RuntimeType::GoSliceU8],
                RuntimeType::GoSliceU8,
            ),
            RuntimeOp::GoSliceU8AppendString => (
                &[RuntimeType::GoSliceU8, RuntimeType::GoString],
                RuntimeType::GoSliceU8,
            ),
            RuntimeOp::GoSliceU8CopyString => (
                &[RuntimeType::GoSliceU8, RuntimeType::GoString],
                RuntimeType::I64,
            ),
            RuntimeOp::GoSliceI64Clear => (&[RuntimeType::GoSliceI64], RuntimeType::Unit),
            RuntimeOp::GoStringFromSliceU8 => (&[RuntimeType::GoSliceU8], RuntimeType::GoString),
            RuntimeOp::GoSliceI64Copy => (
                &[RuntimeType::GoSliceI64, RuntimeType::GoSliceI64],
                RuntimeType::I64,
            ),
            RuntimeOp::GoMapStringI64Nil | RuntimeOp::GoMapStringI64Make => {
                (&[], RuntimeType::GoMapStringI64)
            }
            RuntimeOp::GoMapStringI64Len => (&[RuntimeType::GoMapStringI64], RuntimeType::I64),
            RuntimeOp::GoMapStringI64Get => (
                &[RuntimeType::GoMapStringI64, RuntimeType::GoString],
                RuntimeType::I64,
            ),
            RuntimeOp::GoMapStringI64Contains => (
                &[RuntimeType::GoMapStringI64, RuntimeType::GoString],
                RuntimeType::Bool,
            ),
            RuntimeOp::GoMapStringI64Set => (
                &[
                    RuntimeType::GoMapStringI64,
                    RuntimeType::GoString,
                    RuntimeType::I64,
                ],
                RuntimeType::Unit,
            ),
            RuntimeOp::GoMapStringI64Delete => (
                &[RuntimeType::GoMapStringI64, RuntimeType::GoString],
                RuntimeType::Unit,
            ),
            RuntimeOp::GoMapStringI64Clear => (&[RuntimeType::GoMapStringI64], RuntimeType::Unit),
            RuntimeOp::GoMapStringI64IsNil => (&[RuntimeType::GoMapStringI64], RuntimeType::Bool),
            RuntimeOp::GoMapStringI64KeyAt => (
                &[RuntimeType::GoMapStringI64, RuntimeType::I64],
                RuntimeType::GoString,
            ),
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
fn runtime_requirement_wire_ids_round_trip_and_reject_unknown_values() -> Result<(), Box<dyn Error>>
{
    let requirement = RuntimeRequirement::new([
        RuntimeOp::PrintNewline,
        RuntimeOp::IntDiv,
        RuntimeOp::PrintNewline,
    ]);
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
    let contract = manifest([], [RuntimeOp::IntDiv, RuntimeOp::PrintI64]);
    let target = target_model("wasm32-unknown-unknown")?;
    let toolchain = compatibility_identity(b"rustc");
    let provider = artifact(&contract, target.clone(), [], toolchain, b"runtime");

    let pure = provider.select(request(
        &contract,
        [RuntimeOp::IntDiv],
        target.clone(),
        toolchain,
    )?)?;
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
    let contract = manifest([], [RuntimeOp::IntDiv]);
    let Err(error) =
        RuntimeDependency::new(&contract, RuntimeRequirement::new([RuntimeOp::PrintI64]))
    else {
        return Err("a consumer selected an operation absent from its contract".into());
    };
    assert_eq!(error.operation(), RuntimeOp::PrintI64);
    Ok(())
}

#[test]
fn link_validation_order_is_schema_contract_target_then_toolchain() -> Result<(), Box<dyn Error>> {
    let contract = manifest([], [RuntimeOp::PrintI64]);
    let other_contract = manifest([], [RuntimeOp::IntDiv]);
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

#[test]
fn artifact_and_link_plan_identities_cover_every_selection_dimension() -> Result<(), Box<dyn Error>>
{
    let contract = manifest([], [RuntimeOp::IntDiv, RuntimeOp::PrintI64]);
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let toolchain = compatibility_identity(b"rustc");
    let baseline = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        toolchain,
        b"runtime",
    );
    let changed_capabilities = artifact(&contract, target.clone(), [], toolchain, b"runtime");
    let changed_toolchain = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        compatibility_identity(b"other rustc"),
        b"runtime",
    );
    let changed_implementation = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        toolchain,
        b"other runtime",
    );
    assert_ne!(baseline.identity(), changed_capabilities.identity());
    assert_ne!(baseline.identity(), changed_toolchain.identity());
    assert_ne!(baseline.identity(), changed_implementation.identity());

    let div = baseline.select(request(
        &contract,
        [RuntimeOp::IntDiv],
        target.clone(),
        toolchain,
    )?)?;
    let reordered = baseline.select(request(
        &contract,
        [RuntimeOp::IntDiv, RuntimeOp::IntDiv],
        target.clone(),
        toolchain,
    )?)?;
    let print = baseline.select(request(
        &contract,
        [RuntimeOp::PrintI64],
        target,
        toolchain,
    )?)?;
    assert_eq!(div.identity(), reordered.identity());
    assert_ne!(div.identity(), print.identity());
    Ok(())
}

#[test]
fn invalid_target_triples_are_rejected() {
    assert!(target_model("wasm32-unknown-unknown").is_ok());
    assert_eq!(target_model(""), Err(TargetModelError::EmptyTriple));
    assert_eq!(
        target_model("x86_64 unknown linux gnu"),
        Err(TargetModelError::InvalidTripleCharacter)
    );
}

#[test]
fn rust_rlib_compatibility_identity_is_path_free_and_covers_exact_producer_facts()
-> Result<(), Box<dyn Error>> {
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let rustc_version = b"rustc 1.96.0\nhost: x86_64-unknown-linux-gnu\n";
    let target_libdir = b"canonical-target-libdir-v1";
    let baseline =
        RustRlibCompatibility::new(rustc_version, target_libdir.as_slice(), target.clone())?;
    let same = RustRlibCompatibility::new(
        b"rustc 1.96.0\nhost: aarch64-unknown-linux-gnu\n",
        target_libdir.as_slice(),
        target,
    )?;
    let other_rustc = RustRlibCompatibility::new(
        b"rustc 1.96.1\nhost: x86_64-unknown-linux-gnu\n",
        target_libdir.as_slice(),
        target_model("x86_64-unknown-linux-gnu")?,
    )?;
    let other_target = RustRlibCompatibility::new(
        rustc_version,
        target_libdir.as_slice(),
        target_model("i686-unknown-linux-musl")?,
    )?;
    let other_sysroot = RustRlibCompatibility::new(
        rustc_version,
        b"different-target-libdir".as_slice(),
        target_model("x86_64-unknown-linux-gnu")?,
    )?;

    assert_eq!(baseline.canonical_bytes(), same.canonical_bytes());
    assert_eq!(baseline.identity(), same.identity());
    assert_ne!(baseline.identity(), other_rustc.identity());
    assert_ne!(baseline.identity(), other_target.identity());
    assert_ne!(baseline.identity(), other_sysroot.identity());
    assert!(
        !baseline
            .canonical_bytes()
            .windows(7)
            .any(|bytes| bytes == b"/Users/")
    );
    assert_eq!(
        RustRlibCompatibility::new(
            [],
            target_libdir.as_slice(),
            target_model("x86_64-unknown-linux-gnu")?
        ),
        Err(gors_runtime_abi::RustRlibRecordError::EmptyRustcVerboseVersion)
    );
    Ok(())
}
