use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::lowering::type_lowering::integer_kind;
use crate::compiler::rust_ir as out;
use crate::compiler::types::{ConstValue, FloatTy, Ty};
use gors_runtime_abi::{IntegerPrimitive, IntegerRuntimeOp, PrimitiveOp, RuntimeOp};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

pub(super) fn lower_constant(value: ConstValue, ty: &Ty) -> Result<out::Constant, Diagnostic> {
    match (value, ty.underlying()) {
        (ConstValue::Bool(value), Ty::Bool) => Ok(out::Constant::Bool(value)),
        (ConstValue::Int(value), Ty::Int(_) | Ty::Uint(_)) => {
            let kind = integer_kind(ty).ok_or_else(|| {
                Diagnostic::backend(format!("missing Rust IR integer representation for {ty:?}"))
            })?;
            let exact = value.parse::<BigInt>().map_err(|_| {
                Diagnostic::backend(format!("invalid canonical Go integer: {value}"))
            })?;
            let bits = if kind.is_signed() {
                exact.to_i64()
            } else {
                exact.to_u64().map(|value| value as i64)
            }
            .ok_or_else(|| {
                Diagnostic::backend(format!("Go integer is outside {kind:?}: {value}"))
            })?;
            if !kind.is_canonical_carrier(bits) {
                return Err(Diagnostic::backend(format!(
                    "Go integer has a non-canonical {kind:?} carrier: {value}"
                )));
            }
            Ok(out::Constant::Integer { kind, bits })
        }
        (ConstValue::String(bytes), Ty::String) => Ok(out::Constant::RuntimeStaticBytes {
            op: RuntimeOp::GoStringFromStatic,
            bytes,
        }),
        (value @ (ConstValue::Float(_) | ConstValue::Int(_)), Ty::Float(float_ty)) => value
            .ieee_bits_for(*float_ty)
            .and_then(|bits| lower_float_bits(bits, *float_ty))
            .map(out::Constant::F64)
            .ok_or_else(|| {
                Diagnostic::backend(format!("invalid canonical Go float constant: {value:?}"))
            }),
        (value @ (ConstValue::Int(_) | ConstValue::Float(_)), Ty::Complex(complex_ty)) => value
            .ieee_bits_for(complex_ty.component_type())
            .and_then(|bits| lower_float_bits(bits, complex_ty.component_type()))
            .map(|real| out::Constant::Complex128 {
                real,
                imag: 0.0_f64.to_bits(),
            })
            .ok_or_else(|| {
                Diagnostic::backend(format!("invalid canonical Go complex constant: {value:?}"))
            }),
        (ConstValue::Complex { real, imag }, Ty::Complex(complex_ty)) => {
            let component_ty = complex_ty.component_type();
            let real = ConstValue::Float(real)
                .ieee_bits_for(component_ty)
                .and_then(|bits| lower_float_bits(bits, component_ty))
                .ok_or_else(|| Diagnostic::backend("invalid real complex128 component"))?;
            let imag = ConstValue::Float(imag)
                .ieee_bits_for(component_ty)
                .and_then(|bits| lower_float_bits(bits, component_ty))
                .ok_or_else(|| Diagnostic::backend("invalid imaginary complex128 component"))?;
            Ok(out::Constant::Complex128 { real, imag })
        }
        (value, ty) => Err(Diagnostic::backend(format!(
            "invalid constant reached Rust lowering: {value:?} as {ty:?}"
        ))),
    }
}

fn lower_float_bits(bits: u64, ty: FloatTy) -> Option<u64> {
    match ty {
        FloatTy::Float32 => u32::try_from(bits)
            .ok()
            .map(f32::from_bits)
            .map(f64::from)
            .map(f64::to_bits),
        FloatTy::Float64 => Some(bits),
    }
}

