//! Stable native and runtime operation catalogs.

mod decode;
mod identity;
mod metadata;
mod runtime_encoding;
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
pub use value_model::{RuntimeSignature, RuntimeType};

use crate::encoding::CanonicalEncoder;

/// Go operations emitted directly without a runtime ABI symbol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PrimitiveOp {
    BoolNot,
    BoolEqual,
    BoolNotEqual,
    IntBitNot,
    IntBitAnd,
    IntBitOr,
    IntBitXor,
    IntAndNot,
    IntEqual,
    IntNotEqual,
    IntLess,
    IntLessEqual,
    IntGreater,
    IntGreaterEqual,
    StringEqual,
    StringNotEqual,
    StringLess,
    StringLessEqual,
    StringGreater,
    StringGreaterEqual,
    IntWrappingAdd,
    IntWrappingSub,
    IntWrappingMul,
    IntWrappingNeg,
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
    IntMin,
    IntMax,
    FloatMin,
    FloatMax,
    ComplexFromParts,
    ComplexReal,
    ComplexImag,
    FloatRound32,
    Int32WrappingAdd,
    Int32WrappingNeg,
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
    pub const ALL: &'static [Self] = &[
        Self::BoolNot,
        Self::BoolEqual,
        Self::BoolNotEqual,
        Self::IntBitNot,
        Self::IntBitAnd,
        Self::IntBitOr,
        Self::IntBitXor,
        Self::IntAndNot,
        Self::IntEqual,
        Self::IntNotEqual,
        Self::IntLess,
        Self::IntLessEqual,
        Self::IntGreater,
        Self::IntGreaterEqual,
        Self::StringEqual,
        Self::StringNotEqual,
        Self::StringLess,
        Self::StringLessEqual,
        Self::StringGreater,
        Self::StringGreaterEqual,
        Self::IntWrappingAdd,
        Self::IntWrappingSub,
        Self::IntWrappingMul,
        Self::IntWrappingNeg,
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
        Self::IntMin,
        Self::IntMax,
        Self::FloatMin,
        Self::FloatMax,
        Self::ComplexFromParts,
        Self::ComplexReal,
        Self::ComplexImag,
        Self::FloatRound32,
        Self::Int32WrappingAdd,
        Self::Int32WrappingNeg,
    ];

    /// Exact typed signature for this directly emitted operation.
    #[must_use]
    pub const fn signature(self) -> RuntimeSignature {
        match self {
            Self::BoolNot => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Bool),
            Self::BoolEqual | Self::BoolNotEqual => {
                RuntimeSignature::new(TWO_BOOL_PARAMETERS, RuntimeType::Bool)
            }
            Self::IntBitNot => RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64),
            Self::IntBitAnd | Self::IntBitOr | Self::IntBitXor | Self::IntAndNot => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64)
            }
            Self::IntWrappingAdd | Self::IntWrappingSub | Self::IntWrappingMul => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64)
            }
            Self::IntWrappingNeg => RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64),
            Self::Int32WrappingAdd => RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64),
            Self::Int32WrappingNeg => RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64),
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
            Self::IntMin | Self::IntMax => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64)
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
            Self::IntEqual
            | Self::IntNotEqual
            | Self::IntLess
            | Self::IntLessEqual
            | Self::IntGreater
            | Self::IntGreaterEqual => RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::Bool),
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
    pub const fn name(self) -> &'static str {
        match self {
            Self::BoolNot => "bool-not",
            Self::BoolEqual => "bool-equal",
            Self::BoolNotEqual => "bool-not-equal",
            Self::IntBitNot => "int-bit-not",
            Self::IntBitAnd => "int-bit-and",
            Self::IntBitOr => "int-bit-or",
            Self::IntBitXor => "int-bit-xor",
            Self::IntAndNot => "int-and-not",
            Self::IntEqual => "int-equal",
            Self::IntNotEqual => "int-not-equal",
            Self::IntLess => "int-less",
            Self::IntLessEqual => "int-less-equal",
            Self::IntGreater => "int-greater",
            Self::IntGreaterEqual => "int-greater-equal",
            Self::StringEqual => "string-equal",
            Self::StringNotEqual => "string-not-equal",
            Self::StringLess => "string-less",
            Self::StringLessEqual => "string-less-equal",
            Self::StringGreater => "string-greater",
            Self::StringGreaterEqual => "string-greater-equal",
            Self::IntWrappingAdd => "int-wrapping-add",
            Self::IntWrappingSub => "int-wrapping-sub",
            Self::IntWrappingMul => "int-wrapping-mul",
            Self::IntWrappingNeg => "int-wrapping-neg",
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
            Self::IntMin => "int-min",
            Self::IntMax => "int-max",
            Self::FloatMin => "float-min",
            Self::FloatMax => "float-max",
            Self::ComplexFromParts => "complex-from-parts",
            Self::ComplexReal => "complex-real",
            Self::ComplexImag => "complex-imag",
            Self::FloatRound32 => "float-round-32",
            Self::Int32WrappingAdd => "int32-wrapping-add",
            Self::Int32WrappingNeg => "int32-wrapping-neg",
        }
    }

    /// Stable compact identity for canonical encodings and fingerprints.
    #[must_use]
    pub const fn id(self) -> PrimitiveOpId {
        PrimitiveOpId(match self {
            Self::BoolNot => 1,
            Self::BoolEqual => 2,
            Self::BoolNotEqual => 3,
            Self::IntBitNot => 4,
            Self::IntBitAnd => 5,
            Self::IntBitOr => 6,
            Self::IntBitXor => 7,
            Self::IntAndNot => 8,
            Self::IntWrappingAdd => 21,
            Self::IntWrappingSub => 22,
            Self::IntWrappingMul => 23,
            Self::IntWrappingNeg => 24,
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
            Self::IntMin => 43,
            Self::IntMax => 44,
            Self::FloatMin => 45,
            Self::FloatMax => 46,
            Self::ComplexFromParts => 47,
            Self::ComplexReal => 48,
            Self::ComplexImag => 49,
            Self::FloatRound32 => 50,
            Self::Int32WrappingAdd => 51,
            Self::Int32WrappingNeg => 52,
            Self::IntEqual => 9,
            Self::IntNotEqual => 10,
            Self::IntLess => 11,
            Self::IntLessEqual => 12,
            Self::IntGreater => 13,
            Self::IntGreaterEqual => 14,
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
        encoder.text(self.name());
        self.signature().encode(encoder);
    }
}

/// Operations that require an exact symbol from the versioned runtime ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeOp {
    GoStringFromBytes,
    GoStringFromStatic,
    ConcatGoStrings,
    IntDiv,
    IntRem,
    IntShl,
    IntShr,
    PrintBool,
    PrintI64,
    PrintSpace,
    PrintNewline,
    PrintGoString,
    PanicBool,
    PanicI64,
    PanicGoString,
    GoSliceI64FromStatic,
    GoSliceI64Index,
    GoSliceI64Range,
    GoSliceI64Set,
    GoSliceI64Make,
    GoSliceI64Len,
    GoSliceI64Cap,
    GoSliceI64Append,
    GoSliceU8FromStatic,
    GoSliceU8AppendSlice,
    GoSliceU8AppendString,
    GoSliceU8CopyString,
    GoSliceI64Clear,
    GoStringFromSliceU8,
    GoSliceI64Copy,
    GoMapStringI64Nil,
    GoMapStringI64Make,
    GoMapStringI64Len,
    GoMapStringI64Get,
    GoMapStringI64Contains,
    GoMapStringI64Set,
    GoMapStringI64Delete,
    GoMapStringI64Clear,
    GoMapStringI64IsNil,
    GoMapStringI64KeyAt,
    GoPointerI64Nil,
    GoPointerI64New,
    GoPointerI64Get,
    GoPointerI64Set,
    GoPointerI64IsNil,
    GoChannelI64Nil,
    GoChannelI64Make,
    GoChannelI64Len,
    GoChannelI64Cap,
    GoChannelI64Send,
    GoChannelI64ReceiveValue,
    GoChannelI64Receive,
    GoChannelI64Close,
    GoChannelI64IsNil,
    GoStringLen,
    GoChannelI64TrySend,
    GoChannelI64TryReceive,
    GoPointerStructI64Nil,
    GoPointerStructI64New,
    GoPointerStructI64Get,
    GoPointerStructI64Set,
    GoPointerStructI64IsNil,
    GoPointerStructI64Equal,
    GoInterfaceNil,
    GoInterfaceBoxBool,
    GoInterfaceBoxI64,
    GoInterfaceBoxGoString,
    GoInterfaceBoxStructI64,
    GoInterfaceBoxPointerStructI64,
    GoInterfaceIsNil,
    GoInterfaceIsType,
    GoInterfaceUnboxBool,
    GoInterfaceUnboxI64,
    GoInterfaceUnboxGoString,
    GoInterfaceStructI64Get,
    GoInterfaceUnboxPointerStructI64,
    GoSliceBoolFromStatic,
    GoSliceBoolIndex,
    GoSliceBoolSet,
    GoSliceInterfaceMake,
    GoSliceInterfaceLen,
    GoSliceInterfaceIndex,
    GoSliceInterfaceSet,
    GoMapStringInterfaceMake,
    GoMapStringInterfaceLen,
    GoMapStringInterfaceGet,
    GoMapStringInterfaceContains,
    GoMapStringInterfaceSet,
    GoSliceU8Len,
    GoSliceU8Index,
    GoSliceU8Range,
    GoStringIndex,
    GoStringRange,
    GoStringFromSliceRunes,
    GoStringRangeCount,
    GoStringRangeIndexAt,
    GoStringRangeRuneAt,
    GoSliceI64Nil,
    GoSliceI64IsNil,
    GoSliceU8Nil,
    GoSliceU8IsNil,
    GoSliceBoolNil,
    GoSliceBoolIsNil,
    GoSliceInterfaceNil,
    GoSliceInterfaceIsNil,
    GoInterfaceBoxAggregate,
    GoInterfaceUnboxAggregate,
    GoSliceU8Make,
    GoSliceU8Set,
    GoSliceU8Copy,
    PrintF64,
    GoInterfaceBoxF64,
    GoInterfaceUnboxF64,
    GoInterfaceEqual,
    GoInterfaceBoxComparableAggregate,
    GoStringFromRune,
    PanicGoInterface,
    GoPanicPayloadToInterface,
    GoInterfaceIsRuntimeError,
    GoChannelGoStringNil,
    GoChannelGoStringMake,
    GoChannelGoStringLen,
    GoChannelGoStringCap,
    GoChannelGoStringSend,
    GoChannelGoStringReceiveValue,
    GoChannelGoStringReceive,
    GoChannelGoStringClose,
    GoChannelGoStringIsNil,
    GoChannelGoStringTrySend,
    GoChannelGoStringTryReceive,
    GoChannelGoChannelI64Nil,
    GoChannelGoChannelI64Make,
    GoChannelGoChannelI64Len,
    GoChannelGoChannelI64Cap,
    GoChannelGoChannelI64Send,
    GoChannelGoChannelI64ReceiveValue,
    GoChannelGoChannelI64Receive,
    GoChannelGoChannelI64Close,
    GoChannelGoChannelI64IsNil,
    GoChannelGoChannelI64TrySend,
    GoChannelGoChannelI64TryReceive,
}

