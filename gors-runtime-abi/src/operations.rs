//! Stable native and runtime operation catalogs.

use crate::encoding::CanonicalEncoder;
use crate::target::{TargetCapability, TargetCapability::StandardIo};

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
    ];

    pub(crate) const fn canonical_tag(self) -> u16 {
        match self {
            Self::BoolNot => 1,
            Self::BoolEqual => 2,
            Self::BoolNotEqual => 3,
            Self::IntBitNot => 4,
            Self::IntBitAnd => 5,
            Self::IntBitOr => 6,
            Self::IntBitXor => 7,
            Self::IntAndNot => 8,
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
        }
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
        }
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
            encoder.u8(parameter.canonical_tag());
        }
        encoder.u8(self.result.canonical_tag());
    }
}

const NO_PARAMETERS: &[RuntimeType] = &[];
const BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::ByteSlice];
const STATIC_BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::StaticByteSlice];
const BOOL_PARAMETER: &[RuntimeType] = &[RuntimeType::Bool];
const I64_PARAMETER: &[RuntimeType] = &[RuntimeType::I64];
const TWO_I64_PARAMETERS: &[RuntimeType] = &[RuntimeType::I64, RuntimeType::I64];
const GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoString];
const TWO_GO_STRING_PARAMETERS: &[RuntimeType] = &[RuntimeType::GoString, RuntimeType::GoString];
const NO_CAPABILITIES: &[TargetCapability] = &[];
const STANDARD_IO_CAPABILITY: &[TargetCapability] = &[StandardIo];

/// Operations that require an exact symbol from the versioned runtime ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeOp {
    GoStringFromBytes,
    GoStringFromStatic,
    ConcatGoStrings,
    IntAdd,
    IntSub,
    IntMul,
    IntNeg,
    IntDiv,
    IntRem,
    IntShl,
    IntShr,
    PrintEmpty,
    PrintBool,
    PrintI64,
    PrintSpace,
    PrintNewline,
    PrintGoString,
}

impl RuntimeOp {
    /// Complete helper catalog for the current contract.
    pub const ALL: &'static [Self] = &[
        Self::GoStringFromBytes,
        Self::GoStringFromStatic,
        Self::ConcatGoStrings,
        Self::IntAdd,
        Self::IntSub,
        Self::IntMul,
        Self::IntNeg,
        Self::IntDiv,
        Self::IntRem,
        Self::IntShl,
        Self::IntShr,
        Self::PrintEmpty,
        Self::PrintBool,
        Self::PrintI64,
        Self::PrintSpace,
        Self::PrintNewline,
        Self::PrintGoString,
    ];

    /// Stable exported Rust symbol assigned to this ABI operation.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::GoStringFromBytes => "go_string_from_bytes",
            Self::GoStringFromStatic => "go_string_from_static",
            Self::ConcatGoStrings => "concat_go_strings",
            Self::IntAdd => "int_add",
            Self::IntSub => "int_sub",
            Self::IntMul => "int_mul",
            Self::IntNeg => "int_neg",
            Self::IntDiv => "int_div",
            Self::IntRem => "int_rem",
            Self::IntShl => "int_shl",
            Self::IntShr => "int_shr",
            Self::PrintEmpty => "print_empty",
            Self::PrintBool => "print_bool",
            Self::PrintI64 => "print_i64",
            Self::PrintSpace => "print_space",
            Self::PrintNewline => "print_newline",
            Self::PrintGoString => "print_go_string",
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
            Self::IntAdd
            | Self::IntSub
            | Self::IntMul
            | Self::IntDiv
            | Self::IntRem
            | Self::IntShl
            | Self::IntShr => RuntimeSignature::new(TWO_I64_PARAMETERS, RuntimeType::I64),
            Self::IntNeg => RuntimeSignature::new(I64_PARAMETER, RuntimeType::I64),
            Self::PrintEmpty | Self::PrintSpace | Self::PrintNewline => {
                RuntimeSignature::new(NO_PARAMETERS, RuntimeType::Unit)
            }
            Self::PrintBool => RuntimeSignature::new(BOOL_PARAMETER, RuntimeType::Unit),
            Self::PrintI64 => RuntimeSignature::new(I64_PARAMETER, RuntimeType::Unit),
            Self::PrintGoString => RuntimeSignature::new(GO_STRING_PARAMETER, RuntimeType::Unit),
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
            | Self::IntAdd
            | Self::IntSub
            | Self::IntMul
            | Self::IntNeg
            | Self::IntDiv
            | Self::IntRem
            | Self::IntShl
            | Self::IntShr
            | Self::PrintEmpty => NO_CAPABILITIES,
        }
    }

    pub(crate) const fn canonical_tag(self) -> u16 {
        match self {
            Self::GoStringFromBytes => 1,
            Self::GoStringFromStatic => 2,
            Self::ConcatGoStrings => 3,
            Self::IntAdd => 4,
            Self::IntSub => 5,
            Self::IntMul => 6,
            Self::IntNeg => 7,
            Self::IntDiv => 8,
            Self::IntRem => 9,
            Self::IntShl => 10,
            Self::IntShr => 11,
            Self::PrintEmpty => 12,
            Self::PrintBool => 13,
            Self::PrintI64 => 14,
            Self::PrintSpace => 15,
            Self::PrintNewline => 16,
            Self::PrintGoString => 17,
        }
    }

    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u16(self.canonical_tag());
        encoder.text(self.symbol());
        self.signature().encode(encoder);
        let requirements = self.required_capabilities();
        encoder.count(requirements.len());
        for requirement in requirements {
            encoder.u16(requirement.canonical_tag());
        }
    }
}
