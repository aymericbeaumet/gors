//! Stable native and runtime operation catalogs.

mod decode;
mod identity;
mod integer;
mod metadata;
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

pub use identity::{RuntimeOpId, UnknownRuntimeOpId};
pub use integer::{IntegerKind, IntegerKindConstraint, IntegerPrimitive, IntegerRuntimeOp};
pub use runtime_op::RuntimeOp;
pub use value_model::{RuntimeSignature, RuntimeType};

use crate::encoding::CanonicalEncoder;

/// Go operations emitted directly without a runtime ABI symbol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PrimitiveOp {
    BoolNot,
    BoolEqual,
    BoolNotEqual,
    StringEqual,
    StringNotEqual,
    StringLess,
    StringLessEqual,
    StringGreater,
    StringGreaterEqual,
    FloatAdd,
    FloatSub,
    FloatMul,
    FloatDiv,
    FloatNeg,
    FloatEqual,
    FloatNotEqual,
    FloatLess,
    FloatLessEqual,
    FloatGreater,
    FloatGreaterEqual,
    ComplexAdd,
    ComplexSub,
    ComplexMul,
    ComplexDiv,
    ComplexNeg,
    ComplexEqual,
    ComplexNotEqual,
    FloatMin,
    FloatMax,
    ComplexFromParts,
    ComplexReal,
    ComplexImag,
    FloatRound32,
    Integer {
        op: IntegerPrimitive,
        kind: IntegerKind,
    },
    IntegerConvert {
        from: IntegerKind,
        to: IntegerKind,
    },
}

/// Stable compact identity of one directly emitted operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrimitiveOpId(u16);

impl PrimitiveOpId {
    /// Canonical numeric value used by fingerprints and manifest encodings.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl PrimitiveOp {
    /// Complete native-operation catalog for the current contract.
    pub const ALL: &'static [Self] = &Self::CATALOG;

    const NON_INTEGER: &'static [Self] = &[
        Self::BoolNot,
        Self::BoolEqual,
        Self::BoolNotEqual,
        Self::StringEqual,
        Self::StringNotEqual,
        Self::StringLess,
        Self::StringLessEqual,
        Self::StringGreater,
        Self::StringGreaterEqual,
        Self::FloatAdd,
        Self::FloatSub,
        Self::FloatMul,
        Self::FloatDiv,
        Self::FloatNeg,
        Self::FloatEqual,
        Self::FloatNotEqual,
        Self::FloatLess,
        Self::FloatLessEqual,
        Self::FloatGreater,
        Self::FloatGreaterEqual,
        Self::ComplexAdd,
        Self::ComplexSub,
        Self::ComplexMul,
        Self::ComplexDiv,
        Self::ComplexNeg,
        Self::ComplexEqual,
        Self::ComplexNotEqual,
        Self::FloatMin,
        Self::FloatMax,
        Self::ComplexFromParts,
        Self::ComplexReal,
        Self::ComplexImag,
        Self::FloatRound32,
    ];

    const CATALOG: [Self; 233] = primitive_catalog();

    /// Exact typed signature for this directly emitted operation.
    #[must_use]
    pub const fn signature(self) -> RuntimeSignature {
        match self {
            Self::BoolNot => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Bool),
            Self::BoolEqual | Self::BoolNotEqual => {
                RuntimeSignature::new(TWO_BOOL_PARAMETERS, RuntimeType::Bool)
            }
            Self::Integer { op, .. } if op.arity() == 1 => {
                RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64)
            }
            Self::Integer { op, .. } if op.returns_bool() => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::Bool)
            }
            Self::Integer { .. } => RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64),
            Self::IntegerConvert { .. } => RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64),
            Self::FloatAdd | Self::FloatSub | Self::FloatMul | Self::FloatDiv => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::F64)
            }
            Self::FloatNeg | Self::FloatRound32 => {
                RuntimeSignature::new(F64_PARAMETER, RuntimeType::F64)
            }
            Self::FloatEqual
            | Self::FloatNotEqual
            | Self::FloatLess
            | Self::FloatLessEqual
            | Self::FloatGreater
            | Self::FloatGreaterEqual => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::Bool)
            }
            Self::ComplexAdd | Self::ComplexSub | Self::ComplexMul | Self::ComplexDiv => {
                RuntimeSignature::new(TWO_COMPLEX128_PARAMETERS, RuntimeType::Complex128)
            }
            Self::ComplexNeg => {
                RuntimeSignature::new(COMPLEX128_PARAMETER, RuntimeType::Complex128)
            }
            Self::ComplexEqual | Self::ComplexNotEqual => {
                RuntimeSignature::new(TWO_COMPLEX128_PARAMETERS, RuntimeType::Bool)
            }
            Self::FloatMin | Self::FloatMax => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::F64)
            }
            Self::ComplexFromParts => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::Complex128)
            }
            Self::ComplexReal | Self::ComplexImag => {
                RuntimeSignature::new(COMPLEX128_PARAMETER, RuntimeType::F64)
            }
            Self::StringEqual
            | Self::StringNotEqual
            | Self::StringLess
            | Self::StringLessEqual
            | Self::StringGreater
            | Self::StringGreaterEqual => {
                RuntimeSignature::new(TWO_GO_STRING_PARAMETERS, RuntimeType::Bool)
            }
        }
    }

    /// Stable semantic name protected by the canonical contract identity.
    ///
    /// This is not a runtime symbol: primitive operations are emitted directly.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Integer { op, kind } => format!("{}-{}", kind.name(), op.name()),
            Self::IntegerConvert { from, to } => {
                format!("{}-to-{}", from.name(), to.name())
            }
            other => other.non_integer_name().to_owned(),
        }
    }

    const fn non_integer_name(self) -> &'static str {
        match self {
            Self::BoolNot => "bool-not",
            Self::BoolEqual => "bool-equal",
            Self::BoolNotEqual => "bool-not-equal",
            Self::StringEqual => "string-equal",
            Self::StringNotEqual => "string-not-equal",
            Self::StringLess => "string-less",
            Self::StringLessEqual => "string-less-equal",
            Self::StringGreater => "string-greater",
            Self::StringGreaterEqual => "string-greater-equal",
            Self::FloatAdd => "float-add",
            Self::FloatSub => "float-sub",
            Self::FloatMul => "float-mul",
            Self::FloatDiv => "float-div",
            Self::FloatNeg => "float-neg",
            Self::FloatEqual => "float-equal",
            Self::FloatNotEqual => "float-not-equal",
            Self::FloatLess => "float-less",
            Self::FloatLessEqual => "float-less-equal",
            Self::FloatGreater => "float-greater",
            Self::FloatGreaterEqual => "float-greater-equal",
            Self::ComplexAdd => "complex-add",
            Self::ComplexSub => "complex-sub",
            Self::ComplexMul => "complex-mul",
            Self::ComplexDiv => "complex-div",
            Self::ComplexNeg => "complex-neg",
            Self::ComplexEqual => "complex-equal",
            Self::ComplexNotEqual => "complex-not-equal",
            Self::FloatMin => "float-min",
            Self::FloatMax => "float-max",
            Self::ComplexFromParts => "complex-from-parts",
            Self::ComplexReal => "complex-real",
            Self::ComplexImag => "complex-imag",
            Self::FloatRound32 => "float-round-32",
            Self::Integer { .. } | Self::IntegerConvert { .. } => "integer-operation",
        }
    }

    /// Stable compact identity for canonical encodings and fingerprints.
    #[must_use]
    pub const fn id(self) -> PrimitiveOpId {
        PrimitiveOpId(match self {
            Self::BoolNot => 1,
            Self::BoolEqual => 2,
            Self::BoolNotEqual => 3,
            Self::FloatAdd => 25,
            Self::FloatSub => 26,
            Self::FloatMul => 27,
            Self::FloatDiv => 28,
            Self::FloatNeg => 29,
            Self::FloatEqual => 30,
            Self::FloatNotEqual => 31,
            Self::FloatLess => 32,
            Self::FloatLessEqual => 33,
            Self::FloatGreater => 34,
            Self::FloatGreaterEqual => 35,
            Self::ComplexAdd => 36,
            Self::ComplexSub => 37,
            Self::ComplexMul => 38,
            Self::ComplexDiv => 39,
            Self::ComplexNeg => 40,
            Self::ComplexEqual => 41,
            Self::ComplexNotEqual => 42,
            Self::FloatMin => 45,
            Self::FloatMax => 46,
            Self::ComplexFromParts => 47,
            Self::ComplexReal => 48,
            Self::ComplexImag => 49,
            Self::FloatRound32 => 50,
            Self::Integer { op, kind } => 53 + op.ordinal() * 8 + kind.ordinal(),
            Self::IntegerConvert { from, to } => 189 + from.ordinal() * 8 + to.ordinal(),
            Self::StringEqual => 15,
            Self::StringNotEqual => 16,
            Self::StringLess => 17,
            Self::StringLessEqual => 18,
            Self::StringGreater => 19,
            Self::StringGreaterEqual => 20,
        })
    }

    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u16(self.id().get());
        encoder.text(&self.name());
        self.signature().encode(encoder);
    }
}

