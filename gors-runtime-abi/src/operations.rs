//! Stable native and runtime operation catalogs.

mod decode;
mod float;
mod identity;
mod integer;
mod metadata;
mod primitive;
mod runtime_catalog;
mod runtime_encoding;
mod runtime_op;
mod signature_types;
mod symbols;
mod value_model;
mod value_types;

use signature_types::*;
use value_model::{
    CLOSE_CHANNEL_PANICS, EXPLICIT_PANIC, INDEX_OUT_OF_RANGE, INTEGER_DIVIDE_BY_ZERO,
    NEGATIVE_CHANNEL_CAPACITY, NEGATIVE_SHIFT_AMOUNT, NIL_MAP_ASSIGNMENT, NIL_POINTER_DEREFERENCE,
    NIL_POINTER_OR_INDEX_OUT_OF_RANGE, NO_CAPABILITIES, NO_GO_PANICS, SEND_ON_CLOSED_CHANNEL,
    SLICE_BOUNDS_OUT_OF_RANGE, STANDARD_IO_CAPABILITY, TYPE_ASSERTION_FAILURE,
    TYPE_ASSERTION_OR_INDEX_OUT_OF_RANGE, UNCOMPARABLE_INTERFACE_COMPARISON,
};

pub use float::{FloatKind, FloatPrimitive};
pub use identity::{RuntimeOpId, UnknownRuntimeOpId};
pub use integer::{IntegerKind, IntegerKindConstraint, IntegerPrimitive, IntegerRuntimeOp};
pub use primitive::{PrimitiveOp, PrimitiveOpId};
pub use runtime_op::RuntimeOp;
pub use value_model::{RuntimeSignature, RuntimeType};

impl RuntimeOp {
    /// Complete helper catalog for the current contract.
    pub const ALL: &'static [Self] = runtime_catalog::ALL;

    /// Semantic Go integer constraint for one physical `I64` parameter slot.
    /// Callers must only query positions whose ABI type is [`RuntimeType::I64`].
    #[must_use]
    pub const fn integer_parameter_constraint(self, position: usize) -> IntegerKindConstraint {
        match (self, position) {
            (Self::Integer { kind, .. }, 0) => IntegerKindConstraint::Exact(kind),
            (Self::Integer { op, kind }, 1) => {
                if op.is_shift() {
                    op.count_constraint()
                } else {
                    IntegerKindConstraint::Exact(kind)
                }
            }
            (Self::PrintI64, 0) => IntegerKindConstraint::Signed,
            (Self::PrintU64, 0) => IntegerKindConstraint::Unsigned,
            (Self::GoInterfaceBoxI64, 1) => IntegerKindConstraint::Any,
            // Every scalar integer element shares the one i64 carrier while
            // keeping its own declared kind.
            (Self::GoSliceI64Set, 2) | (Self::GoSliceI64Append, 1) => IntegerKindConstraint::Any,
            (Self::GoSliceU8Set, 2) => IntegerKindConstraint::Exact(IntegerKind::U8),
            // A struct field keeps its own declared integer kind while the
            // pointee stores every field in one shared `i64` carrier.
            (Self::GoPointerStructI64Set, 2) => IntegerKindConstraint::Any,
            (Self::GoStringFromRune, 0) => IntegerKindConstraint::Any,
            _ => IntegerKindConstraint::Exact(IntegerKind::I64),
        }
    }

    /// Semantic Go integer constraint for one flattened physical `I64` result
    /// slot. Callers must only query result positions whose ABI component is
    /// [`RuntimeType::I64`].
    #[must_use]
    pub const fn integer_result_constraint(self, position: usize) -> IntegerKindConstraint {
        match (self, position) {
            (Self::Integer { kind, .. }, 0) => IntegerKindConstraint::Exact(kind),
            (Self::GoInterfaceUnboxI64, 0) => IntegerKindConstraint::Any,
            // The carrier is shared; the field's declared kind is the result.
            (Self::GoPointerStructI64Get, 0) => IntegerKindConstraint::Any,
            (Self::GoSliceI64Index, 0) => IntegerKindConstraint::Any,
            (Self::GoSliceU8Index | Self::GoStringIndex, 0) => {
                IntegerKindConstraint::Exact(IntegerKind::U8)
            }
            (Self::GoStringRangeRuneAt, 0) => IntegerKindConstraint::Exact(IntegerKind::I32),
            _ => IntegerKindConstraint::Exact(IntegerKind::I64),
        }
    }

