//! Stable native and runtime operation catalogs.

use crate::effects::{
    AllocationEffect, ArgumentMutationEffect, GoPanicCondition, HostIoEffect, RuntimeEffects,
};
use crate::encoding::CanonicalEncoder;
use crate::target::{TargetCapability, TargetCapability::StandardIo};
use std::fmt::{Display, Formatter};

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
            Self::FloatAdd | Self::FloatSub | Self::FloatMul | Self::FloatDiv => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::F64)
            }
            Self::FloatNeg => RuntimeSignature::new(F64_PARAMETER, RuntimeType::F64),
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

/// Value categories supported at the typed runtime call boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeType {
    Unit,
    Bool,
    I64,
    GoString,
    ByteSlice,
    StaticByteSlice,
    F64,
    Complex128,
}

impl RuntimeType {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Unit => 1,
            Self::Bool => 2,
            Self::I64 => 3,
            Self::GoString => 4,
            Self::ByteSlice => 5,
            Self::StaticByteSlice => 6,
            Self::F64 => 7,
            Self::Complex128 => 8,
        }
    }

    fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u8(self.canonical_tag());
    }
}

/// Complete function signature for one runtime operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeSignature {
    parameters: &'static [RuntimeType],
    result: RuntimeType,
}

impl RuntimeSignature {
    const fn new(parameters: &'static [RuntimeType], result: RuntimeType) -> Self {
        Self { parameters, result }
    }

    #[must_use]
    pub const fn parameters(self) -> &'static [RuntimeType] {
        self.parameters
    }

    #[must_use]
    pub const fn result(self) -> RuntimeType {
        self.result
    }

    fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.count(self.parameters.len());
        for parameter in self.parameters {
            parameter.encode(encoder);
        }
        self.result.encode(encoder);
    }
}

const NO_PARAMETERS: &[RuntimeType] = &[];
const BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::ByteSlice];
const STATIC_BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::StaticByteSlice];
const BOOL_PARAMETER: &[RuntimeType] = &[RuntimeType::Bool];
const TWO_BOOL_PARAMETERS: &[RuntimeType] = &[RuntimeType::Bool, RuntimeType::Bool];
const I64_PARAMETER: &[RuntimeType] = &[RuntimeType::I64];
const TWO_I64_PARAMETERS: &[RuntimeType] = &[RuntimeType::I64, RuntimeType::I64];
const F64_PARAMETER: &[RuntimeType] = &[RuntimeType::F64];
const TWO_F64_PARAMETERS: &[RuntimeType] = &[RuntimeType::F64, RuntimeType::F64];
const COMPLEX128_PARAMETER: &[RuntimeType] = &[RuntimeType::Complex128];
const TWO_COMPLEX128_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::Complex128, RuntimeType::Complex128];
const GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoString];
const TWO_GO_STRING_PARAMETERS: &[RuntimeType] = &[RuntimeType::GoString, RuntimeType::GoString];
const NO_CAPABILITIES: &[TargetCapability] = &[];
const STANDARD_IO_CAPABILITY: &[TargetCapability] = &[StandardIo];
const NO_GO_PANICS: &[GoPanicCondition] = &[];
const INTEGER_DIVIDE_BY_ZERO: &[GoPanicCondition] = &[GoPanicCondition::IntegerDivideByZero];
const NEGATIVE_SHIFT_AMOUNT: &[GoPanicCondition] = &[GoPanicCondition::NegativeShiftAmount];
const EXPLICIT_PANIC: &[GoPanicCondition] = &[GoPanicCondition::ExplicitPanic];

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
}

/// Stable compact identity of one runtime ABI operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeOpId(u16);

impl RuntimeOpId {
    /// Canonical numeric value used by fingerprints and manifest encodings.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Stable operation ID that is not defined by this ABI crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownRuntimeOpId(u16);

impl UnknownRuntimeOpId {
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Display for UnknownRuntimeOpId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "unknown runtime operation ID {}", self.0)
    }
}

