//! MIR type and representation compatibility rules.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Ty, UintTy};

pub(super) fn verify_bootstrap_type(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if *ty == Ty::Unit || ty.is_bootstrap_value() {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "unsupported type reached {context} MIR: {ty:?}"
        )))
    }
}

pub(super) fn verify_constant_type(value: &ConstValue, ty: &Ty) -> Result<(), Diagnostic> {
    let underlying = ty.underlying();
    let shape_matches = matches!(
        (value, underlying),
        (ConstValue::Bool(_), Ty::Bool)
            | (ConstValue::Int(_), Ty::Int(IntTy::Int))
            | (ConstValue::Int(_), Ty::Int(IntTy::Int8))
            | (ConstValue::Int(_), Ty::Int(IntTy::Int32))
            | (ConstValue::Int(_), Ty::Uint(UintTy::Uint))
            | (ConstValue::Int(_), Ty::Uint(UintTy::Uint8))
            | (ConstValue::Int(_), Ty::Uint(UintTy::Uintptr))
            | (ConstValue::Float(_), Ty::Float(_))
            | (ConstValue::Int(_), Ty::Float(_))
            | (
                ConstValue::Complex { .. },
                Ty::Complex(ComplexTy::Complex128)
            )
            | (ConstValue::Int(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::Float(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::String(_), Ty::String)
    );
    (shape_matches && value.is_representable_as(ty))
        .then_some(())
        .ok_or_else(|| {
            Diagnostic::backend(format!(
                "MIR constant {value:?} is not representable as declared type {ty:?}"
            ))
        })
}

pub(super) fn verify_same_type(
    actual: &Ty,
    expected: &Ty,
    context: &str,
) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} type mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

pub(super) fn same_mir_representation(left: &Ty, right: &Ty) -> bool {
    left.underlying() == right.underlying()
        || matches!(
            (left.underlying(), right.underlying()),
            (Ty::Channel(_, left), Ty::Channel(_, right)) if left == right
        )
        || matches!(
            (left.underlying(), right.underlying()),
            (
                Ty::Int(IntTy::Int32) | Ty::Uint(UintTy::Uint8),
                Ty::Int(IntTy::Int)
            )
        )
        || matches!(
            (left.underlying(), right.underlying()),
            (Ty::Interface(_), Ty::Interface(_))
        )
        || matches!(
            (left.underlying(), right.underlying()),
            (Ty::Float(_), Ty::Float(_))
        )
}

pub(super) fn verify_binary_types(
    op: hir::BinaryOp,
    left: &Ty,
    right: &Ty,
    result: &Ty,
) -> Result<(), Diagnostic> {
    let same_operands = left == right;
    let same_result = result == left;
    let underlying = left.underlying();
    let valid = match op {
        hir::BinaryOp::Add => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Int(IntTy::Int32)
                        | Ty::Uint(UintTy::Uint8)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                )
        }
        hir::BinaryOp::Sub | hir::BinaryOp::Mul | hir::BinaryOp::Div => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                )
        }
        hir::BinaryOp::Min | hir::BinaryOp::Max => {
            same_operands && same_result && matches!(underlying, Ty::Int(IntTy::Int) | Ty::Float(_))
        }
        hir::BinaryOp::Complex => {
            same_operands
                && *underlying == Ty::Float(FloatTy::Float64)
                && result == &Ty::Complex(ComplexTy::Complex128)
        }
        hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => {
            same_operands && same_result && *underlying == Ty::Int(IntTy::Int)
        }
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            same_operands
                && (matches!(
                    underlying,
                    Ty::Bool
                        | Ty::Int(IntTy::Int | IntTy::Int8 | IntTy::Int32)
                        | Ty::Uint(UintTy::Uint | UintTy::Uint8 | UintTy::Uintptr)
                        | Ty::Float(_)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                ) || underlying.is_bootstrap_comparable_aggregate())
                && result == &Ty::Bool
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => {
            same_operands
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int | IntTy::Int8 | IntTy::Int32)
                        | Ty::Uint(UintTy::Uint | UintTy::Uint8 | UintTy::Uintptr)
                        | Ty::Float(_)
                        | Ty::String
                )
                && result == &Ty::Bool
        }
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => {
            left == &Ty::Bool && right == &Ty::Bool && result == &Ty::Bool
        }
    };
    valid.then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid MIR binary operation {op:?}: {left:?}, {right:?} -> {result:?}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::verify_constant_type;
    use crate::compiler::types::{ConstValue, IntTy, Ty, UintTy};

    #[test]
    fn constant_verification_enforces_exact_integer_representation_bounds() {
        assert!(
            verify_constant_type(&ConstValue::Int("127".into()), &Ty::Int(IntTy::Int8)).is_ok()
        );
        assert!(
            verify_constant_type(&ConstValue::Int("128".into()), &Ty::Int(IntTy::Int8)).is_err()
        );
        assert!(
            verify_constant_type(
                &ConstValue::Int(i64::MAX.to_string()),
                &Ty::Uint(UintTy::Uint),
            )
            .is_ok()
        );
        assert!(
            verify_constant_type(
                &ConstValue::Int("9223372036854775808".into()),
                &Ty::Uint(UintTy::Uint),
            )
            .is_err()
        );
    }
}
