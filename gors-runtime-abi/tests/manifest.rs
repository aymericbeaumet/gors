use std::error::Error;

use sha2::{Digest as _, Sha256};

use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, ArtifactSchemaVersion, BlockingEffect,
    CURRENT_ARTIFACT_SCHEMA, CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA,
    CompatibilityIdentity, ContractVersion, DataWidth, Endianness, GoPanicCondition,
    GoSemanticModel, HostIoEffect, ImplementationHash, IntegerKind, IntegerPrimitive,
    IntegerRuntimeOp, PrimitiveOp, RuntimeAbiManifest, RuntimeArtifactFormat,
    RuntimeArtifactManifest, RuntimeDependency, RuntimeLinkError, RuntimeLinkRequest, RuntimeOp,
    RuntimeRequirement, TargetCapabilities, TargetCapability, TargetModel, TargetModelError,
};

const INT_DIV: RuntimeOp = RuntimeOp::Integer {
    op: IntegerRuntimeOp::Div,
    kind: IntegerKind::I64,
};
const INT_REM: RuntimeOp = RuntimeOp::Integer {
    op: IntegerRuntimeOp::Rem,
    kind: IntegerKind::I64,
};
const INT_SHL: RuntimeOp = RuntimeOp::Integer {
    op: IntegerRuntimeOp::ShlSigned,
    kind: IntegerKind::I64,
};
const INT_SHR: RuntimeOp = RuntimeOp::Integer {
    op: IntegerRuntimeOp::ShrSigned,
    kind: IntegerKind::I64,
};

#[path = "manifest/append_effects.rs"]
mod append_effects;
#[path = "manifest/catalog.rs"]
mod catalog;
#[path = "manifest/channel_effects.rs"]
mod channel_effects;
#[path = "manifest/contract_and_artifact.rs"]
mod contract_and_artifact;
#[path = "manifest/float_catalog.rs"]
mod float_catalog;
#[path = "manifest/link_identity.rs"]
mod link_identity;
#[path = "manifest/link_validation.rs"]
mod link_validation;
#[path = "manifest/string_slice_effects.rs"]
mod string_slice_effects;

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
    let integer_equal = PrimitiveOp::Integer {
        op: IntegerPrimitive::Equal,
        kind: IntegerKind::I64,
    };
    let left = manifest(
        [integer_equal, PrimitiveOp::BoolNot, integer_equal],
        [RuntimeOp::PrintI64, INT_DIV, RuntimeOp::PrintI64],
    );
    let right = manifest(
        [PrimitiveOp::BoolNot, integer_equal],
        [INT_DIV, RuntimeOp::PrintI64],
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
    assert_eq!(manifest.contract(), ContractVersion::new(2, 31, 0));
    assert_eq!(manifest.identity().as_bytes(), &expected);
    assert_eq!(
        manifest.identity().to_string(),
        "e885c47239d38dd2942e64f40df07e6a1d4bda18c5e57f7fd9740cd6b547e351",
        "the canonical runtime contract changed; review the ABI diff and bump its semantic version before accepting a new identity",
    );
}

