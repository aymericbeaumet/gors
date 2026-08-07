//! Typed value conversions that select explicit runtime representations.

use super::FunctionLowerer;
use super::expression_lower::slice_runtime_effects;
use super::expressions::{coerce_expr, is_assignable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{FloatTy, IntTy, Ty, UintTy};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_conversion_call(
        &mut self,
        callee: &ExprSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic(
                "conversions do not accept ...",
                source,
            ));
        }
        let [argument] = arguments else {
            return Err(Diagnostic::semantic(
                "a conversion requires exactly one argument",
                source,
            ));
        };
        let target = self.lower_scoped_type(callee, source)?;
        if is_nil_identifier(argument) {
            if !is_nilable_type(&target) {
                return Err(Diagnostic::semantic(
                    format!("cannot convert nil to {target:?}"),
                    source,
                ));
            }
            let mut result = self.zero_value_expr(node, source, target)?;
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        if matches!(target.underlying(), Ty::Interface(_)) {
            // A conversion to an interface type, including the predeclared
            // `any` alias, boxes the operand exactly like interface
            // assignment.
            let value = self.lower_expr(argument, None)?;
            let mut result = self.coerce_interface_value(value, &target, argument.source)?;
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        let mut argument = self.lower_expr(argument, None)?;
        let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        let rune_slice = Ty::Slice(Box::new(Ty::Int(IntTy::Int32)));
        let mut result = if target == Ty::String && argument.ty.is_integer() {
            let default_ty = argument.ty.default_typed();
            if argument.ty != default_ty {
                coerce_expr(&mut argument, &default_ty, source)?;
            }
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromRune),
                    args: vec![argument],
                },
                ty: Ty::String,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if target == Ty::String && argument.ty == byte_slice {
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceU8),
                    args: vec![argument],
                },
                ty: Ty::String,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if target == Ty::String && argument.ty == rune_slice {
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceRunes),
                    args: vec![argument],
                },
                ty: Ty::String,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if target == byte_slice && argument.ty == Ty::String {
            let empty_node = self.alloc_node(callee.source)?;
            let empty = hir::Expr {
                node: empty_node,
                kind: hir::ExprKind::SliceLiteralU8(Vec::new()),
                ty: byte_slice.clone(),
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
                source: SourceRef::node(empty_node),
            };
            let effects = slice_runtime_effects(&[&empty, &argument], true, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::SliceU8AppendString),
                    args: vec![empty, argument],
                },
                ty: byte_slice,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if let hir::ExprKind::Constant(value) = &argument.kind
            && value.is_representable_as(&target)
        {
            argument.ty = target;
            argument
        } else if is_assignable(&argument.ty, &target) {
            coerce_expr(&mut argument, &target, source)?;
            argument
        } else if argument.ty.underlying() == target.underlying()
            || is_lossless_integer_conversion(&argument.ty, &target)
            || is_float_conversion(&argument.ty, &target)
        {
            let effects = argument.effects;
            hir::Expr {
                node,
                kind: hir::ExprKind::Conversion {
                    value: Box::new(argument),
                },
                ty: target,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else {
            return Err(Diagnostic::unsupported(
                format!(
                    "conversion from {:?} to {target:?} requires a representation change",
                    argument.ty
                ),
                source,
            ));
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == "nil"
    )
}

fn is_nilable_type(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Interface(_)
            | Ty::Function(_)
            | Ty::Pointer(_)
            | Ty::Slice(_)
            | Ty::Map(_, _)
            | Ty::Channel(_, _)
    )
}

fn is_lossless_integer_conversion(from: &Ty, to: &Ty) -> bool {
    matches!(
        (from.underlying(), to.underlying()),
        (
            Ty::Int(IntTy::Int32) | Ty::Uint(UintTy::Uint8),
            Ty::Int(IntTy::Int)
        )
    )
}

fn is_float_conversion(from: &Ty, to: &Ty) -> bool {
    matches!(
        (from.underlying(), to.underlying()),
        (
            Ty::Float(FloatTy::Float32 | FloatTy::Float64),
            Ty::Float(FloatTy::Float32 | FloatTy::Float64)
        )
    )
}