// Every index is bounded by the lengths which define the exact catalog size.
// Const slice access cannot yet use the checked APIs on the supported compiler.
#[allow(clippy::indexing_slicing)]
const fn primitive_catalog() -> [PrimitiveOp; 233] {
    let mut result = [PrimitiveOp::BoolNot; 233];
    let mut output = 0;
    let mut index = 0;
    while index < PrimitiveOp::NON_INTEGER.len() {
        result[output] = PrimitiveOp::NON_INTEGER[index];
        output += 1;
        index += 1;
    }
    let mut primitive = 0;
    while primitive < IntegerPrimitive::ALL.len() {
        let mut kind = 0;
        while kind < IntegerKind::ALL.len() {
            result[output] = PrimitiveOp::Integer {
                op: IntegerPrimitive::ALL[primitive],
                kind: IntegerKind::ALL[kind],
            };
            output += 1;
            kind += 1;
        }
        primitive += 1;
    }
    let mut from = 0;
    while from < IntegerKind::ALL.len() {
        let mut to = 0;
        while to < IntegerKind::ALL.len() {
            result[output] = PrimitiveOp::IntegerConvert {
                from: IntegerKind::ALL[from],
                to: IntegerKind::ALL[to],
            };
            output += 1;
            to += 1;
        }
        from += 1;
    }
    result
}

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
            (Self::GoSliceI64Set, 2) | (Self::GoSliceI64Append, 1) => {
                IntegerKindConstraint::I64OrI32
            }
            (Self::GoSliceU8Set, 2) => IntegerKindConstraint::Exact(IntegerKind::U8),
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
            (Self::GoSliceI64Index, 0) => IntegerKindConstraint::I64OrI32,
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
            Self::PrintF64 => RuntimeSignature::new(F64_PARAMETER, RuntimeType::Unit),
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
            Self::GoInterfaceBoxF64 => {
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
            Self::GoInterfaceUnboxF64 => {
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