impl RuntimeOp {
    /// Complete helper catalog for the current contract.
    pub const ALL: &'static [Self] = &[
        Self::GoStringFromBytes,
        Self::GoStringFromStatic,
        Self::ConcatGoStrings,
        Self::IntDiv,
        Self::IntRem,
        Self::IntShl,
        Self::IntShr,
        Self::PrintBool,
        Self::PrintI64,
        Self::PrintSpace,
        Self::PrintNewline,
        Self::PrintGoString,
        Self::PanicBool,
        Self::PanicI64,
        Self::PanicGoString,
        Self::GoSliceI64FromStatic,
        Self::GoSliceI64Index,
        Self::GoSliceI64Range,
        Self::GoSliceI64Set,
        Self::GoSliceI64Make,
        Self::GoSliceI64Len,
        Self::GoSliceI64Cap,
        Self::GoSliceI64Append,
        Self::GoSliceU8FromStatic,
        Self::GoSliceU8AppendSlice,
        Self::GoSliceU8AppendString,
        Self::GoSliceU8CopyString,
        Self::GoSliceI64Clear,
        Self::GoStringFromSliceU8,
        Self::GoSliceI64Copy,
        Self::GoMapStringI64Nil,
        Self::GoMapStringI64Make,
        Self::GoMapStringI64Len,
        Self::GoMapStringI64Get,
        Self::GoMapStringI64Contains,
        Self::GoMapStringI64Set,
        Self::GoMapStringI64Delete,
        Self::GoMapStringI64Clear,
        Self::GoMapStringI64IsNil,
        Self::GoMapStringI64KeyAt,
        Self::GoPointerI64Nil,
        Self::GoPointerI64New,
        Self::GoPointerI64Get,
        Self::GoPointerI64Set,
        Self::GoPointerI64IsNil,
        Self::GoChannelI64Nil,
        Self::GoChannelI64Make,
        Self::GoChannelI64Len,
        Self::GoChannelI64Cap,
        Self::GoChannelI64Send,
        Self::GoChannelI64ReceiveValue,
        Self::GoChannelI64Receive,
        Self::GoChannelI64Close,
        Self::GoChannelI64IsNil,
        Self::GoStringLen,
        Self::GoChannelI64TrySend,
        Self::GoChannelI64TryReceive,
        Self::GoPointerStructI64Nil,
        Self::GoPointerStructI64New,
        Self::GoPointerStructI64Get,
        Self::GoPointerStructI64Set,
        Self::GoPointerStructI64IsNil,
        Self::GoPointerStructI64Equal,
        Self::GoInterfaceNil,
        Self::GoInterfaceBoxBool,
        Self::GoInterfaceBoxI64,
        Self::GoInterfaceBoxGoString,
        Self::GoInterfaceBoxStructI64,
        Self::GoInterfaceBoxPointerStructI64,
        Self::GoInterfaceIsNil,
        Self::GoInterfaceIsType,
        Self::GoInterfaceUnboxBool,
        Self::GoInterfaceUnboxI64,
        Self::GoInterfaceUnboxGoString,
        Self::GoInterfaceStructI64Get,
        Self::GoInterfaceUnboxPointerStructI64,
        Self::GoSliceBoolFromStatic,
        Self::GoSliceBoolIndex,
        Self::GoSliceBoolSet,
        Self::GoSliceInterfaceMake,
        Self::GoSliceInterfaceLen,
        Self::GoSliceInterfaceIndex,
        Self::GoSliceInterfaceSet,
        Self::GoMapStringInterfaceMake,
        Self::GoMapStringInterfaceLen,
        Self::GoMapStringInterfaceGet,
        Self::GoMapStringInterfaceContains,
        Self::GoMapStringInterfaceSet,
        Self::GoSliceU8Len,
        Self::GoSliceU8Index,
        Self::GoSliceU8Range,
        Self::GoStringIndex,
        Self::GoStringRange,
        Self::GoStringFromSliceRunes,
        Self::GoStringRangeCount,
        Self::GoStringRangeIndexAt,
        Self::GoStringRangeRuneAt,
        Self::GoSliceI64Nil,
        Self::GoSliceI64IsNil,
        Self::GoSliceU8Nil,
        Self::GoSliceU8IsNil,
        Self::GoSliceBoolNil,
        Self::GoSliceBoolIsNil,
        Self::GoSliceInterfaceNil,
        Self::GoSliceInterfaceIsNil,
        Self::GoInterfaceBoxAggregate,
        Self::GoInterfaceUnboxAggregate,
        Self::GoSliceU8Make,
        Self::GoSliceU8Set,
        Self::GoSliceU8Copy,
        Self::PrintF64,
        Self::GoInterfaceBoxF64,
        Self::GoInterfaceUnboxF64,
        Self::GoInterfaceEqual,
        Self::GoInterfaceBoxComparableAggregate,
        Self::GoStringFromRune,
        Self::PanicGoInterface,
        Self::GoPanicPayloadToInterface,
        Self::GoInterfaceIsRuntimeError,
        Self::GoChannelGoStringNil,
        Self::GoChannelGoStringMake,
        Self::GoChannelGoStringLen,
        Self::GoChannelGoStringCap,
        Self::GoChannelGoStringSend,
        Self::GoChannelGoStringReceiveValue,
        Self::GoChannelGoStringReceive,
        Self::GoChannelGoStringClose,
        Self::GoChannelGoStringIsNil,
        Self::GoChannelGoStringTrySend,
        Self::GoChannelGoStringTryReceive,
        Self::GoChannelGoChannelI64Nil,
        Self::GoChannelGoChannelI64Make,
        Self::GoChannelGoChannelI64Len,
        Self::GoChannelGoChannelI64Cap,
        Self::GoChannelGoChannelI64Send,
        Self::GoChannelGoChannelI64ReceiveValue,
        Self::GoChannelGoChannelI64Receive,
        Self::GoChannelGoChannelI64Close,
        Self::GoChannelGoChannelI64IsNil,
        Self::GoChannelGoChannelI64TrySend,
        Self::GoChannelGoChannelI64TryReceive,
    ];

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
            Self::IntDiv | Self::IntRem | Self::IntShl | Self::IntShr => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64)
            }
            Self::PrintSpace | Self::PrintNewline => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::Unit)
            }
            Self::PrintBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PrintI64 => RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit),
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
