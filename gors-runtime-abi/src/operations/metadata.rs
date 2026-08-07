//! Capability and effect metadata for runtime operations.

use super::{
    CLOSE_CHANNEL_PANICS, EXPLICIT_PANIC, INDEX_OUT_OF_RANGE, INTEGER_DIVIDE_BY_ZERO,
    NEGATIVE_CHANNEL_CAPACITY, NEGATIVE_SHIFT_AMOUNT, NIL_MAP_ASSIGNMENT, NIL_POINTER_DEREFERENCE,
    NIL_POINTER_OR_INDEX_OUT_OF_RANGE, NO_CAPABILITIES, NO_GO_PANICS, RuntimeOp,
    SEND_ON_CLOSED_CHANNEL, SLICE_BOUNDS_OUT_OF_RANGE, STANDARD_IO_CAPABILITY,
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
            | Self::PrintF64
            | Self::PrintSpace
            | Self::PrintNewline
            | Self::PrintGoString => STANDARD_IO_CAPABILITY,
            Self::GoStringFromBytes
            | Self::GoStringFromRune
            | Self::GoStringFromStatic
            | Self::ConcatGoStrings
            | Self::IntDiv
            | Self::IntRem
            | Self::IntShl
            | Self::IntShr
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
            | Self::GoPointerI64Nil
            | Self::GoPointerI64New
            | Self::GoPointerI64Get
            | Self::GoPointerI64Set
            | Self::GoPointerI64IsNil
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
            | Self::GoInterfaceBoxPointerStructI64
            | Self::GoInterfaceIsNil
            | Self::GoInterfaceIsType
            | Self::GoInterfaceUnboxBool
            | Self::GoInterfaceUnboxI64
            | Self::GoInterfaceUnboxF64
            | Self::GoInterfaceUnboxGoString
            | Self::GoInterfaceStructI64Get
            | Self::GoInterfaceUnboxPointerStructI64
            | Self::GoSliceBoolFromStatic
            | Self::GoSliceBoolIndex
            | Self::GoSliceBoolSet
            | Self::GoSliceInterfaceMake
            | Self::GoSliceInterfaceLen
            | Self::GoSliceInterfaceIndex
            | Self::GoSliceInterfaceSet
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
            | Self::GoChannelI64TryReceive => NO_CAPABILITIES,
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
            Self::IntDiv | Self::IntRem => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INTEGER_DIVIDE_BY_ZERO,
            ),
            Self::IntShl | Self::IntShr => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NEGATIVE_SHIFT_AMOUNT,
            ),
            Self::PrintBool
            | Self::PrintI64
            | Self::PrintSpace
            | Self::PrintNewline
            | Self::PrintGoString => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::StandardError,
                NO_GO_PANICS,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::PrintF64 => RuntimeEffects::new(
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
            | Self::GoSliceInterfaceNil => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64IsNil
            | Self::GoSliceU8IsNil
            | Self::GoSliceBoolIsNil
            | Self::GoSliceInterfaceIsNil => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoStringFromSliceRunes => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64Index
            | Self::GoSliceU8Index
            | Self::GoSliceBoolIndex
            | Self::GoSliceInterfaceIndex
            | Self::GoStringIndex => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Range | Self::GoSliceU8Range | Self::GoStringRange => {
                RuntimeEffects::new(
                    AllocationEffect::None,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    SLICE_BOUNDS_OUT_OF_RANGE,
                )
            }
            Self::GoSliceI64Set
            | Self::GoSliceU8Set
            | Self::GoSliceBoolSet
            | Self::GoSliceInterfaceSet => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                INDEX_OUT_OF_RANGE,
            ),
            Self::GoSliceI64Make | Self::GoSliceU8Make | Self::GoSliceInterfaceMake => {
                RuntimeEffects::new(
                    AllocationEffect::MayAllocate,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    SLICE_BOUNDS_OUT_OF_RANGE,
                )
            }
            Self::GoSliceI64Len
            | Self::GoSliceI64Cap
            | Self::GoSliceU8Len
            | Self::GoSliceInterfaceLen
            | Self::GoStringRangeCount => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoSliceI64Append => RuntimeEffects::new(
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
            | Self::GoSliceI64Copy => RuntimeEffects::new(
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
            | Self::GoMapStringInterfaceLen
            | Self::GoMapStringInterfaceGet
            | Self::GoMapStringInterfaceContains
            | Self::GoPointerI64Nil
            | Self::GoPointerI64IsNil
            | Self::GoPointerStructI64Nil
            | Self::GoPointerStructI64IsNil
            | Self::GoPointerStructI64Equal => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoMapStringI64Make | Self::GoMapStringInterfaceMake | Self::GoPointerI64New => {
                RuntimeEffects::new(
                    AllocationEffect::MayAllocate,
                    ArgumentMutationEffect::None,
                    HostIoEffect::None,
                    NO_GO_PANICS,
                )
            }
            Self::GoMapStringI64Set | Self::GoMapStringInterfaceSet => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NIL_MAP_ASSIGNMENT,
            ),
            Self::GoMapStringI64Delete | Self::GoMapStringI64Clear => RuntimeEffects::new(
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
            | Self::GoInterfaceBoxPointerStructI64
            | Self::GoInterfaceBoxAggregate
            | Self::GoInterfaceBoxComparableAggregate => RuntimeEffects::new(
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
            | Self::GoInterfaceUnboxPointerStructI64
            | Self::GoInterfaceUnboxAggregate => RuntimeEffects::new(
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
            | Self::GoStringLen => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::GoChannelI64Make => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NEGATIVE_CHANNEL_CAPACITY,
            ),
            Self::GoChannelI64Send => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                SEND_ON_CLOSED_CHANNEL,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::GoChannelI64ReceiveValue | Self::GoChannelI64Receive => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            )
            .with_blocking(BlockingEffect::MayBlock),
            Self::GoChannelI64Close => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                CLOSE_CHANNEL_PANICS,
            ),
            Self::GoChannelI64TrySend => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                SEND_ON_CLOSED_CHANNEL,
            ),
            Self::GoChannelI64TryReceive => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::MayMutateOwnedArgument,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
        }
    }
}