pub(super) fn lower_unary_op(
    op: hir::UnaryOp,
    _go_result: &Ty,
    operand: out::RustType,
    result: out::RustType,
) -> Result<Option<out::ValueOp>, Diagnostic> {
    let lowered = match (op, operand, result) {
        (
            hir::UnaryOp::Positive,
            out::RustType::Integer(operand),
            out::RustType::Integer(result),
        ) if operand == result => None,
        (hir::UnaryOp::Positive, out::RustType::F64, out::RustType::F64)
        | (hir::UnaryOp::Positive, out::RustType::Complex128, out::RustType::Complex128) => None,
        (
            hir::UnaryOp::Negative,
            out::RustType::Integer(operand),
            out::RustType::Integer(result),
        ) if operand == result => Some(out::ValueOp::Primitive(PrimitiveOp::Integer {
            op: IntegerPrimitive::WrappingNeg,
            kind: result,
        })),
        (hir::UnaryOp::Negative, out::RustType::F64, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::FloatNeg))
        }
        (hir::UnaryOp::Negative, out::RustType::Complex128, out::RustType::Complex128) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexNeg))
        }
        (hir::UnaryOp::Not, out::RustType::Bool, out::RustType::Bool) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::BoolNot))
        }
        (hir::UnaryOp::BitNot, out::RustType::Integer(operand), out::RustType::Integer(result))
            if operand == result =>
        {
            Some(out::ValueOp::Primitive(PrimitiveOp::Integer {
                op: IntegerPrimitive::BitNot,
                kind: result,
            }))
        }
        (hir::UnaryOp::Real, out::RustType::Complex128, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexReal))
        }
        (hir::UnaryOp::Imag, out::RustType::Complex128, out::RustType::F64) => {
            Some(out::ValueOp::Primitive(PrimitiveOp::ComplexImag))
        }
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid unary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

