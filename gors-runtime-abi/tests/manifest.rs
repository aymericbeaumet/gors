use std::error::Error;

use sha2::{Digest as _, Sha256};

use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, ArtifactSchemaVersion, BlockingEffect,
    CURRENT_ARTIFACT_SCHEMA, CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA,
    CompatibilityIdentity, ContractVersion, DataWidth, Endianness, GoPanicCondition,
    GoSemanticModel, HostIoEffect, ImplementationHash, IntegerKind, IntegerPrimitive, PrimitiveOp,
    RuntimeAbiManifest, RuntimeArtifactFormat, RuntimeArtifactManifest, RuntimeDependency,
    RuntimeLinkError, RuntimeLinkRequest, RuntimeOp, RuntimeRequirement, TargetCapabilities,
    TargetCapability, TargetModel, TargetModelError,
};

#[path = "manifest/catalog.rs"]
mod catalog;
#[path = "manifest/channel_effects.rs"]
mod channel_effects;
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
        [RuntimeOp::PrintI64, RuntimeOp::IntDiv, RuntimeOp::PrintI64],
    );
    let right = manifest(
        [PrimitiveOp::BoolNot, integer_equal],
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
    assert_eq!(manifest.contract(), ContractVersion::new(2, 24, 0));
    assert_eq!(manifest.identity().as_bytes(), &expected);
    assert_eq!(
        manifest.identity().to_string(),
        "7f2e66d6085c7d0dff9daca318651200bfba66b8644fa022e4ec12c16f5b93c7",
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
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
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
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
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
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64Set
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
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
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
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
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64New
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
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
            | RuntimeOp::PrintSpace
            | RuntimeOp::PrintNewline
            | RuntimeOp::PrintGoString => HostIoEffect::StandardError,
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::IntDiv
            | RuntimeOp::IntRem
            | RuntimeOp::IntShl
            | RuntimeOp::IntShr
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
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Set
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
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
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64New
            | RuntimeOp::GoPointerStructI64Get
            | RuntimeOp::GoPointerStructI64Set
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
            | RuntimeOp::GoInterfaceBoxPointerStructI64
            | RuntimeOp::GoInterfaceBoxAggregate
            | RuntimeOp::GoInterfaceBoxComparableAggregate
            | RuntimeOp::GoInterfaceIsNil
            | RuntimeOp::GoInterfaceIsType
            | RuntimeOp::GoInterfaceUnboxBool
            | RuntimeOp::GoInterfaceUnboxI64
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
            | RuntimeOp::GoInterfaceStructI64Get
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
            RuntimeOp::IntDiv | RuntimeOp::IntRem => &[GoPanicCondition::IntegerDivideByZero],
            RuntimeOp::IntShl | RuntimeOp::IntShr => &[GoPanicCondition::NegativeShiftAmount],
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
            | RuntimeOp::GoInterfaceUnboxF64
            | RuntimeOp::GoInterfaceUnboxGoString
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
            RuntimeOp::GoStringFromBytes
            | RuntimeOp::GoStringFromRune
            | RuntimeOp::GoStringFromStatic
            | RuntimeOp::ConcatGoStrings
            | RuntimeOp::PrintBool
            | RuntimeOp::PrintI64
            | RuntimeOp::PrintU64
            | RuntimeOp::PrintF64
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
            | RuntimeOp::GoSliceInterfaceLen
            | RuntimeOp::GoSliceU8FromStatic
            | RuntimeOp::GoSliceU8AppendSlice
            | RuntimeOp::GoSliceU8AppendString
            | RuntimeOp::GoSliceU8CopyString
            | RuntimeOp::GoSliceU8Copy
            | RuntimeOp::GoSliceI64Clear
            | RuntimeOp::GoStringFromSliceU8
            | RuntimeOp::GoStringFromSliceRunes
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
            | RuntimeOp::GoPointerStructI64Nil
            | RuntimeOp::GoPointerStructI64IsNil
            | RuntimeOp::GoPointerStructI64Equal
            | RuntimeOp::GoInterfaceNil
            | RuntimeOp::GoInterfaceBoxBool
            | RuntimeOp::GoInterfaceBoxI64
            | RuntimeOp::GoInterfaceBoxF64
            | RuntimeOp::GoInterfaceBoxGoString
            | RuntimeOp::GoInterfaceBoxStructI64
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
