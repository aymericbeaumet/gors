//! MIR type and representation compatibility rules.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Ty};

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
    matches!(
        (value, underlying),
        (ConstValue::Bool(_), Ty::Bool)
            | (ConstValue::Int(_), Ty::Int(IntTy::Int))
            | (ConstValue::Int(_), Ty::Int(IntTy::Int32))
            | (
                ConstValue::Int(_),
                Ty::Uint(crate::compiler::types::UintTy::Uint8)
            )
            | (
                ConstValue::Int(_),
                Ty::Uint(crate::compiler::types::UintTy::Uintptr)
            )
            | (ConstValue::Float(_), Ty::Float(FloatTy::Float64))
            | (ConstValue::Int(_), Ty::Float(FloatTy::Float64))
            | (
                ConstValue::Complex { .. },
                Ty::Complex(ComplexTy::Complex128)
            )
            | (ConstValue::Int(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::Float(_), Ty::Complex(ComplexTy::Complex128))
            | (ConstValue::String(_), Ty::String)
    )
    .then_some(())
    .ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR constant {value:?} does not have declared type {ty:?}"
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
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int) | Ty::Float(FloatTy::Float64)
                )
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
                        | Ty::Int(IntTy::Int)
                        | Ty::Int(IntTy::Int32)
                        | Ty::Uint(crate::compiler::types::UintTy::Uint8)
                        | Ty::Uint(crate::compiler::types::UintTy::Uintptr)
                        | Ty::Float(FloatTy::Float64)
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
                    Ty::Int(IntTy::Int)
                        | Ty::Int(IntTy::Int32)
                        | Ty::Uint(crate::compiler::types::UintTy::Uint8)
                        | Ty::Uint(crate::compiler::types::UintTy::Uintptr)
                        | Ty::Float(FloatTy::Float64)
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
