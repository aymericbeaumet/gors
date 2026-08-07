//! MIR type and representation compatibility rules.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, Ty};

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
            | (ConstValue::Int(_), Ty::Int(_))
            | (ConstValue::Int(_), Ty::Uint(_))
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
    let canonical = value.normalized_for(ty) == *value;
    (shape_matches && value.is_representable_as(ty) && canonical)
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
            (Ty::Int(_) | Ty::Uint(_), Ty::Int(_) | Ty::Uint(_))
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
                    Ty::Int(_)
                        | Ty::Uint(_)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                )
        }
        hir::BinaryOp::Sub | hir::BinaryOp::Mul => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(_)
                        | Ty::Uint(_)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                )
        }
        hir::BinaryOp::Div => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(_)
                        | Ty::Uint(_)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                )
        }
        hir::BinaryOp::Min | hir::BinaryOp::Max => {
            same_operands
                && same_result
                && matches!(underlying, Ty::Int(_) | Ty::Uint(_) | Ty::Float(_))
        }
        hir::BinaryOp::Complex => {
            same_operands
                && *underlying == Ty::Float(FloatTy::Float64)
                && result == &Ty::Complex(ComplexTy::Complex128)
        }
        hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::AndNot => {
            same_operands && same_result && matches!(underlying, Ty::Int(_) | Ty::Uint(_))
        }
        hir::BinaryOp::Rem => {
            same_operands && same_result && matches!(underlying, Ty::Int(_) | Ty::Uint(_))
        }
        hir::BinaryOp::Shl | hir::BinaryOp::Shr => {
            same_result
                && matches!(underlying, Ty::Int(_) | Ty::Uint(_))
                && matches!(right.underlying(), Ty::Int(_) | Ty::Uint(_))
        }
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            same_operands
                && (matches!(
                    underlying,
                    Ty::Bool
                        | Ty::Int(_)
                        | Ty::Uint(_)
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
                    Ty::Int(_) | Ty::Uint(_) | Ty::Float(_) | Ty::String
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
    use crate::compiler::types::{ConstValue, FloatTy, IntTy, Ty, UintTy};

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
                &ConstValue::Int(u64::MAX.to_string()),
                &Ty::Uint(UintTy::Uint),
            )
            .is_ok()
        );
        assert!(
            verify_constant_type(
                &ConstValue::Int("18446744073709551616".into()),
                &Ty::Uint(UintTy::Uint64),
            )
            .is_err()
        );
    }

    #[test]
    fn constant_verification_rejects_unquantized_typed_floats() {
        assert!(
            verify_constant_type(
                &ConstValue::Int("16777217".into()),
                &Ty::Float(FloatTy::Float32),
            )
            .is_err()
        );
        assert!(
            verify_constant_type(
                &ConstValue::Int("16777216".into()),
                &Ty::Float(FloatTy::Float32),
            )
            .is_ok()
        );
        assert!(
            verify_constant_type(
                &ConstValue::Int("9007199254740993".into()),
                &Ty::Float(FloatTy::Float64),
            )
            .is_err()
        );
    }
}
