//! Capability and effect metadata for runtime operations.

use super::{
    CLOSE_CHANNEL_PANICS, EXPLICIT_PANIC, INDEX_OUT_OF_RANGE, INTEGER_DIVIDE_BY_ZERO,
    IntegerRuntimeOp, NEGATIVE_CHANNEL_CAPACITY, NEGATIVE_SHIFT_AMOUNT, NIL_MAP_ASSIGNMENT,
    NIL_POINTER_DEREFERENCE, NIL_POINTER_OR_INDEX_OUT_OF_RANGE, NO_CAPABILITIES, NO_GO_PANICS,
    RuntimeOp, SEND_ON_CLOSED_CHANNEL, SLICE_BOUNDS_OUT_OF_RANGE, STANDARD_IO_CAPABILITY,
    TYPE_ASSERTION_FAILURE, TYPE_ASSERTION_OR_INDEX_OUT_OF_RANGE,
    UNCOMPARABLE_INTERFACE_COMPARISON,
};
use crate::effects::{
    AllocationEffect, ArgumentMutationEffect, BlockingEffect, HostIoEffect, RuntimeEffects,
};
use crate::target::TargetCapability;

impl RuntimeOp {
    /// Host facilities required to invoke this operation.
    #[must_use]
    pub const fn required_capabilities(self) -> &'static [TargetCapability] {
        match self {
            Self::PrintBool
            | Self::PrintI64
            | Self::PrintU64
            | Self::PrintF64
            | Self::PrintF32
            | Self::PrintSpace
            | Self::PrintNewline
            | Self::PrintGoString => STANDARD_IO_CAPABILITY,
            Self::GoStringFromBytes
            | Self::GoStringFromRune
            | Self::GoStringFromStatic
            | Self::ConcatGoStrings
            | Self::Integer { .. }
            | Self::PanicBool
            | Self::PanicI64
            | Self::PanicGoString
            | Self::PanicGoInterface
            | Self::GoPanicPayloadToInterface
            | Self::GoInterfaceIsRuntimeError
            | Self::GoSliceI64FromStatic
            | Self::GoSliceI64Index
            | Self::GoSliceI64Range
            | Self::GoSliceI64Set
            | Self::GoSliceI64Make
            | Self::GoSliceI64Len
            | Self::GoSliceI64Cap
            | Self::GoSliceI64Append
            | Self::GoSliceI64AppendSlice
            | Self::GoSliceU8FromStatic
            | Self::GoSliceU8AppendSlice
            | Self::GoSliceU8AppendString
            | Self::GoSliceU8CopyString
            | Self::GoSliceI64Clear
            | Self::GoStringFromSliceU8
            | Self::GoSliceI64Copy
            | Self::GoMapStringI64Nil
            | Self::GoMapStringI64Make
            | Self::GoMapStringI64Len
            | Self::GoMapStringI64Get
            | Self::GoMapStringI64Contains
            | Self::GoMapStringI64Set
            | Self::GoMapStringI64Delete
            | Self::GoMapStringI64Clear
            | Self::GoMapStringI64IsNil
            | Self::GoMapStringI64KeyAt
            | Self::GoMapStringI64RangeKeys
            | Self::GoMapI64GoStringNil
            | Self::GoMapI64GoStringMake
            | Self::GoMapI64GoStringLen
            | Self::GoMapI64GoStringGet
            | Self::GoMapI64GoStringContains
            | Self::GoMapI64GoStringSet
            | Self::GoMapI64GoStringDelete
            | Self::GoMapI64GoStringClear
            | Self::GoMapI64GoStringIsNil
            | Self::GoMapI64GoStringRangeKeys
            | Self::GoPointerI64Nil
            | Self::GoPointerI64New
            | Self::GoPointerI64Get
            | Self::GoPointerI64Set
            | Self::GoPointerI64IsNil
            | Self::GoPointerI64Equal
            | Self::GoPointerStructI64Nil
            | Self::GoPointerStructI64New
            | Self::GoPointerStructI64Get
            | Self::GoPointerStructI64Set
            | Self::GoPointerStructI64IsNil
            | Self::GoPointerStructI64Equal
            | Self::GoInterfaceNil
            | Self::GoInterfaceBoxBool
            | Self::GoInterfaceBoxI64
            | Self::GoInterfaceBoxF64
            | Self::GoInterfaceBoxGoString
            | Self::GoInterfaceBoxStructI64
            | Self::GoInterfaceBoxPointerI64
            | Self::GoInterfaceBoxPointerStructI64
            | Self::GoInterfaceIsNil
            | Self::GoInterfaceIsType
            | Self::GoInterfaceUnboxBool
            | Self::GoInterfaceUnboxI64
            | Self::GoInterfaceUnboxF64
            | Self::GoInterfaceUnboxGoString
            | Self::GoInterfaceStructI64Get
            | Self::GoInterfaceUnboxPointerI64
            | Self::GoInterfaceUnboxPointerStructI64
            | Self::GoSliceBoolFromStatic
            | Self::GoSliceBoolIndex
            | Self::GoSliceBoolSet
            | Self::GoSliceInterfaceMake
            | Self::GoSliceInterfaceLen
            | Self::GoSliceInterfaceIndex
            | Self::GoSliceInterfaceSet
            | Self::GoSliceInterfaceAppend
            | Self::GoMapStringInterfaceMake
            | Self::GoMapStringInterfaceLen
            | Self::GoMapStringInterfaceGet
            | Self::GoMapStringInterfaceContains
            | Self::GoMapStringInterfaceSet
            | Self::GoSliceU8Len
            | Self::GoSliceU8Index
            | Self::GoSliceU8Range
            | Self::GoStringIndex
            | Self::GoStringRange
            | Self::GoStringFromSliceRunes
            | Self::GoStringRangeCount
            | Self::GoStringRangeIndexAt
            | Self::GoStringRangeRuneAt
            | Self::GoStringToSliceRunes
            | Self::GoSliceI64Nil
            | Self::GoSliceI64IsNil
            | Self::GoSliceU8Nil
            | Self::GoSliceU8IsNil
            | Self::GoSliceBoolNil
            | Self::GoSliceBoolIsNil
            | Self::GoSliceInterfaceNil
            | Self::GoSliceInterfaceIsNil
            | Self::GoInterfaceBoxAggregate
            | Self::GoInterfaceBoxComparableAggregate
            | Self::GoInterfaceUnboxAggregate
            | Self::GoInterfaceEqual
            | Self::GoSliceU8Make
            | Self::GoSliceU8Set
            | Self::GoSliceU8Copy
            | Self::GoChannelI64Nil
            | Self::GoChannelI64Make
            | Self::GoChannelI64Len
            | Self::GoChannelI64Cap
            | Self::GoChannelI64Send
            | Self::GoChannelI64ReceiveValue
            | Self::GoChannelI64Receive
            | Self::GoChannelI64Close
            | Self::GoChannelI64IsNil
            | Self::GoStringLen
            | Self::GoChannelI64TrySend
            | Self::GoChannelI64TryReceive
            | Self::GoChannelGoStringNil
            | Self::GoChannelGoStringMake
            | Self::GoChannelGoStringLen
            | Self::GoChannelGoStringCap
            | Self::GoChannelGoStringSend
            | Self::GoChannelGoStringReceiveValue
            | Self::GoChannelGoStringReceive
            | Self::GoChannelGoStringClose
            | Self::GoChannelGoStringIsNil
            | Self::GoChannelGoStringTrySend
            | Self::GoChannelGoStringTryReceive
            | Self::GoChannelGoChannelI64Nil
            | Self::GoChannelGoChannelI64Make
            | Self::GoChannelGoChannelI64Len
            | Self::GoChannelGoChannelI64Cap
            | Self::GoChannelGoChannelI64Send
            | Self::GoChannelGoChannelI64ReceiveValue
            | Self::GoChannelGoChannelI64Receive
            | Self::GoChannelGoChannelI64Close
            | Self::GoChannelGoChannelI64IsNil
            | Self::GoChannelGoChannelI64TrySend
            | Self::GoChannelGoChannelI64TryReceive => NO_CAPABILITIES,
            Self::GoSliceGoStringNil
            | Self::GoSliceGoStringMake
            | Self::GoSliceGoStringLen
            | Self::GoSliceGoStringCap
            | Self::GoSliceGoStringIndex
            | Self::GoSliceGoStringRange
            | Self::GoSliceGoStringSet
            | Self::GoSliceGoStringAppend
            | Self::GoSliceGoStringCopy
            | Self::GoSliceGoStringClear
            | Self::GoSliceGoStringIsNil
            | Self::GoInterfaceBoxGoSliceGoString
            | Self::GoInterfaceUnboxGoSliceGoString => NO_CAPABILITIES,
        }
    }

    /// Allocation, host-I/O, and Go-panic behavior of this operation.
    #[must_use]
    pub const fn effects(self) -> RuntimeEffects {
        match self {
            Self::GoStringFromBytes | Self::GoStringFromRune | Self::GoPanicPayloadToInterface => {
                RuntimeEffects::new(
                    AllocationEffect::MayAllocate,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    NO_GO_PANICS,
                )
            }
            Self::ConcatGoStrings => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::Integer {
                op: IntegerRuntimeOp::Div | IntegerRuntimeOp::Rem,
                ..
            } => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INTEGER_DIVIDE_BY_ZERO,
            ),
            Self::Integer {
                op: IntegerRuntimeOp::ShlSigned | IntegerRuntimeOp::ShrSigned,
                ..
            } => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NEGATIVE_SHIFT_AMOUNT,
            ),
            Self::Integer {
                op: IntegerRuntimeOp::ShlUnsigned | IntegerRuntimeOp::ShrUnsigned,
                ..
            } => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::PrintBool
            | Self::PrintI64
            | Self::PrintU64
            | Self::PrintSpace
            | Self::PrintNewline
            | Self::PrintGoString => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::StandardError,
                NO_GO_PANICS,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::PrintF64 | Self::PrintF32 => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::StandardError,
                NO_GO_PANICS,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::GoStringFromStatic => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::PanicBool | Self::PanicI64 | Self::PanicGoString | Self::PanicGoInterface => {
                RuntimeEffects::new(
                    AllocationEffect::None,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    EXPLICIT_PANIC,
                )
            }
            Self::GoInterfaceIsRuntimeError => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64FromStatic | Self::GoSliceBoolFromStatic => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64Nil
            | Self::GoSliceU8Nil
            | Self::GoSliceBoolNil
            | Self::GoSliceInterfaceNil
            | Self::GoSliceGoStringNil => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64IsNil
            | Self::GoSliceU8IsNil
            | Self::GoSliceBoolIsNil
            | Self::GoSliceInterfaceIsNil
            | Self::GoSliceGoStringIsNil => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoStringFromSliceRunes | Self::GoStringToSliceRunes => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64Index
            | Self::GoSliceU8Index
            | Self::GoSliceBoolIndex
            | Self::GoSliceInterfaceIndex
            | Self::GoSliceGoStringIndex
            | Self::GoStringIndex => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Range
            | Self::GoSliceU8Range
            | Self::GoSliceGoStringRange
            | Self::GoStringRange => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                SLICE_BOUNDS_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Set
            | Self::GoSliceU8Set
            | Self::GoSliceBoolSet
            | Self::GoSliceInterfaceSet
            | Self::GoSliceGoStringSet => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Make
            | Self::GoSliceU8Make
            | Self::GoSliceInterfaceMake
            | Self::GoSliceGoStringMake => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                SLICE_BOUNDS_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Len
            | Self::GoSliceI64Cap
            | Self::GoSliceU8Len
            | Self::GoSliceInterfaceLen
            | Self::GoSliceGoStringLen
            | Self::GoSliceGoStringCap
            | Self::GoStringRangeCount => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64Append
            | Self::GoSliceI64AppendSlice
            | Self::GoSliceInterfaceAppend
            | Self::GoSliceGoStringAppend => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceU8FromStatic | Self::GoStringFromSliceU8 => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceU8AppendSlice | Self::GoSliceU8AppendString => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceU8CopyString
            | Self::GoSliceU8Copy
            | Self::GoSliceI64Clear
            | Self::GoSliceI64Copy
            | Self::GoSliceGoStringCopy
            | Self::GoSliceGoStringClear => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoMapStringI64Nil
            | Self::GoMapStringI64Len
            | Self::GoMapStringI64Get
            | Self::GoMapStringI64Contains
            | Self::GoMapStringI64IsNil
            | Self::GoMapI64GoStringNil
            | Self::GoMapI64GoStringLen
            | Self::GoMapI64GoStringGet
            | Self::GoMapI64GoStringContains
            | Self::GoMapI64GoStringIsNil
            | Self::GoMapStringInterfaceLen
            | Self::GoMapStringInterfaceGet
            | Self::GoMapStringInterfaceContains
            | Self::GoPointerI64Nil
            | Self::GoPointerI64IsNil
            | Self::GoPointerI64Equal
            | Self::GoPointerStructI64Nil
            | Self::GoPointerStructI64IsNil
            | Self::GoPointerStructI64Equal => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoMapStringI64Make
            | Self::GoMapI64GoStringMake
            | Self::GoMapStringInterfaceMake
            | Self::GoPointerI64New => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoMapStringI64Set | Self::GoMapI64GoStringSet | Self::GoMapStringInterfaceSet => {
                RuntimeEffects::new(
                    AllocationEffect::MayAllocate,
                    ArgumentMutationEffect::MayMutateOwnedArgument,
                    HostIoEffect::None,
                    NIL_MAP_ASSIGNMENT,
                )
            }
            Self::GoMapStringI64Delete
            | Self::GoMapStringI64Clear
            | Self::GoMapI64GoStringDelete
            | Self::GoMapI64GoStringClear => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoMapStringI64KeyAt => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoMapStringI64RangeKeys | Self::GoMapI64GoStringRangeKeys => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoStringRangeIndexAt | Self::GoStringRangeRuneAt => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoPointerI64Get => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NIL_POINTER_DEREFERENCE,
            ),
            Self::GoPointerI64Set => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NIL_POINTER_DEREFERENCE,
            ),
            Self::GoPointerStructI64New => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoPointerStructI64Get => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NIL_POINTER_OR_INDEX_OUT_OF_RANGE,
            ),
            Self::GoPointerStructI64Set => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NIL_POINTER_OR_INDEX_OUT_OF_RANGE,
            ),
            Self::GoInterfaceNil | Self::GoInterfaceIsNil | Self::GoInterfaceIsType => {
                RuntimeEffects::new(
                    AllocationEffect::None,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    NO_GO_PANICS,
                )
            }
            Self::GoInterfaceBoxBool
            | Self::GoInterfaceBoxI64
            | Self::GoInterfaceBoxF64
            | Self::GoInterfaceBoxGoString
            | Self::GoInterfaceBoxPointerI64
            | Self::GoInterfaceBoxPointerStructI64
            | Self::GoInterfaceBoxAggregate
            | Self::GoInterfaceBoxComparableAggregate => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoInterfaceBoxGoSliceGoString => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoInterfaceBoxStructI64 => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoInterfaceUnboxBool
            | Self::GoInterfaceUnboxI64
            | Self::GoInterfaceUnboxF64
            | Self::GoInterfaceUnboxGoString
            | Self::GoInterfaceUnboxPointerI64
            | Self::GoInterfaceUnboxPointerStructI64
            | Self::GoInterfaceUnboxAggregate => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                TYPE_ASSERTION_FAILURE,
            ),
            Self::GoInterfaceUnboxGoSliceGoString => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                TYPE_ASSERTION_FAILURE,
            ),
            Self::GoInterfaceEqual => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                UNCOMPARABLE_INTERFACE_COMPARISON,
            ),
            Self::GoInterfaceStructI64Get => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                TYPE_ASSERTION_OR_INDEX_OUT_OF_RANGE,
            ),
            Self::GoChannelI64Nil
            | Self::GoChannelI64Len
            | Self::GoChannelI64Cap
            | Self::GoChannelI64IsNil
            | Self::GoChannelGoStringNil
            | Self::GoChannelGoStringLen
            | Self::GoChannelGoStringCap
            | Self::GoChannelGoStringIsNil
            | Self::GoChannelGoChannelI64Nil
            | Self::GoChannelGoChannelI64Len
            | Self::GoChannelGoChannelI64Cap
            | Self::GoChannelGoChannelI64IsNil
            | Self::GoStringLen => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoChannelI64Make
            | Self::GoChannelGoStringMake
            | Self::GoChannelGoChannelI64Make => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NEGATIVE_CHANNEL_CAPACITY,
            ),
            Self::GoChannelI64Send
            | Self::GoChannelGoStringSend
            | Self::GoChannelGoChannelI64Send => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                SEND_ON_CLOSED_CHANNEL,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::GoChannelI64ReceiveValue
            | Self::GoChannelI64Receive
            | Self::GoChannelGoStringReceiveValue
            | Self::GoChannelGoStringReceive
            | Self::GoChannelGoChannelI64ReceiveValue
            | Self::GoChannelGoChannelI64Receive => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::GoChannelI64Close
            | Self::GoChannelGoStringClose
            | Self::GoChannelGoChannelI64Close => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                CLOSE_CHANNEL_PANICS,
            ),
            Self::GoChannelI64TrySend
            | Self::GoChannelGoStringTrySend
            | Self::GoChannelGoChannelI64TrySend => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                SEND_ON_CLOSED_CHANNEL,
            ),
            Self::GoChannelI64TryReceive
            | Self::GoChannelGoStringTryReceive
            | Self::GoChannelGoChannelI64TryReceive => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
        }
    }
}
