//! Stable directly emitted operation catalog.

use super::signature_types::*;
use super::{
    FloatKind, FloatPrimitive, IntegerKind, IntegerPrimitive, RuntimeSignature, RuntimeType,
};
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
    /// One float32 operation using the shared physical `f64` carrier.
    Float32 {
        op: FloatPrimitive,
    },
    IntegerToFloat {
        from: IntegerKind,
        to: FloatKind,
    },
    FloatToInteger {
        from: FloatKind,
        to: IntegerKind,
    },
    /// Widen one canonical float32 carrier without changing its physical form.
    FloatWiden64,
    Complex128ToComplex64,
    Complex64ToComplex128,
    Complex64Real,
    Complex64Imag,
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

    const LEGACY_NON_INTEGER: &'static [Self] = &[
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

    const CATALOG: [Self; 283] = primitive_catalog();

    /// Select the exact operation for a Go floating-point width.
    ///
    /// Float64 keeps the original operation variants and their canonical
    /// contracts. Float32 selects the appended width-specific catalog.
    #[must_use]
    pub const fn float(op: FloatPrimitive, kind: FloatKind) -> Self {
        match (op, kind) {
            (FloatPrimitive::Add, FloatKind::F64) => Self::FloatAdd,
            (FloatPrimitive::Sub, FloatKind::F64) => Self::FloatSub,
            (FloatPrimitive::Mul, FloatKind::F64) => Self::FloatMul,
            (FloatPrimitive::Div, FloatKind::F64) => Self::FloatDiv,
            (FloatPrimitive::Neg, FloatKind::F64) => Self::FloatNeg,
            (FloatPrimitive::Equal, FloatKind::F64) => Self::FloatEqual,
            (FloatPrimitive::NotEqual, FloatKind::F64) => Self::FloatNotEqual,
            (FloatPrimitive::Less, FloatKind::F64) => Self::FloatLess,
            (FloatPrimitive::LessEqual, FloatKind::F64) => Self::FloatLessEqual,
            (FloatPrimitive::Greater, FloatKind::F64) => Self::FloatGreater,
            (FloatPrimitive::GreaterEqual, FloatKind::F64) => Self::FloatGreaterEqual,
            (FloatPrimitive::Min, FloatKind::F64) => Self::FloatMin,
            (FloatPrimitive::Max, FloatKind::F64) => Self::FloatMax,
            (op, FloatKind::F32) => Self::Float32 { op },
        }
    }

    /// Select a width-changing floating-point conversion, if one is needed.
    #[must_use]
    pub const fn float_conversion(from: FloatKind, to: FloatKind) -> Option<Self> {
        match (from, to) {
            (FloatKind::F64, FloatKind::F32) => Some(Self::FloatRound32),
            (FloatKind::F32, FloatKind::F64) => Some(Self::FloatWiden64),
            (FloatKind::F32, FloatKind::F32) | (FloatKind::F64, FloatKind::F64) => None,
        }
    }

    /// Select a width-changing complex conversion, if one is needed.
    #[must_use]
    pub const fn complex_conversion(from: FloatKind, to: FloatKind) -> Option<Self> {
        match (from, to) {
            (FloatKind::F64, FloatKind::F32) => Some(Self::Complex128ToComplex64),
            (FloatKind::F32, FloatKind::F64) => Some(Self::Complex64ToComplex128),
            (FloatKind::F32, FloatKind::F32) | (FloatKind::F64, FloatKind::F64) => None,
        }
    }

    /// Select real-component extraction for a complex component width.
    #[must_use]
    pub const fn complex_real(kind: FloatKind) -> Self {
        match kind {
            FloatKind::F32 => Self::Complex64Real,
            FloatKind::F64 => Self::ComplexReal,
        }
    }

    /// Select imaginary-component extraction for a complex component width.
    #[must_use]
    pub const fn complex_imag(kind: FloatKind) -> Self {
        match kind {
            FloatKind::F32 => Self::Complex64Imag,
            FloatKind::F64 => Self::ComplexImag,
        }
    }

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
            Self::IntegerToFloat { .. } => RuntimeSignature::new(I64_PARAMETER, RuntimeType::F64),
            Self::FloatToInteger { .. } => RuntimeSignature::new(F64_PARAMETER, RuntimeType::I64),
            Self::FloatAdd | Self::FloatSub | Self::FloatMul | Self::FloatDiv => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::F64)
            }
            Self::FloatNeg | Self::FloatRound32 | Self::FloatWiden64 => {
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
            Self::Float32 { op } if op.arity() == 1 => {
                RuntimeSignature::new(F64_PARAMETER, RuntimeType::F64)
            }
            Self::Float32 { op } if op.returns_bool() => {
                RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::Bool)
            }
            Self::Float32 { .. } => RuntimeSignature::new(TWO_F64_PARAMETERS, RuntimeType::F64),
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
            Self::ComplexReal | Self::ComplexImag | Self::Complex64Real | Self::Complex64Imag => {
                RuntimeSignature::new(COMPLEX128_PARAMETER, RuntimeType::F64)
            }
            Self::Complex128ToComplex64 | Self::Complex64ToComplex128 => {
                RuntimeSignature::new(COMPLEX128_PARAMETER, RuntimeType::Complex128)
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
            Self::Float32 { op } => format!("float32-{}", op.name()),
            Self::IntegerToFloat { from, to } => {
                format!("{}-to-{}", from.name(), to.name())
            }
            Self::FloatToInteger { from, to } => {
                format!("{}-to-{}", from.name(), to.name())
            }
            other => other.non_parameterized_name().to_owned(),
        }
    }

    const fn non_parameterized_name(self) -> &'static str {
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
            Self::FloatWiden64 => "float32-to-float64",
            Self::Complex128ToComplex64 => "complex128-to-complex64",
            Self::Complex64ToComplex128 => "complex64-to-complex128",
            Self::Complex64Real => "complex64-real",
            Self::Complex64Imag => "complex64-imag",
            Self::Integer { .. }
            | Self::IntegerConvert { .. }
            | Self::Float32 { .. }
            | Self::IntegerToFloat { .. }
            | Self::FloatToInteger { .. } => "parameterized-operation",
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
            Self::Float32 { op } => 253 + op.ordinal(),
            Self::IntegerToFloat { from, to } => 266 + from.ordinal() * 2 + to.ordinal(),
            Self::FloatToInteger { from, to } => 282 + from.ordinal() * 8 + to.ordinal(),
            Self::FloatWiden64 => 298,
            Self::Complex128ToComplex64 => 299,
            Self::Complex64ToComplex128 => 300,
            Self::Complex64Real => 301,
            Self::Complex64Imag => 302,
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
const fn primitive_catalog() -> [PrimitiveOp; 283] {
    let mut result = [PrimitiveOp::BoolNot; 283];
    let mut output = 0;
    let mut index = 0;
    while index < PrimitiveOp::LEGACY_NON_INTEGER.len() {
        result[output] = PrimitiveOp::LEGACY_NON_INTEGER[index];
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
    let mut from_integer = 0;
    while from_integer < IntegerKind::ALL.len() {
        let mut to_integer = 0;
        while to_integer < IntegerKind::ALL.len() {
            result[output] = PrimitiveOp::IntegerConvert {
                from: IntegerKind::ALL[from_integer],
                to: IntegerKind::ALL[to_integer],
            };
            output += 1;
            to_integer += 1;
        }
        from_integer += 1;
    }
    let mut float_primitive = 0;
    while float_primitive < FloatPrimitive::ALL.len() {
        result[output] = PrimitiveOp::Float32 {
            op: FloatPrimitive::ALL[float_primitive],
        };
        output += 1;
        float_primitive += 1;
    }
    from_integer = 0;
    while from_integer < IntegerKind::ALL.len() {
        let mut to_float = 0;
        while to_float < FloatKind::ALL.len() {
            result[output] = PrimitiveOp::IntegerToFloat {
                from: IntegerKind::ALL[from_integer],
                to: FloatKind::ALL[to_float],
            };
            output += 1;
            to_float += 1;
        }
        from_integer += 1;
    }
    let mut from_float = 0;
    while from_float < FloatKind::ALL.len() {
        let mut to_integer = 0;
        while to_integer < IntegerKind::ALL.len() {
            result[output] = PrimitiveOp::FloatToInteger {
                from: FloatKind::ALL[from_float],
                to: IntegerKind::ALL[to_integer],
            };
            output += 1;
            to_integer += 1;
        }
        from_float += 1;
    }
    result[output] = PrimitiveOp::FloatWiden64;
    output += 1;
    result[output] = PrimitiveOp::Complex128ToComplex64;
    output += 1;
    result[output] = PrimitiveOp::Complex64ToComplex128;
    output += 1;
    result[output] = PrimitiveOp::Complex64Real;
    output += 1;
    result[output] = PrimitiveOp::Complex64Imag;
    result
}