    /// Exact typed call signature at the Rust runtime boundary.
    #[must_use]
    pub const fn signature(self) -> RuntimeSignature {
        match self {
            Self::GoStringFromBytes => {
                RuntimeSignature::new(BYTES_PARAMETER, RuntimeType::GoString)
            }
            Self::GoStringFromStatic => {
                RuntimeSignature::new(STATIC_BYTES_PARAMETER, RuntimeType::GoString)
            }
            Self::ConcatGoStrings => {
                RuntimeSignature::new(TWO_GO_STRING_PARAMETERS, RuntimeType::GoString)
            }
            Self::Integer { .. } => RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64),
            Self::PrintSpace | Self::PrintNewline => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::Unit)
            }
            Self::PrintBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PrintI64 | Self::PrintU64 => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit)
            }
            Self::PrintF64 | Self::PrintF32 => {
                RuntimeSignature::new(F64_PARAMETER, RuntimeType::Unit)
            }
            Self::PrintGoString => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::Unit),
            Self::PanicBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PanicI64 => RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit),
            Self::PanicGoString => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::Unit),
            Self::PanicGoInterface => {
                RuntimeSignature::new(GO_INTERFACE_PARAMETER, RuntimeType::Unit)
            }
            Self::GoPanicPayloadToInterface => {
                RuntimeSignature::new(GO_PANIC_PAYLOAD_PARAMETER, RuntimeType::GoInterface)
            }
            Self::GoInterfaceIsRuntimeError => {
                RuntimeSignature::new(GO_INTERFACE_PARAMETER, RuntimeType::Bool)
            }
            Self::GoSliceI64FromStatic => {
                RuntimeSignature::new(STATIC_I64_SLICE_PARAMETER, RuntimeType::GoSliceI64)
            }
            Self::GoSliceI64Index => {
                RuntimeSignature::new(GO_SLICE_I64_AND_INDEX, RuntimeType::I64)
            }
            Self::GoSliceI64Range => {
                RuntimeSignature::new(GO_SLICE_I64_RANGE, RuntimeType::GoSliceI64)
            }
            Self::GoSliceI64Set => RuntimeSignature::new(GO_SLICE_I64_SET, RuntimeType::Unit),
            Self::GoSliceI64Make => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::GoSliceI64)
            }
            Self::GoSliceI64Len | Self::GoSliceI64Cap => {
                RuntimeSignature::new(GO_SLICE_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoSliceI64Append => {
                RuntimeSignature::new(GO_SLICE_I64_AND_INDEX, RuntimeType::GoSliceI64)
            }
            Self::GoSliceI64AppendSlice => {
                RuntimeSignature::new(TWO_GO_SLICE_I64_PARAMETERS, RuntimeType::GoSliceI64)
            }
            Self::GoSliceU8FromStatic => {
                RuntimeSignature::new(STATIC_BYTES_PARAMETER, RuntimeType::GoSliceU8)
            }
            Self::GoSliceU8AppendSlice => {
                RuntimeSignature::new(TWO_GO_SLICE_U8_PARAMETERS, RuntimeType::GoSliceU8)
            }
            Self::GoSliceU8AppendString => {
                RuntimeSignature::new(GO_SLICE_U8_AND_STRING, RuntimeType::GoSliceU8)
            }
            Self::GoSliceU8CopyString => {
                RuntimeSignature::new(GO_SLICE_U8_AND_STRING, RuntimeType::I64)
            }
            Self::GoSliceI64Clear => {
                RuntimeSignature::new(GO_SLICE_I64_PARAMETER, RuntimeType::Unit)
            }
            Self::GoStringFromSliceU8 => {
                RuntimeSignature::new(GO_SLICE_U8_PARAMETER, RuntimeType::GoString)
            }
            Self::GoSliceI64Copy => {
                RuntimeSignature::new(TWO_GO_SLICE_I64_PARAMETERS, RuntimeType::I64)
            }
            Self::GoMapStringI64Nil | Self::GoMapStringI64Make => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoMapStringI64)
            }
            Self::GoMapStringI64Len => {
                RuntimeSignature::new(GO_MAP_STRING_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoMapStringI64Get => {
                RuntimeSignature::new(GO_MAP_STRING_I64_AND_KEY, RuntimeType::I64)
            }
            Self::GoMapStringI64Contains => {
                RuntimeSignature::new(GO_MAP_STRING_I64_AND_KEY, RuntimeType::Bool)
            }
            Self::GoMapStringI64IsNil => {
                RuntimeSignature::new(GO_MAP_STRING_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoMapStringI64Set => {
                RuntimeSignature::new(GO_MAP_STRING_I64_SET, RuntimeType::Unit)
            }
            Self::GoMapStringI64Delete => {
                RuntimeSignature::new(GO_MAP_STRING_I64_AND_KEY, RuntimeType::Unit)
            }
            Self::GoMapStringI64Clear => {
                RuntimeSignature::new(GO_MAP_STRING_I64_PARAMETER, RuntimeType::Unit)
            }
            Self::GoMapStringI64KeyAt => {
                RuntimeSignature::new(GO_MAP_STRING_I64_AND_INDEX, RuntimeType::GoString)
            }
            Self::GoMapStringI64RangeKeys => {
                RuntimeSignature::new(GO_MAP_STRING_I64_PARAMETER, RuntimeType::GoSliceGoString)
            }
            Self::GoMapI64GoStringNil | Self::GoMapI64GoStringMake => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoMapI64GoString)
            }
            Self::GoMapI64GoStringLen => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_PARAMETER, RuntimeType::I64)
            }
            Self::GoMapI64GoStringGet => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_AND_KEY, RuntimeType::GoString)
            }
            Self::GoMapI64GoStringContains => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_AND_KEY, RuntimeType::Bool)
            }
            Self::GoMapI64GoStringSet => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_SET, RuntimeType::Unit)
            }
            Self::GoMapI64GoStringDelete => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_AND_KEY, RuntimeType::Unit)
            }
            Self::GoMapI64GoStringClear => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_PARAMETER, RuntimeType::Unit)
            }
            Self::GoMapI64GoStringIsNil => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_PARAMETER, RuntimeType::Bool)
            }
            Self::GoMapI64GoStringRangeKeys => {
                RuntimeSignature::new(GO_MAP_I64_GO_STRING_PARAMETER, RuntimeType::GoSliceI64)
            }
            Self::GoPointerI64Nil | Self::GoPointerI64New => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoPointerI64)
            }
            Self::GoPointerI64Get => {
                RuntimeSignature::new(GO_POINTER_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoPointerI64Set => RuntimeSignature::new(GO_POINTER_I64_SET, RuntimeType::Unit),
            Self::GoPointerI64IsNil => {
                RuntimeSignature::new(GO_POINTER_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoPointerI64Equal => {
                RuntimeSignature::new(TWO_GO_POINTER_I64_PARAMETERS, RuntimeType::Bool)
            }
            Self::GoChannelI64Nil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoChannelI64)
            }
            Self::GoChannelI64Make => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::GoChannelI64)
            }
            Self::GoChannelI64Len | Self::GoChannelI64Cap => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoChannelI64Send => RuntimeSignature::new(GO_CHANNEL_I64_SEND, RuntimeType::Unit),
            Self::GoChannelI64ReceiveValue => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoChannelI64Receive => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::I64BoolTuple)
            }
            Self::GoChannelI64Close => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::Unit)
            }
            Self::GoChannelI64IsNil => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoStringLen => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::I64),
            Self::GoChannelI64TrySend => {
                RuntimeSignature::new(GO_CHANNEL_I64_SEND, RuntimeType::Bool)
            }
            Self::GoChannelI64TryReceive => {
                RuntimeSignature::new(GO_CHANNEL_I64_PARAMETER, RuntimeType::I64I64Tuple)
            }
            Self::GoChannelGoStringNil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoChannelGoString)
            }
            Self::GoChannelGoStringMake => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::GoChannelGoString)
            }
            Self::GoChannelGoStringLen | Self::GoChannelGoStringCap => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_PARAMETER, RuntimeType::I64)
            }
            Self::GoChannelGoStringSend => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_SEND, RuntimeType::Unit)
            }
            Self::GoChannelGoStringReceiveValue => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_PARAMETER, RuntimeType::GoString)
            }
            Self::GoChannelGoStringReceive => RuntimeSignature::new(
                GO_CHANNEL_GO_STRING_PARAMETER,
                RuntimeType::GoStringBoolTuple,
            ),
            Self::GoChannelGoStringClose => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_PARAMETER, RuntimeType::Unit)
            }
            Self::GoChannelGoStringIsNil => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_PARAMETER, RuntimeType::Bool)
            }
            Self::GoChannelGoStringTrySend => {
                RuntimeSignature::new(GO_CHANNEL_GO_STRING_SEND, RuntimeType::Bool)
            }
            Self::GoChannelGoStringTryReceive => RuntimeSignature::new(
                GO_CHANNEL_GO_STRING_PARAMETER,
                RuntimeType::GoStringI64Tuple,
            ),
            Self::GoChannelGoChannelI64Nil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoChannelGoChannelI64)
            }
            Self::GoChannelGoChannelI64Make => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::GoChannelGoChannelI64)
            }
            Self::GoChannelGoChannelI64Len | Self::GoChannelGoChannelI64Cap => {
                RuntimeSignature::new(GO_CHANNEL_GO_CHANNEL_I64_PARAMETER, RuntimeType::I64)
            }
            Self::GoChannelGoChannelI64Send => {
                RuntimeSignature::new(GO_CHANNEL_GO_CHANNEL_I64_SEND, RuntimeType::Unit)
            }
            Self::GoChannelGoChannelI64ReceiveValue => RuntimeSignature::new(
                GO_CHANNEL_GO_CHANNEL_I64_PARAMETER,
                RuntimeType::GoChannelI64,
            ),
            Self::GoChannelGoChannelI64Receive => RuntimeSignature::new(
                GO_CHANNEL_GO_CHANNEL_I64_PARAMETER,
                RuntimeType::GoChannelI64BoolTuple,
            ),
            Self::GoChannelGoChannelI64Close => {
                RuntimeSignature::new(GO_CHANNEL_GO_CHANNEL_I64_PARAMETER, RuntimeType::Unit)
            }
            Self::GoChannelGoChannelI64IsNil => {
                RuntimeSignature::new(GO_CHANNEL_GO_CHANNEL_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoChannelGoChannelI64TrySend => {
                RuntimeSignature::new(GO_CHANNEL_GO_CHANNEL_I64_SEND, RuntimeType::Bool)
            }
            Self::GoChannelGoChannelI64TryReceive => RuntimeSignature::new(
                GO_CHANNEL_GO_CHANNEL_I64_PARAMETER,
                RuntimeType::GoChannelI64I64Tuple,
            ),
            Self::GoSliceGoStringNil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoSliceGoString)
            }
            Self::GoSliceGoStringMake => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::GoSliceGoString)
            }
            Self::GoSliceGoStringLen | Self::GoSliceGoStringCap => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_PARAMETER, RuntimeType::I64)
            }
            Self::GoSliceGoStringIndex => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_AND_INDEX, RuntimeType::GoString)
            }
            Self::GoSliceGoStringRange => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_RANGE, RuntimeType::GoSliceGoString)
            }
            Self::GoSliceGoStringSet => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_SET, RuntimeType::Unit)
            }
            Self::GoSliceGoStringAppend => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_AND_VALUE, RuntimeType::GoSliceGoString)
            }
            Self::GoSliceGoStringCopy => {
                RuntimeSignature::new(TWO_GO_SLICE_GO_STRING_PARAMETERS, RuntimeType::I64)
            }
            Self::GoSliceGoStringClear => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_PARAMETER, RuntimeType::Unit)
            }
            Self::GoSliceGoStringIsNil => {
                RuntimeSignature::new(GO_SLICE_GO_STRING_PARAMETER, RuntimeType::Bool)
            }
            Self::GoInterfaceBoxGoSliceGoString => RuntimeSignature::new(
                GO_INTERFACE_BOX_GO_SLICE_GO_STRING,
                RuntimeType::GoInterface,
            ),
            Self::GoInterfaceUnboxGoSliceGoString => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::GoSliceGoString)
            }
            Self::GoPointerStructI64Nil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoPointerStructI64)
            }
            Self::GoPointerStructI64New => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::GoPointerStructI64)
            }
            Self::GoPointerStructI64Get => {
                RuntimeSignature::new(GO_POINTER_STRUCT_I64_AND_INDEX, RuntimeType::I64)
            }
            Self::GoPointerStructI64Set => {
                RuntimeSignature::new(GO_POINTER_STRUCT_I64_SET, RuntimeType::Unit)
            }
            Self::GoPointerStructI64IsNil => {
                RuntimeSignature::new(GO_POINTER_STRUCT_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoPointerStructI64Equal => {
                RuntimeSignature::new(TWO_GO_POINTER_STRUCT_I64_PARAMETERS, RuntimeType::Bool)
            }
            Self::GoInterfaceNil => RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoInterface),
            Self::GoInterfaceBoxBool => {
                RuntimeSignature::new(GO_INTERFACE_BOX_BOOL, RuntimeType::GoInterface)
            }
            Self::GoInterfaceBoxI64 => {
                RuntimeSignature::new(GO_INTERFACE_BOX_I64, RuntimeType::GoInterface)
            }
            Self::GoInterfaceBoxF64 | Self::GoInterfaceBoxF32 => {
                RuntimeSignature::new(GO_INTERFACE_BOX_F64, RuntimeType::GoInterface)
            }
            Self::GoInterfaceBoxGoString => {
                RuntimeSignature::new(GO_INTERFACE_BOX_STRING, RuntimeType::GoInterface)
            }
            Self::GoInterfaceBoxStructI64 => {
                RuntimeSignature::new(GO_INTERFACE_BOX_STRUCT_I64, RuntimeType::GoInterface)
            }
            Self::GoInterfaceBoxPointerStructI64 => RuntimeSignature::new(
                GO_INTERFACE_BOX_POINTER_STRUCT_I64,
                RuntimeType::GoInterface,
            ),
            Self::GoInterfaceBoxPointerI64 => {
                RuntimeSignature::new(GO_INTERFACE_BOX_POINTER_I64, RuntimeType::GoInterface)
            }
            Self::GoInterfaceIsNil => {
                RuntimeSignature::new(GO_INTERFACE_PARAMETER, RuntimeType::Bool)
            }
            Self::GoInterfaceIsType => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::Bool)
            }
            Self::GoInterfaceUnboxBool => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::Bool)
            }
            Self::GoInterfaceUnboxI64 => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::I64)
            }
            Self::GoInterfaceUnboxF64 | Self::GoInterfaceUnboxF32 => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::F64)
            }
            Self::GoInterfaceEqual => {
                RuntimeSignature::new(TWO_GO_INTERFACE_PARAMETERS, RuntimeType::Bool)
            }
            Self::GoInterfaceUnboxGoString => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::GoString)
            }
            Self::GoInterfaceStructI64Get => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE_AND_INDEX, RuntimeType::I64)
            }
            Self::GoInterfaceUnboxPointerStructI64 => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::GoPointerStructI64)
            }
            Self::GoInterfaceUnboxPointerI64 => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::GoPointerI64)
            }
            Self::GoSliceBoolFromStatic => {
                RuntimeSignature::new(STATIC_BOOL_SLICE_PARAMETER, RuntimeType::GoSliceBool)
            }
            Self::GoSliceBoolIndex => {
                RuntimeSignature::new(GO_SLICE_BOOL_AND_INDEX, RuntimeType::Bool)
            }
            Self::GoSliceBoolSet => RuntimeSignature::new(GO_SLICE_BOOL_SET, RuntimeType::Unit),
            Self::GoSliceInterfaceMake => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::GoSliceInterface)
            }
            Self::GoSliceInterfaceLen => {
                RuntimeSignature::new(GO_SLICE_INTERFACE_PARAMETER, RuntimeType::I64)
            }
            Self::GoSliceInterfaceIndex => {
                RuntimeSignature::new(GO_SLICE_INTERFACE_AND_INDEX, RuntimeType::GoInterface)
            }
            Self::GoSliceInterfaceSet => {
                RuntimeSignature::new(GO_SLICE_INTERFACE_SET, RuntimeType::Unit)
            }
            Self::GoSliceInterfaceAppend => RuntimeSignature::new(
                TWO_GO_SLICE_INTERFACE_PARAMETERS,
                RuntimeType::GoSliceInterface,
            ),
            Self::GoMapStringInterfaceMake => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoMapStringInterface)
            }
            Self::GoMapStringInterfaceLen => {
                RuntimeSignature::new(GO_MAP_STRING_INTERFACE_PARAMETER, RuntimeType::I64)
            }
            Self::GoMapStringInterfaceGet => {
                RuntimeSignature::new(GO_MAP_STRING_INTERFACE_AND_KEY, RuntimeType::GoInterface)
            }
            Self::GoMapStringInterfaceContains => {
                RuntimeSignature::new(GO_MAP_STRING_INTERFACE_AND_KEY, RuntimeType::Bool)
            }
            Self::GoMapStringInterfaceSet => {
                RuntimeSignature::new(GO_MAP_STRING_INTERFACE_SET, RuntimeType::Unit)
            }
            Self::GoSliceU8Len => RuntimeSignature::new(GO_SLICE_U8_PARAMETER, RuntimeType::I64),
            Self::GoSliceU8Index => RuntimeSignature::new(GO_SLICE_U8_AND_INDEX, RuntimeType::I64),
            Self::GoSliceU8Range => {
                RuntimeSignature::new(GO_SLICE_U8_RANGE, RuntimeType::GoSliceU8)
            }
            Self::GoStringIndex => RuntimeSignature::new(GO_STRING_AND_INDEX, RuntimeType::I64),
            Self::GoStringRange => RuntimeSignature::new(GO_STRING_RANGE, RuntimeType::GoString),
            Self::GoStringFromSliceRunes => {
                RuntimeSignature::new(GO_SLICE_I64_PARAMETER, RuntimeType::GoString)
            }
            Self::GoStringToSliceRunes => {
                RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::GoSliceI64)
            }
            Self::GoStringFromRune => RuntimeSignature::new(I64_PARAMETER, RuntimeType::GoString),
            Self::GoStringRangeCount => {
                RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::I64)
            }
            Self::GoStringRangeIndexAt | Self::GoStringRangeRuneAt => {
                RuntimeSignature::new(GO_STRING_AND_INDEX, RuntimeType::I64)
            }
            Self::GoSliceI64Nil => RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoSliceI64),
            Self::GoSliceI64IsNil => {
                RuntimeSignature::new(GO_SLICE_I64_PARAMETER, RuntimeType::Bool)
            }
            Self::GoSliceU8Nil => RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoSliceU8),
            Self::GoSliceU8IsNil => RuntimeSignature::new(GO_SLICE_U8_PARAMETER, RuntimeType::Bool),
            Self::GoSliceBoolNil => RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoSliceBool),
            Self::GoSliceBoolIsNil => {
                RuntimeSignature::new(GO_SLICE_BOOL_PARAMETER, RuntimeType::Bool)
            }
            Self::GoSliceInterfaceNil => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::GoSliceInterface)
            }
            Self::GoSliceInterfaceIsNil => {
                RuntimeSignature::new(GO_SLICE_INTERFACE_PARAMETER, RuntimeType::Bool)
            }
            Self::GoInterfaceBoxAggregate | Self::GoInterfaceBoxComparableAggregate => {
                RuntimeSignature::new(GO_INTERFACE_BOX_AGGREGATE, RuntimeType::GoInterface)
            }
            Self::GoInterfaceUnboxAggregate => {
                RuntimeSignature::new(GO_INTERFACE_AND_TYPE, RuntimeType::GoSliceInterface)
            }
            Self::GoSliceU8Make => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::GoSliceU8)
            }
            Self::GoSliceU8Set => RuntimeSignature::new(GO_SLICE_U8_SET, RuntimeType::Unit),
            Self::GoSliceU8Copy => {
                RuntimeSignature::new(TWO_GO_SLICE_U8_PARAMETERS, RuntimeType::I64)
            }
        }
    }
}
