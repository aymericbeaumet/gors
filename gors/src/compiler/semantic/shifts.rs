//! Go shift typing keeps the left value and shift count independently typed.

use super::expressions::{
    coerce_expr, constant_shift_integer_operand, ensure_bootstrap_value_type, expr_constant,
    fold_constant_binary, normalize_constant_for_type, validate_binary_operator,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Ty, UintTy, UntypedTy};

pub(super) fn lower_shift_expression(
    op: hir::BinaryOp,
    mut left: hir::Expr,
    mut right: hir::Expr,
    node: NodeId,
    source: SourceRef,
    expected: Option<&Ty>,
) -> Result<hir::Expr, Diagnostic> {
    prepare_shift_count(&mut right, source)?;
    let fully_constant = expr_constant(&left).is_some() && expr_constant(&right).is_some();

    if matches!(left.ty, Ty::Untyped(_)) {
        let value = expr_constant(&left)
            .ok_or_else(|| Diagnostic::backend("untyped shift operand was not constant"))?;
        let value = constant_shift_integer_operand(value, "shifted operand", source)?;
        replace_constant(&mut left, value)?;
        left.ty = Ty::Untyped(UntypedTy::Int);
        if let Some(expected) = expected.filter(|_| !fully_constant) {
            if !expected.is_integer() {
                return Err(Diagnostic::semantic(
                    format!("operator {op:?} is invalid for {expected:?}"),
                    source,
                ));
            }
            coerce_expr(&mut left, expected, source)?;
        }
    }

    validate_binary_operator(op, &left.ty.default_typed(), source)?;
    ensure_bootstrap_value_type(&left.ty.default_typed(), source)?;

    if let (Some(left_value), Some(right_value)) = (expr_constant(&left), expr_constant(&right)) {
        let value = fold_constant_binary(op, left_value, right_value, source)?
            .ok_or_else(|| Diagnostic::backend("integer constant shift did not fold"))?;
        let ty = left.ty.clone();
        let value = normalize_constant_for_type(value, &ty, source)?;
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Constant(value),
            ty,
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        return Ok(result);
    }

    let mut effects = left.effects.union(right.effects);
    if matches!(right.ty.underlying(), Ty::Int(_)) {
        effects.may_panic = true;
    }
    let ty = left.ty.clone();
    let mut result = hir::Expr {
        node,
        kind: hir::ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        ty,
        category: hir::ValueCategory::Value,
        effects,
        source,
    };
    if let Some(expected) = expected {
        coerce_expr(&mut result, expected, source)?;
    }
    Ok(result)
}

pub(super) fn prepare_shift_count(
    count: &mut hir::Expr,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    if let Some(value) = expr_constant(count) {
        let value = constant_shift_integer_operand(value, "shift count", source)?;
        let ConstValue::Int(spelling) = &value else {
            return Err(Diagnostic::backend(
                "normalized shift count was not an integer",
            ));
        };
        if spelling.starts_with('-') {
            return Err(Diagnostic::semantic(
                "shift count must be a non-negative integer",
                source,
            ));
        }
        if matches!(count.ty, Ty::Untyped(_)) {
            replace_constant(count, value)?;
            count.ty = Ty::Untyped(UntypedTy::Int);
            // Go keeps this source operand untyped but requires it to fit
            // `uint`; U64 is this compiler's concrete `uint` execution carrier.
            coerce_expr(count, &Ty::Uint(UintTy::Uint), source)?;
        }
    } else if !count.ty.is_integer() {
        return Err(Diagnostic::semantic(
            format!("shift count must be integer, found {:?}", count.ty),
            source,
        ));
    }

    if !count.ty.is_integer() {
        return Err(Diagnostic::semantic(
            format!("shift count must be integer, found {:?}", count.ty),
            source,
        ));
    }
    ensure_bootstrap_value_type(&count.ty.default_typed(), source)
}

fn replace_constant(expression: &mut hir::Expr, value: ConstValue) -> Result<(), Diagnostic> {
    match &mut expression.kind {
        hir::ExprKind::Constant(current) | hir::ExprKind::GlobalConstant(_, current) => {
            *current = value;
            Ok(())
        }
        _ => Err(Diagnostic::backend(
            "constant shift expression lost its constant value",
        )),
    }
}