impl std::error::Error for UnknownRuntimeOpId {}

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
    ];

    /// Stable exported Rust symbol assigned to this ABI operation.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::GoStringFromBytes => "go_string_from_bytes",
            Self::GoStringFromStatic => "go_string_from_static",
            Self::ConcatGoStrings => "concat_go_strings",
            Self::IntDiv => "int_div",
            Self::IntRem => "int_rem",
            Self::IntShl => "int_shl",
            Self::IntShr => "int_shr",
            Self::PrintBool => "print_bool",
            Self::PrintI64 => "print_i64",
            Self::PrintSpace => "print_space",
            Self::PrintNewline => "print_newline",
            Self::PrintGoString => "print_go_string",
            Self::PanicBool => "panic_bool",
            Self::PanicI64 => "panic_i64",
            Self::PanicGoString => "panic_go_string",
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
            Self::IntDiv | Self::IntRem | Self::IntShl | Self::IntShr => {
                RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64)
            }
            Self::PrintSpace | Self::PrintNewline => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::Unit)
            }
            Self::PrintBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PrintI64 => RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit),
            Self::PrintGoString => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::Unit),
            Self::PanicBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PanicI64 => RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit),
            Self::PanicGoString => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::Unit),
        }
    }

    /// Host facilities required to invoke this operation.
    #[must_use]
    pub const fn required_capabilities(self) -> &'static [TargetCapability] {
        match self {
            Self::PrintBool
            | Self::PrintI64
            | Self::PrintSpace
            | Self::PrintNewline
            | Self::PrintGoString => STANDARD_IO_CAPABILITY,
            Self::GoStringFromBytes
            | Self::GoStringFromStatic
            | Self::ConcatGoStrings
            | Self::IntDiv
            | Self::IntRem
            | Self::IntShl
            | Self::IntShr
            | Self::PanicBool
            | Self::PanicI64
            | Self::PanicGoString => NO_CAPABILITIES,
        }
    }

    /// Allocation, host-I/O, and Go-panic behavior of this operation.
    #[must_use]
    pub const fn effects(self) -> RuntimeEffects {
        match self {
            Self::GoStringFromBytes => RuntimeEffects::new(
                AllocationEffect::MayAllocate,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
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
            ),
            Self::GoStringFromStatic => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                NO_GO_PANICS,
            ),
            Self::PanicBool | Self::PanicI64 | Self::PanicGoString => RuntimeEffects::new(
                AllocationEffect::None,
                ArgumentMutationEffect::None,
                HostIoEffect::None,
                EXPLICIT_PANIC,
            ),
        }
    }

    /// Stable compact identity for canonical encodings and fingerprints.
    #[must_use]
    pub const fn id(self) -> RuntimeOpId {
        RuntimeOpId(match self {
            Self::GoStringFromBytes => 1,
            Self::GoStringFromStatic => 2,
            Self::ConcatGoStrings => 3,
            Self::IntDiv => 8,
            Self::IntRem => 9,
            Self::IntShl => 10,
            Self::IntShr => 11,
            Self::PrintBool => 13,
            Self::PrintI64 => 14,
            Self::PrintSpace => 15,
            Self::PrintNewline => 16,
            Self::PrintGoString => 17,
            Self::PanicBool => 18,
            Self::PanicI64 => 19,
            Self::PanicGoString => 20,
        })
    }

    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u16(self.id().get());
        encoder.text(self.symbol());
        self.signature().encode(encoder);
        self.effects().encode(encoder);
        let requirements = self.required_capabilities();
        encoder.count(requirements.len());
        for requirement in requirements {
            encoder.u16(requirement.canonical_tag());
        }
    }
}

impl TryFrom<u16> for RuntimeOp {
    type Error = UnknownRuntimeOpId;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::GoStringFromBytes),
            2 => Ok(Self::GoStringFromStatic),
            3 => Ok(Self::ConcatGoStrings),
            8 => Ok(Self::IntDiv),
            9 => Ok(Self::IntRem),
            10 => Ok(Self::IntShl),
            11 => Ok(Self::IntShr),
            13 => Ok(Self::PrintBool),
            14 => Ok(Self::PrintI64),
            15 => Ok(Self::PrintSpace),
            16 => Ok(Self::PrintNewline),
            17 => Ok(Self::PrintGoString),
            18 => Ok(Self::PanicBool),
            19 => Ok(Self::PanicI64),
            20 => Ok(Self::PanicGoString),
            unknown => Err(UnknownRuntimeOpId(unknown)),
        }
    }
}