#[test]
fn runtime_effect_metadata_is_complete_and_exact() {
    for operation in RuntimeOp::ALL {
        let effects = operation.effects();
        let expected_allocation = match operation {
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoPanicPayloadToInterface
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::PrintF64
            | RuntimeOp::PrintF32
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceBoolFromStatic
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceU8Make
            | RuntimeOp::GoSliceInterfaceMake
            | RuntimeOp::GoSliceI64Nil
            | RuntimeOp::GoSliceU8Nil
            | RuntimeOp::GoSliceBoolNil
            | RuntimeOp::GoSliceInterfaceNil
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceI64AppendSlice
            | RuntimeOp::GoSliceInterfaceAppend
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
            | RuntimeOp::GoStringToSliceRunes
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Set
            | RuntimeOp::GoMapStringI64RangeKeys
            | RuntimeOp::GoMapI64GoStringMake
            | RuntimeOp::GoMapI64GoStringSet
            | RuntimeOp::GoMapI64GoStringRangeKeys
            | RuntimeOp::GoMapStringInterfaceMake
            | RuntimeOp::GoMapStringInterfaceSet
            | RuntimeOp::GoPointerI64New
            | RuntimeOp::GoPointerStructI64New
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoChannelI64Make => AllocationEffect::MayAllocate,
            RuntimeOp::GoStringFromStatic
            | RuntimeOp::Integer { .. }
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::PanicGoInterface
            | RuntimeOp::GoInterfaceIsRuntimeError
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64IsNil
            | RuntimeOp::GoSliceU8IsNil
            | RuntimeOp::GoSliceBoolIsNil
            | RuntimeOp::GoSliceInterfaceIsNil
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceBoolIndex
            | RuntimeOp::GoSliceBoolSet
            | RuntimeOp::GoSliceInterfaceLen
            | RuntimeOp::GoSliceInterfaceIndex
            | RuntimeOp::GoSliceInterfaceSet
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Set
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64KeyAt
            | RuntimeOp::GoMapI64GoStringNil
            | RuntimeOp::GoMapI64GoStringLen
            | RuntimeOp::GoMapI64GoStringGet
            | RuntimeOp::GoMapI64GoStringContains
            | RuntimeOp::GoMapI64GoStringDelete
            | RuntimeOp::GoMapI64GoStringClear
            | RuntimeOp::GoMapI64GoStringIsNil
            | RuntimeOp::GoMapStringInterfaceLen
            | RuntimeOp::GoMapStringInterfaceGet
            | RuntimeOp::GoMapStringInterfaceContains
            | RuntimeOp::GoPointerI64Nil
            | RuntimeOp::GoPointerI64Get
            | RuntimeOp::GoPointerI64Set
            | RuntimeOp::GoPointerI64IsNil
            | RuntimeOp::GoPointerI64Equal
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64Set
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF32
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxPointerI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF32
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
            | RuntimeOp::GoInterfaceUnboxPointerI64
            | RuntimeOp::GoInterfaceUnboxPointerStructI64
            | RuntimeOp::GoInterfaceUnboxAggregate
            | RuntimeOp::GoInterfaceEqual
            | RuntimeOp::GoChannelI64Nil
            | RuntimeOp::GoChannelI64Len
            | RuntimeOp::GoChannelI64Cap
            | RuntimeOp::GoChannelI64Send
            | RuntimeOp::GoChannelI64ReceiveValue
            | RuntimeOp::GoChannelI64Receive
            | RuntimeOp::GoChannelI64Close
            | RuntimeOp::GoChannelI64IsNil
            | RuntimeOp::GoStringLen
            | RuntimeOp::GoSliceU8Len
            | RuntimeOp::GoSliceU8Index
            | RuntimeOp::GoSliceU8Range
            | RuntimeOp::GoStringIndex
            | RuntimeOp::GoStringRange
            | RuntimeOp::GoStringRangeCount
            | RuntimeOp::GoStringRangeIndexAt
            | RuntimeOp::GoStringRangeRuneAt
            | RuntimeOp::GoChannelI64TrySend
            | RuntimeOp::GoChannelI64TryReceive => AllocationEffect::None,
            operation if string_slice_effects::is_string_slice_operation(*operation) => {
                string_slice_effects::allocation(*operation)
            }
            operation => channel_effects::allocation(*operation),
        };
        let expected_argument_mutation = match operation {
            RuntimeOp::ConcatGoStrings
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceBoolSet
            | RuntimeOp::GoSliceInterfaceSet
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceI64AppendSlice
            | RuntimeOp::GoSliceInterfaceAppend
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Set
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Set
            | RuntimeOp::GoMapI64GoStringSet
            | RuntimeOp::GoMapStringInterfaceSet
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapI64GoStringDelete
            | RuntimeOp::GoMapI64GoStringClear
            | RuntimeOp::GoPointerI64Set
            | RuntimeOp::GoPointerStructI64Set
            | RuntimeOp::GoChannelI64Send
            | RuntimeOp::GoChannelI64ReceiveValue
            | RuntimeOp::GoChannelI64Receive
            | RuntimeOp::GoChannelI64Close
            | RuntimeOp::GoChannelI64TrySend
            | RuntimeOp::GoChannelI64TryReceive => ArgumentMutationEffect::MayMutateOwnedArgument,
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::Integer { .. }
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
            | RuntimeOp::PrintF32
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::PanicGoInterface
            | RuntimeOp::GoPanicPayloadToInterface
            | RuntimeOp::GoInterfaceIsRuntimeError
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Nil
            | RuntimeOp::GoSliceI64IsNil
            | RuntimeOp::GoSliceU8Nil
            | RuntimeOp::GoSliceU8IsNil
            | RuntimeOp::GoSliceBoolNil
            | RuntimeOp::GoSliceBoolIsNil
            | RuntimeOp::GoSliceInterfaceNil
            | RuntimeOp::GoSliceInterfaceIsNil
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceBoolFromStatic
            | RuntimeOp::GoSliceBoolIndex
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceU8Make
            | RuntimeOp::GoSliceInterfaceMake
            | RuntimeOp::GoSliceInterfaceLen
            | RuntimeOp::GoSliceInterfaceIndex
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
            | RuntimeOp::GoStringToSliceRunes
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringInterfaceMake
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringInterfaceLen
            | RuntimeOp::GoMapStringInterfaceGet
            | RuntimeOp::GoMapStringInterfaceContains
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64KeyAt
            | RuntimeOp::GoMapStringI64RangeKeys
            | RuntimeOp::GoMapI64GoStringNil
            | RuntimeOp::GoMapI64GoStringMake
            | RuntimeOp::GoMapI64GoStringLen
            | RuntimeOp::GoMapI64GoStringGet
            | RuntimeOp::GoMapI64GoStringContains
            | RuntimeOp::GoMapI64GoStringIsNil
            | RuntimeOp::GoMapI64GoStringRangeKeys
            | RuntimeOp::GoPointerI64Nil
            | RuntimeOp::GoPointerI64New
            | RuntimeOp::GoPointerI64Get
            | RuntimeOp::GoPointerI64IsNil
            | RuntimeOp::GoPointerI64Equal
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64New
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF32
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoInterfaceBoxPointerI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF32
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
            | RuntimeOp::GoInterfaceUnboxPointerI64
            | RuntimeOp::GoInterfaceUnboxPointerStructI64
            | RuntimeOp::GoInterfaceUnboxAggregate
            | RuntimeOp::GoInterfaceEqual
            | RuntimeOp::GoChannelI64Nil
            | RuntimeOp::GoChannelI64Make
            | RuntimeOp::GoChannelI64Len
            | RuntimeOp::GoChannelI64Cap
            | RuntimeOp::GoChannelI64IsNil
            | RuntimeOp::GoStringLen
            | RuntimeOp::GoSliceU8Len
            | RuntimeOp::GoSliceU8Index
            | RuntimeOp::GoSliceU8Range
            | RuntimeOp::GoStringIndex
            | RuntimeOp::GoStringRange
            | RuntimeOp::GoStringRangeCount
            | RuntimeOp::GoStringRangeIndexAt
            | RuntimeOp::GoStringRangeRuneAt => ArgumentMutationEffect::None,
            operation if string_slice_effects::is_string_slice_operation(*operation) => {
                string_slice_effects::argument_mutation(*operation)
            }
            operation => channel_effects::argument_mutation(*operation),
        };
        let expected_blocking = match operation {
            RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
            | RuntimeOp::PrintF32
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::GoChannelI64Send
            | RuntimeOp::GoChannelI64ReceiveValue
            | RuntimeOp::GoChannelI64Receive => BlockingEffect::MayBlock,
            operation if channel_effects::is_extended_channel_operation(*operation) => {
                channel_effects::blocking(*operation)
            }
            _ => BlockingEffect::None,
        };
        let expected_host_io = match operation {
            RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
            | RuntimeOp::PrintF32
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => HostIoEffect::StandardError,
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::Integer { .. }
            | RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::PanicGoInterface
            | RuntimeOp::GoPanicPayloadToInterface
            | RuntimeOp::GoInterfaceIsRuntimeError
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Nil
            | RuntimeOp::GoSliceI64IsNil
            | RuntimeOp::GoSliceU8Nil
            | RuntimeOp::GoSliceU8IsNil
            | RuntimeOp::GoSliceBoolNil
            | RuntimeOp::GoSliceBoolIsNil
            | RuntimeOp::GoSliceInterfaceNil
            | RuntimeOp::GoSliceInterfaceIsNil
            | RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceBoolFromStatic
            | RuntimeOp::GoSliceBoolIndex
            | RuntimeOp::GoSliceBoolSet
            | RuntimeOp::GoSliceInterfaceMake
            | RuntimeOp::GoSliceInterfaceLen
            | RuntimeOp::GoSliceInterfaceIndex
            | RuntimeOp::GoSliceInterfaceSet
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceU8Make
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceI64AppendSlice
            | RuntimeOp::GoSliceInterfaceAppend
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Set
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
            | RuntimeOp::GoStringToSliceRunes
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
            | RuntimeOp::GoMapStringI64KeyAt
            | RuntimeOp::GoMapStringInterfaceMake
            | RuntimeOp::GoMapStringInterfaceLen
            | RuntimeOp::GoMapStringInterfaceGet
            | RuntimeOp::GoMapStringInterfaceContains
            | RuntimeOp::GoMapStringInterfaceSet
            | RuntimeOp::GoPointerI64Nil
            | RuntimeOp::GoPointerI64New
            | RuntimeOp::GoPointerI64Get
            | RuntimeOp::GoPointerI64Set
            | RuntimeOp::GoPointerI64IsNil
            | RuntimeOp::GoPointerI64Equal
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64New
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64Set
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF32
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoInterfaceBoxPointerI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF32
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
            | RuntimeOp::GoInterfaceUnboxPointerI64
            | RuntimeOp::GoInterfaceUnboxPointerStructI64
            | RuntimeOp::GoInterfaceUnboxAggregate
            | RuntimeOp::GoInterfaceEqual
            | RuntimeOp::GoChannelI64Nil
            | RuntimeOp::GoChannelI64Make
            | RuntimeOp::GoChannelI64Len
            | RuntimeOp::GoChannelI64Cap
            | RuntimeOp::GoChannelI64Send
            | RuntimeOp::GoChannelI64ReceiveValue
            | RuntimeOp::GoChannelI64Receive
            | RuntimeOp::GoChannelI64Close
            | RuntimeOp::GoChannelI64IsNil
            | RuntimeOp::GoStringLen
            | RuntimeOp::GoSliceU8Len
            | RuntimeOp::GoSliceU8Index
            | RuntimeOp::GoSliceU8Range
            | RuntimeOp::GoStringIndex
            | RuntimeOp::GoStringRange
            | RuntimeOp::GoStringRangeCount
            | RuntimeOp::GoStringRangeIndexAt
            | RuntimeOp::GoStringRangeRuneAt
            | RuntimeOp::GoChannelI64TrySend
            | RuntimeOp::GoChannelI64TryReceive => HostIoEffect::None,
            operation if channel_effects::is_extended_channel_operation(*operation) => {
                channel_effects::host_io(*operation)
            }
            _ => HostIoEffect::None,
        };
        let expected_panics: &[GoPanicCondition] = match operation {
            RuntimeOp::Integer {
                op: IntegerRuntimeOp::Div | IntegerRuntimeOp::Rem,
                ..
            } => &[GoPanicCondition::IntegerDivideByZero],
            RuntimeOp::Integer {
                op: IntegerRuntimeOp::ShlSigned | IntegerRuntimeOp::ShrSigned,
                ..
            } => &[GoPanicCondition::NegativeShiftAmount],
            RuntimeOp::PanicBool
            | RuntimeOp::PanicI64
            | RuntimeOp::PanicGoString
            | RuntimeOp::PanicGoInterface => &[GoPanicCondition::ExplicitPanic],
            RuntimeOp::GoSliceI64Index
            | RuntimeOp::GoSliceU8Index
            | RuntimeOp::GoStringIndex
            | RuntimeOp::GoSliceI64Set
            | RuntimeOp::GoSliceU8Set
            | RuntimeOp::GoSliceBoolIndex
            | RuntimeOp::GoSliceBoolSet
            | RuntimeOp::GoSliceInterfaceIndex
            | RuntimeOp::GoSliceInterfaceSet
            | RuntimeOp::GoMapStringI64KeyAt
            | RuntimeOp::GoStringRangeIndexAt
            | RuntimeOp::GoStringRangeRuneAt => &[GoPanicCondition::IndexOutOfRange],
            RuntimeOp::GoSliceI64Range
            | RuntimeOp::GoSliceU8Range
            | RuntimeOp::GoStringRange
            | RuntimeOp::GoSliceI64Make
            | RuntimeOp::GoSliceU8Make
            | RuntimeOp::GoSliceInterfaceMake => &[GoPanicCondition::SliceBoundsOutOfRange],
            RuntimeOp::GoMapStringI64Set
            | RuntimeOp::GoMapI64GoStringSet
            | RuntimeOp::GoMapStringInterfaceSet => &[GoPanicCondition::NilMapAssignment],
            RuntimeOp::GoPointerI64Get | RuntimeOp::GoPointerI64Set => {
                &[GoPanicCondition::NilPointerDereference]
            }
            RuntimeOp::GoPointerStructI64New => &[GoPanicCondition::IndexOutOfRange],
            RuntimeOp::GoPointerStructI64Get | RuntimeOp::GoPointerStructI64Set => &[
                GoPanicCondition::NilPointerDereference,
                GoPanicCondition::IndexOutOfRange,
            ],
            RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF32
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceUnboxPointerI64
            | RuntimeOp::GoInterfaceUnboxPointerStructI64
            | RuntimeOp::GoInterfaceUnboxAggregate => &[GoPanicCondition::TypeAssertionFailure],
            RuntimeOp::GoInterfaceEqual => &[GoPanicCondition::UncomparableInterfaceComparison],
            RuntimeOp::GoInterfaceStructI64Get => &[
                GoPanicCondition::IndexOutOfRange,
                GoPanicCondition::TypeAssertionFailure,
            ],
            RuntimeOp::GoChannelI64Make => &[GoPanicCondition::NegativeChannelCapacity],
            RuntimeOp::GoChannelI64Send | RuntimeOp::GoChannelI64TrySend => {
                &[GoPanicCondition::SendOnClosedChannel]
            }
            RuntimeOp::GoChannelI64Close => &[
                GoPanicCondition::CloseOfNilChannel,
                GoPanicCondition::CloseOfClosedChannel,
            ],
            RuntimeOp::Integer {
                op: IntegerRuntimeOp::ShlUnsigned | IntegerRuntimeOp::ShrUnsigned,
                ..
            } => &[],
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
            | RuntimeOp::PrintF32
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString
            | RuntimeOp::GoPanicPayloadToInterface
            | RuntimeOp::GoInterfaceIsRuntimeError
            | RuntimeOp::GoSliceI64FromStatic
            | RuntimeOp::GoSliceI64Nil
            | RuntimeOp::GoSliceI64IsNil
            | RuntimeOp::GoSliceU8Nil
            | RuntimeOp::GoSliceU8IsNil
            | RuntimeOp::GoSliceBoolNil
            | RuntimeOp::GoSliceBoolIsNil
            | RuntimeOp::GoSliceInterfaceNil
            | RuntimeOp::GoSliceInterfaceIsNil
            | RuntimeOp::GoSliceBoolFromStatic
            | RuntimeOp::GoSliceI64Len
            | RuntimeOp::GoSliceI64Cap
            | RuntimeOp::GoSliceI64Append
            | RuntimeOp::GoSliceI64AppendSlice
            | RuntimeOp::GoSliceInterfaceAppend
            | RuntimeOp::GoSliceInterfaceLen
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
            | RuntimeOp::GoStringToSliceRunes
            | RuntimeOp::GoSliceI64Copy
            | RuntimeOp::GoMapStringI64Nil
            | RuntimeOp::GoMapStringI64Make
            | RuntimeOp::GoMapStringI64Len
            | RuntimeOp::GoMapStringI64Get
            | RuntimeOp::GoMapStringI64Contains
            | RuntimeOp::GoMapStringInterfaceMake
            | RuntimeOp::GoMapStringInterfaceLen
            | RuntimeOp::GoMapStringInterfaceGet
            | RuntimeOp::GoMapStringInterfaceContains
            | RuntimeOp::GoMapStringI64Delete
            | RuntimeOp::GoMapStringI64Clear
            | RuntimeOp::GoMapStringI64IsNil
            | RuntimeOp::GoMapStringI64RangeKeys
            | RuntimeOp::GoMapI64GoStringNil
            | RuntimeOp::GoMapI64GoStringMake
            | RuntimeOp::GoMapI64GoStringLen
            | RuntimeOp::GoMapI64GoStringGet
            | RuntimeOp::GoMapI64GoStringContains
            | RuntimeOp::GoMapI64GoStringDelete
            | RuntimeOp::GoMapI64GoStringClear
            | RuntimeOp::GoMapI64GoStringIsNil
            | RuntimeOp::GoMapI64GoStringRangeKeys
            | RuntimeOp::GoPointerI64Nil
            | RuntimeOp::GoPointerI64New
            | RuntimeOp::GoPointerI64IsNil
            | RuntimeOp::GoPointerI64Equal
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF32
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoInterfaceBoxPointerI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoChannelI64Nil
            | RuntimeOp::GoChannelI64Len
            | RuntimeOp::GoChannelI64Cap
            | RuntimeOp::GoChannelI64ReceiveValue
            | RuntimeOp::GoChannelI64Receive
            | RuntimeOp::GoChannelI64IsNil
            | RuntimeOp::GoStringLen
            | RuntimeOp::GoStringRangeCount
            | RuntimeOp::GoSliceU8Len
            | RuntimeOp::GoChannelI64TryReceive => &[],
            operation if string_slice_effects::is_string_slice_operation(*operation) => {
                string_slice_effects::panics(*operation)
            }
            operation => channel_effects::panics(*operation),
        };

        assert_eq!(effects.allocation(), expected_allocation, "{operation:?}");
        assert_eq!(
            effects.argument_mutation(),
            expected_argument_mutation,
            "{operation:?}"
        );
        assert_eq!(effects.blocking(), expected_blocking, "{operation:?}");
        assert_eq!(effects.host_io(), expected_host_io, "{operation:?}");
        assert_eq!(effects.go_panics(), expected_panics, "{operation:?}");
    }
}