pub(super) fn lower_binary_op(
    op: hir::BinaryOp,
    _go_result: &Ty,
    left: out::RustType,
    right: out::RustType,
    result: out::RustType,
) -> Result<out::ValueOp, Diagnostic> {
    use hir::BinaryOp as Go;
    use out::RustType::{Bool, Complex128, F64, GoString, Integer};
    use out::ValueOp::{Primitive, Runtime};
    let integer_op = |op, kind| Primitive(PrimitiveOp::Integer { op, kind });
    let lowered = match (op, left, right, result) {
        (Go::Add, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::WrappingAdd, result)
        }
        (Go::Sub, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::WrappingSub, result)
        }
        (Go::Mul, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::WrappingMul, result)
        }
        (Go::Div, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            Runtime(RuntimeOp::Integer {
                op: IntegerRuntimeOp::Div,
                kind: result,
            })
        }
        (Go::Rem, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            Runtime(RuntimeOp::Integer {
                op: IntegerRuntimeOp::Rem,
                kind: result,
            })
        }
        (Go::BitAnd, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::BitAnd, result)
        }
        (Go::BitOr, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::BitOr, result)
        }
        (Go::BitXor, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::BitXor, result)
        }
        (Go::Shl, Integer(left), Integer(right), Integer(result)) if left == result => {
            Runtime(RuntimeOp::Integer {
                op: if right.is_signed() {
                    IntegerRuntimeOp::ShlSigned
                } else {
                    IntegerRuntimeOp::ShlUnsigned
                },
                kind: result,
            })
        }
        (Go::Shr, Integer(left), Integer(right), Integer(result)) if left == result => {
            Runtime(RuntimeOp::Integer {
                op: if right.is_signed() {
                    IntegerRuntimeOp::ShrSigned
                } else {
                    IntegerRuntimeOp::ShrUnsigned
                },
                kind: result,
            })
        }
        (Go::AndNot, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::AndNot, result)
        }
        (Go::Equal, Bool, Bool, Bool) => Primitive(PrimitiveOp::BoolEqual),
        (Go::NotEqual, Bool, Bool, Bool) => Primitive(PrimitiveOp::BoolNotEqual),
        (Go::Equal, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::Equal, left)
        }
        (Go::NotEqual, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::NotEqual, left)
        }
        (Go::Less, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::Less, left)
        }
        (Go::LessEqual, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::LessEqual, left)
        }
        (Go::Greater, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::Greater, left)
        }
        (Go::GreaterEqual, Integer(left), Integer(right), Bool) if left == right => {
            integer_op(IntegerPrimitive::GreaterEqual, left)
        }
        (Go::Min, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::Min, result)
        }
        (Go::Max, Integer(left), Integer(right), Integer(result))
            if left == right && left == result =>
        {
            integer_op(IntegerPrimitive::Max, result)
        }
        (Go::Add, F64, F64, F64) => Primitive(PrimitiveOp::FloatAdd),
        (Go::Sub, F64, F64, F64) => Primitive(PrimitiveOp::FloatSub),
        (Go::Mul, F64, F64, F64) => Primitive(PrimitiveOp::FloatMul),
        (Go::Div, F64, F64, F64) => Primitive(PrimitiveOp::FloatDiv),
        (Go::Equal, F64, F64, Bool) => Primitive(PrimitiveOp::FloatEqual),
        (Go::NotEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatNotEqual),
        (Go::Less, F64, F64, Bool) => Primitive(PrimitiveOp::FloatLess),
        (Go::LessEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatLessEqual),
        (Go::Greater, F64, F64, Bool) => Primitive(PrimitiveOp::FloatGreater),
        (Go::GreaterEqual, F64, F64, Bool) => Primitive(PrimitiveOp::FloatGreaterEqual),
        (Go::Min, F64, F64, F64) => Primitive(PrimitiveOp::FloatMin),
        (Go::Max, F64, F64, F64) => Primitive(PrimitiveOp::FloatMax),
        (Go::Complex, F64, F64, Complex128) => Primitive(PrimitiveOp::ComplexFromParts),
        (Go::Add, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexAdd),
        (Go::Sub, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexSub),
        (Go::Mul, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexMul),
        (Go::Div, Complex128, Complex128, Complex128) => Primitive(PrimitiveOp::ComplexDiv),
        (Go::Equal, Complex128, Complex128, Bool) => Primitive(PrimitiveOp::ComplexEqual),
        (Go::NotEqual, Complex128, Complex128, Bool) => Primitive(PrimitiveOp::ComplexNotEqual),
        (Go::Add, GoString, GoString, GoString) => Runtime(RuntimeOp::ConcatGoStrings),
        (Go::Equal, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringEqual),
        (Go::NotEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringNotEqual),
        (Go::Less, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringLess),
        (Go::LessEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringLessEqual),
        (Go::Greater, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringGreater),
        (Go::GreaterEqual, GoString, GoString, Bool) => Primitive(PrimitiveOp::StringGreaterEqual),
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid binary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gors_runtime_abi::IntegerKind;

    #[test]
    fn every_integer_kind_selects_the_parameterized_division_and_remainder_runtime_ops() {
        for kind in IntegerKind::ALL {
            let ty = out::RustType::Integer(*kind);
            for (go, runtime) in [
                (hir::BinaryOp::Div, IntegerRuntimeOp::Div),
                (hir::BinaryOp::Rem, IntegerRuntimeOp::Rem),
            ] {
                let expected = out::ValueOp::Runtime(RuntimeOp::Integer {
                    op: runtime,
                    kind: *kind,
                });
                let actual = lower_binary_op(
                    go,
                    &Ty::Int(crate::compiler::types::IntTy::Int),
                    ty.clone(),
                    ty.clone(),
                    ty.clone(),
                );
                assert!(
                    matches!(&actual, Ok(value) if value == &expected),
                    "{actual:?}"
                );
            }
        }
    }

    #[test]
    fn every_lhs_and_count_kind_selects_the_exact_shift_runtime_member() {
        for lhs in IntegerKind::ALL {
            for count in IntegerKind::ALL {
                let lhs_ty = out::RustType::Integer(*lhs);
                let count_ty = out::RustType::Integer(*count);
                for (go, signed, unsigned) in [
                    (
                        hir::BinaryOp::Shl,
                        IntegerRuntimeOp::ShlSigned,
                        IntegerRuntimeOp::ShlUnsigned,
                    ),
                    (
                        hir::BinaryOp::Shr,
                        IntegerRuntimeOp::ShrSigned,
                        IntegerRuntimeOp::ShrUnsigned,
                    ),
                ] {
                    let expected = out::ValueOp::Runtime(RuntimeOp::Integer {
                        op: if count.is_signed() { signed } else { unsigned },
                        kind: *lhs,
                    });
                    let actual = lower_binary_op(
                        go,
                        &Ty::Int(crate::compiler::types::IntTy::Int),
                        lhs_ty.clone(),
                        count_ty.clone(),
                        lhs_ty.clone(),
                    );
                    assert!(
                        matches!(&actual, Ok(value) if value == &expected),
                        "{actual:?}"
                    );
                }
            }
        }
    }
}
